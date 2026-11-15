//! Lands dropped MIDI files as note Clips on the target the drop resolved.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;
use vibez_core::id::{SectionId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::midi_file::MidiFile;
use vibez_core::track::MediaSourceRef;
use vibez_project::PerformLayout;

use super::App;
use crate::domains::midi_file_import::{
    MidiClipTrack, MidiFileImport, MidiFileLayout, MidiFileLayoutChoice, MIDI_FILE_TARGETS,
};
use crate::message::{BrowserImportTarget, Message};
use crate::state::UiNoteClip;

/// Where the Clips of one MIDI file drop go.
#[derive(Debug, Clone, Copy)]
enum MidiLanding {
    Arrange {
        onto: Option<TrackId>,
        beat: f64,
    },
    Section {
        section_id: SectionId,
        onto: Option<TrackId>,
        beat: f64,
    },
    Slot {
        onto: TrackId,
        row: u32,
    },
}

impl MidiLanding {
    fn onto(self) -> Option<TrackId> {
        match self {
            Self::Arrange { onto, .. } | Self::Section { onto, .. } => onto,
            Self::Slot { onto, .. } => Some(onto),
        }
    }

    fn beat(self) -> f64 {
        match self {
            Self::Arrange { beat, .. } | Self::Section { beat, .. } => beat,
            Self::Slot { .. } => 0.0,
        }
    }
}

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

    /// Whether `target` takes MIDI, re-checked when the file has been read
    /// since the track or slot may have gone meanwhile.
    fn midi_landing(&self, target: &BrowserImportTarget) -> Result<MidiLanding, String> {
        let midi_track = |track_id: TrackId| {
            self.state
                .find_track(track_id)
                .is_some_and(|track| track.kind.is_midi())
        };
        let refused = || Err(MIDI_FILE_TARGETS.to_string());
        let beat = |position_samples: u64| {
            position_samples as f64 / self.state.transport.samples_per_beat().max(f64::EPSILON)
        };
        match *target {
            BrowserImportTarget::ArrangementClipAt {
                track_id,
                position_samples,
            } if midi_track(track_id) => Ok(MidiLanding::Arrange {
                onto: Some(track_id),
                beat: beat(position_samples),
            }),
            BrowserImportTarget::ArrangementClip(preferred) => Ok(MidiLanding::Arrange {
                onto: preferred.filter(|track_id| midi_track(*track_id)),
                beat: self.state.transport.position_beats(),
            }),
            BrowserImportTarget::ArrangementNewTrackAt { position_samples } => {
                Ok(MidiLanding::Arrange {
                    onto: None,
                    beat: beat(position_samples),
                })
            }
            BrowserImportTarget::SectionClipAt {
                section_id,
                track_id,
                position_samples,
            } if midi_track(track_id)
                && self.state.perform.sections.by_id(section_id).is_some() =>
            {
                Ok(MidiLanding::Section {
                    section_id,
                    onto: Some(track_id),
                    beat: beat(position_samples),
                })
            }
            BrowserImportTarget::LauncherClipAt { track_id, row } if midi_track(track_id) => {
                if self.state.perform.layout != PerformLayout::Clips
                    || self.state.perform.clips.at(track_id, row).is_some()
                {
                    return Err("Clip slot is no longer available".into());
                }
                Ok(MidiLanding::Slot {
                    onto: track_id,
                    row,
                })
            }
            _ => refused(),
        }
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
        if self.state.project_tracks.tracks.len() + new_tracks
            > vibez_engine::mixer::CHANNEL_CAPACITY
        {
            self.state.status_text = crate::domains::arrangement::PROJECT_TRACKS_FULL.into();
            return;
        }
        self.push_undo_snapshot(None);
        self.mark_project_dirty();
        let mut landed_on = Vec::new();
        for planned in plan {
            let track_id = match planned.track {
                MidiClipTrack::Existing(track_id) => {
                    self.place_note_clip(landing, track_id, planned.clip, false);
                    track_id
                }
                MidiClipTrack::New { name, drum_rack } => {
                    let track_id = self.add_imported_midi_track(name, drum_rack);
                    self.place_note_clip(landing, track_id, planned.clip, true);
                    let track = self.state.find_track(track_id).cloned();
                    if let Some(track) = track {
                        self.replay_track_to_engine(&track);
                    }
                    track_id
                }
            };
            landed_on.push(track_id);
        }
        if new_tracks > 0 {
            self.state
                .perform
                .sync_project_tracks(&self.state.project_tracks.tracks);
        }
        if let MidiLanding::Section { section_id, .. } = landing {
            self.refresh_playing_section_after_edit(section_id);
        }
        self.state.arrangement.selected_track = landed_on.first().copied();
        self.state.status_text = self.midi_landing_status(&import.name, landing, &landed_on);
    }

    /// A new MIDI track named for the file; the engine learns of it when the
    /// whole track replays.
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

    /// Puts `clip` where the landing says; a new track's Arrange Clips reach
    /// the engine with its replay instead.
    fn place_note_clip(
        &mut self,
        landing: MidiLanding,
        track_id: TrackId,
        clip: UiNoteClip,
        new_track: bool,
    ) {
        match landing {
            MidiLanding::Arrange { .. } => {
                let content = self.state.arrange_content_mut(track_id);
                let resident = content.note_clips.len();
                let command = clip.add_command_beside(track_id, resident);
                content.note_clips.push(clip);
                if !new_track {
                    self.send_command(command);
                }
            }
            MidiLanding::Section { section_id, .. } => {
                if self.state.perform.selected_section == Some(section_id) {
                    let editor = self.state.perform.section_editor.editor_mut();
                    Arc::make_mut(&mut editor.timeline)
                        .ensure(track_id)
                        .note_clips
                        .push(clip);
                    self.state.perform.commit_selected_timeline();
                } else if let Some(section) =
                    Arc::make_mut(&mut self.state.perform.sections).by_id_mut(section_id)
                {
                    Arc::make_mut(&mut section.timeline)
                        .ensure(track_id)
                        .note_clips
                        .push(clip);
                }
            }
            MidiLanding::Slot { row, .. } => {
                let id = clip.id;
                let mut timeline = crate::state::ArrangementTimeline::default();
                timeline.ensure(track_id).note_clips.push(clip);
                Arc::make_mut(&mut self.state.perform.clips).clips.push(
                    crate::domains::perform::LauncherClip {
                        id,
                        track_id,
                        row,
                        timeline: Arc::new(timeline),
                        launch: Default::default(),
                    },
                );
                self.state.perform.select_launcher_clip(id);
                self.state.view.detail_panel_tab = crate::state::DetailPanelTab::Clip;
            }
        }
    }

    fn midi_landing_status(&self, name: &str, landing: MidiLanding, tracks: &[TrackId]) -> String {
        if tracks.len() > 1 {
            return format!("Imported '{name}' as {} new MIDI tracks", tracks.len());
        }
        let track_name = tracks
            .first()
            .and_then(|track_id| self.state.find_track(*track_id))
            .map(|track| track.name.clone())
            .unwrap_or_default();
        match landing {
            MidiLanding::Slot { row, .. } => format!("Added {name} to Clip row {}", row + 1),
            _ => format!(
                "Imported '{name}' to {track_name} at beat {:.2}",
                landing.beat()
            ),
        }
    }
}
