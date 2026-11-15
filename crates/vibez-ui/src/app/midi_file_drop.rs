//! Lands dropped MIDI files as note Clips on the target the drop resolved.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use vibez_core::id::TrackId;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::midi_file::MidiFile;
use vibez_core::track::MediaSourceRef;
use vibez_project::TimelineLocation;

use super::App;
use crate::domains::midi_file_import::{
    MidiClipTrack, MidiFileImport, MidiFileLayout, MidiFileLayoutChoice,
};
use crate::domains::midi_file_landing::{MidiLanding, MidiLandingCtx};
use crate::message::{BrowserImportTarget, Message};
use crate::state::{ArrangementTimeline, UiNoteClip};

async fn read_midi_file_async(path: PathBuf) -> Result<MidiFileImport, String> {
    tokio::task::spawn_blocking(move || {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "MIDI".into());
        let bytes =
            std::fs::read(&path).map_err(|error| format!("Couldn't read {name}: {error}"))?;
        let file = MidiFile::read(&bytes).map_err(|error| format!("{name}: {error}"))?;
        Ok(MidiFileImport { name, file })
    })
    .await
    .map_err(|error| error.to_string())?
}

impl App {
    /// Reads the file off the UI thread once the target is known to take
    /// MIDI.
    pub(super) fn dispatch_midi_file_drop(
        &mut self,
        path: PathBuf,
        target: BrowserImportTarget,
    ) -> Task<Message> {
        if let Err(reason) = self.midi_landing(&target) {
            self.state.status_text = reason;
            return Task::none();
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.state.status_text = format!("Reading {name}...");
        let generation = self.browser_import_request.begin();
        let task = Task::perform(read_midi_file_async(path), move |result| {
            Message::MidiFileRead {
                target: target.clone(),
                generation,
                result,
            }
        });
        self.browser_import_request.attach(task)
    }

    /// A MIDI file in the Browser has nothing to audition, so choosing it
    /// only selects it and says where it can be dragged.
    pub(super) fn select_midi_file(&mut self, source: MediaSourceRef) {
        self.state.status_text = format!(
            "Drag {} onto a MIDI track, empty Arrange or a MIDI track's Clip slot",
            source.display_name()
        );
        self.state.browser.select_source(source);
    }

    pub(super) fn on_midi_file_read(
        &mut self,
        target: BrowserImportTarget,
        generation: u64,
        result: Result<MidiFileImport, String>,
    ) {
        if !self.browser_import_request.finish(generation) {
            return;
        }
        match result {
            Err(error) => self.state.status_text = error,
            Ok(import) if import.offers_layouts() => {
                self.state.status_text = format!(
                    "Choose how the {} tracks of '{}' land",
                    import.file.tracks.len(),
                    import.name
                );
                self.state.view.midi_file_layout_choice =
                    Some(MidiFileLayoutChoice { import, target });
            }
            Ok(import) => self.land_midi_file(&import, &target, MidiFileLayout::OneClip),
        }
    }

    pub(super) fn on_choose_midi_file_layout(&mut self, layout: Option<MidiFileLayout>) {
        let Some(choice) = self.state.view.midi_file_layout_choice.take() else {
            return;
        };
        match layout {
            Some(layout) => self.land_midi_file(&choice.import, &choice.target, layout),
            None => self.state.status_text = format!("'{}' was not imported", choice.import.name),
        }
    }

    fn midi_landing(&self, target: &BrowserImportTarget) -> Result<MidiLanding, String> {
        MidiLanding::resolve(
            target,
            &MidiLandingCtx {
                project_tracks: &self.state.project_tracks,
                perform: &self.state.perform,
                samples_per_beat: self.state.transport.samples_per_beat(),
                playhead_beats: self.state.transport.position_beats(),
            },
        )
    }

    fn land_midi_file(
        &mut self,
        import: &MidiFileImport,
        target: &BrowserImportTarget,
        layout: MidiFileLayout,
    ) {
        let landing = match self.midi_landing(target) {
            Ok(landing) => landing,
            Err(reason) => {
                self.state.status_text = reason;
                return;
            }
        };
        let plan = import.plan(landing.onto(), landing.beat(), layout);
        let new_tracks = plan
            .iter()
            .filter(|planned| matches!(planned.track, MidiClipTrack::New { .. }))
            .count();
        if !self.state.project_tracks.has_room_for(new_tracks) {
            self.state.status_text = crate::domains::arrangement::PROJECT_TRACKS_FULL.into();
            return;
        }
        self.push_undo_snapshot(None);
        self.mark_project_dirty();
        let landed: Vec<_> = plan
            .into_iter()
            .map(|planned| match planned.track {
                MidiClipTrack::Existing(track_id) => (track_id, planned.clip),
                MidiClipTrack::New { name, drum_rack } => {
                    let track_id = self.add_imported_midi_track(name, drum_rack);
                    // The new track reaches the engine before its Clip, so the
                    // Clip and a playing Section's refresh find it there.
                    if let Some(track) = self.state.find_track(track_id).cloned() {
                        self.replay_track_to_engine(&track);
                    }
                    (track_id, planned.clip)
                }
            })
            .collect();
        if new_tracks > 0 {
            self.state
                .perform
                .sync_project_tracks(&self.state.project_tracks.tracks);
        }
        let tracks: Vec<_> = landed.iter().map(|(track_id, _)| *track_id).collect();
        self.place_note_clips(landing, landed);
        self.state.arrangement.selected_track = tracks.first().copied();
        let names: Vec<_> = tracks
            .iter()
            .filter_map(|track_id| self.state.find_track(*track_id))
            .map(|track| track.name.as_str())
            .collect();
        self.state.status_text = landing.status(&import.name, &names);
    }

    /// A new MIDI track named for the file, with a Drum Rack for drums.
    fn add_imported_midi_track(&mut self, name: String, drum_rack: bool) -> TrackId {
        let project_tracks = Arc::make_mut(&mut self.state.project_tracks);
        let track_id = project_tracks
            .add_numbered_track(
                "MIDI",
                TrackKind::Midi,
                &mut crate::domains::DiscardingEngine,
            )
            .expect("capacity was checked for every new track");
        let track = project_tracks
            .find_mut(track_id)
            .expect("new Project Track");
        track.name = name;
        track.has_instrument = drum_rack;
        track.instrument_kind = drum_rack.then_some(InstrumentKind::DrumRack);
        self.state.arrange_content_mut(track_id);
        track_id
    }

    /// Puts each Clip where the landing says; the first of several in a
    /// row of Clip slots is the one left open.
    fn place_note_clips(&mut self, landing: MidiLanding, clips: Vec<(TrackId, UiNoteClip)>) {
        match landing {
            MidiLanding::Slot { row, .. } => {
                let first = clips.first().map(|(_, clip)| clip.id);
                for (track_id, clip) in clips {
                    let id = clip.id;
                    let mut timeline = ArrangementTimeline::default();
                    timeline.ensure(track_id).note_clips.push(clip);
                    self.add_launcher_clip(id, track_id, row, timeline);
                }
                if let Some(first) = first {
                    self.state.perform.select_launcher_clip(first);
                }
            }
            MidiLanding::Arrange { .. } => {
                self.add_imported_note_clips_at(TimelineLocation::Arrange, clips)
            }
            MidiLanding::Section { section_id, .. } => {
                self.add_imported_note_clips_at(TimelineLocation::Section(section_id), clips)
            }
        }
    }

    fn add_imported_note_clips_at(
        &mut self,
        location: TimelineLocation,
        clips: Vec<(TrackId, UiNoteClip)>,
    ) {
        let action = self.with_timeline_editor_at(location, |editor, _, engine| {
            editor.add_imported_note_clips(engine, clips)
        });
        let _ = self.apply_arrangement_action_at(action, location);
    }
}
