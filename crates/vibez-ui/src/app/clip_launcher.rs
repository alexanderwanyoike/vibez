//! Browser imports land in independent Clip slots without touching Arrange.

use std::sync::Arc;

use iced::Task;
use vibez_core::id::{ClipId, TrackId};
use vibez_project::{PerformLayout, TimelineLocation};

use crate::domains::perform::LauncherClip;
use crate::message::{Message, PreparedBrowserImport};
use crate::state::{ArrangementTimeline, AuditionMode, UiClip};

use super::App;

impl App {
    pub(super) fn add_audio_launcher_clip(
        &mut self,
        track_id: TrackId,
        row: u32,
        payload: PreparedBrowserImport,
    ) -> Task<Message> {
        if self.state.perform.layout != PerformLayout::Clips
            || self
                .state
                .find_track(track_id)
                .is_none_or(|track| track.kind.is_midi())
            || self.state.perform.clips.at(track_id, row).is_some()
        {
            self.state.status_text = "Clip slot is no longer available".into();
            return Task::none();
        }
        let PreparedBrowserImport {
            treatment,
            audio,
            original_audio,
            name,
            source,
        } = payload;
        let id = ClipId::new();
        let duration = audio.num_frames() as u64;
        let warped = treatment.mode == AuditionMode::Warp;
        let clip = UiClip {
            id,
            name: name.clone(),
            audio,
            original_audio,
            source: Some(source),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration,
            loop_enabled: true,
            loop_start: 0,
            loop_end: duration,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: treatment.source_bpm,
            warped,
            warped_to_bpm: warped.then_some(self.state.transport.bpm),
        };
        let mut timeline = ArrangementTimeline::default();
        timeline.ensure(track_id).clips.push(clip);
        Arc::make_mut(&mut self.state.perform.clips)
            .clips
            .push(LauncherClip {
                id,
                track_id,
                row,
                timeline: Arc::new(timeline),
            });
        self.state.perform.select_launcher_clip(id);
        self.state.view.detail_panel_tab = crate::state::DetailPanelTab::Clip;
        self.state.arrangement.selected_track = Some(track_id);
        self.state.status_text = format!("Added {name} to Clip row {}", row + 1);
        self.mark_project_dirty();
        self.schedule_auto_detect_clip_transients(TimelineLocation::LauncherClip(id), track_id, id)
    }
}

impl App {
    pub(super) fn launch_clips(&mut self, request: crate::domains::perform::ClipLaunchRequest) {
        use crate::domains::perform::ClipLaunchRequest;
        use vibez_engine::{commands::EngineCommand, playback_source::PreparedClipPlayback};
        if self.state.perform.layout != PerformLayout::Clips {
            return;
        }
        let targets: Vec<_> = match request {
            ClipLaunchRequest::Clip(id) => self
                .state
                .perform
                .clips
                .by_id(id)
                .map(|clip| vec![(clip.track_id, Some(clip.clone()))])
                .unwrap_or_default(),
            ClipLaunchRequest::Stop(track_id) => vec![(track_id, None)],
            ClipLaunchRequest::Row(row) => self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|track| {
                    (
                        track.id,
                        self.state.perform.clips.at(track.id, row).cloned(),
                    )
                })
                .collect(),
            ClipLaunchRequest::StopAll => self
                .state
                .project_tracks
                .tracks
                .iter()
                .map(|track| (track.id, None))
                .collect(),
        };
        if self
            .state
            .perform
            .clip_record
            .session
            .as_ref()
            .is_some_and(|session| {
                targets
                    .iter()
                    .any(|(track, _)| *track == session.working.track_id)
            })
        {
            if self.state.perform.clip_record.pending_audio_arm.is_some() {
                self.cancel_clip_record();
                self.launch_clips(request);
                return;
            }
            self.state.perform.clip_record.notes.request_stop();
            self.send_command(EngineCommand::StopClipRecord { immediate: false });
        }
        if !self.state.perform.clip_editor.running && targets.iter().all(|(_, clip)| clip.is_none())
        {
            return;
        }
        if targets
            .iter()
            .filter_map(|(_, clip)| clip.as_ref())
            .any(|clip| {
                clip.timeline.get(clip.track_id).is_some_and(|content| {
                    content
                        .clips
                        .iter()
                        .any(|audio| audio.audio.num_frames() == 0)
                })
            })
        {
            self.state.status_text = "Clip media is not ready".into();
            return;
        }
        let samples_per_beat = self.state.transport.samples_per_beat();
        let mut prepared = Vec::new();
        for (track_id, clip) in targets {
            self.state.perform.clip_editor.next_request += 1;
            let request_id = self.state.perform.clip_editor.next_request;
            // Reflect intent before the callback without letting an older engine
            // acknowledgement overwrite a newer tap.
            self.state.perform.clip_editor.queue_request(
                track_id,
                clip.as_ref().map(|clip| clip.id),
                request_id,
            );
            if let Some(clip) = clip {
                prepared.push(clip.prepare(request_id, samples_per_beat));
                self.state
                    .perform
                    .clip_editor
                    .pending
                    .insert(request_id, clip);
            } else {
                prepared.push(PreparedClipPlayback::stop(track_id, request_id));
            }
        }
        if !prepared.is_empty() {
            self.state.perform.clip_editor.running = true;
            self.send_command(EngineCommand::QueueClips {
                clips: prepared,
                quantization: vibez_core::perform::MusicalBoundary::OneBar,
            });
        }
    }
}

pub(super) fn refresh_edited_clips(
    before: &Arc<crate::domains::perform::ClipStore>,
    perform: &mut crate::domains::perform::PerformState,
    recording: Option<ClipId>,
    spb: f64,
    engine: &mut impl crate::domains::EngineHandle,
) {
    use vibez_engine::commands::EngineCommand;
    if perform.layout != PerformLayout::Clips
        || !perform.clip_editor.running
        || Arc::ptr_eq(before, &perform.clips)
    {
        return;
    }
    for removed in before
        .clips
        .iter()
        .filter(|clip| perform.clips.by_id(clip.id).is_none())
    {
        let editor = &mut perform.clip_editor;
        if editor.queued.get(&removed.track_id) == Some(&None) {
            continue;
        }
        if editor
            .playing
            .get(&removed.track_id)
            .is_some_and(|active| active.id == removed.id)
            || editor.queued.get(&removed.track_id) == Some(&Some(removed.id))
        {
            editor.next_request += 1;
            editor.queue_request(removed.track_id, None, editor.next_request);
            engine.send(EngineCommand::QueueClips {
                clips: vec![vibez_engine::playback_source::PreparedClipPlayback::stop(
                    removed.track_id,
                    editor.next_request,
                )],
                quantization: vibez_core::perform::MusicalBoundary::OneBar,
            });
        }
    }
    for clip in &perform.clips.clips {
        if recording == Some(clip.id)
            || perform
                .clip_record
                .session
                .as_ref()
                .is_some_and(|s| s.working.id == clip.id)
            || before
                .by_id(clip.id)
                .is_none_or(|old| Arc::ptr_eq(&old.timeline, &clip.timeline))
        {
            continue;
        }
        let editor = &mut perform.clip_editor;
        editor.next_request += 1;
        let active = clip.prepare(editor.next_request, spb);
        editor.pending.insert(editor.next_request, clip.clone());
        editor.next_request += 1;
        let queued = clip.prepare(editor.next_request, spb);
        editor.pending.insert(editor.next_request, clip.clone());
        engine.send(EngineCommand::EditClip { active, queued });
    }
}

#[cfg(test)]
mod live_edit_tests {
    use super::*;
    use crate::domains::perform::{ClipMsg, PerformCtx, PerformMsg};
    use crate::domains::piano_roll::{PianoRollCtx, PianoRollMsg};
    use crate::domains::test_support::RecordingEngine;
    use crate::state::AppState;
    use vibez_engine::commands::EngineCommand;

    #[test]
    fn deleting_the_part_in_the_shared_editor_stops_its_resident_clip() {
        let track = TrackId::new();
        let clip = crate::domains::perform::clip_record::empty_midi_clip(
            ClipId::new(),
            track,
            0,
            "Take".into(),
            4.0,
        );
        let mut state = crate::domains::perform::PerformState::default();
        state.layout = PerformLayout::Clips;
        Arc::make_mut(&mut state.clips).clips.push(clip.clone());
        state.clip_editor.running = true;
        state.clip_editor.playing.insert(track, clip.clone());
        state.select_launcher_clip(clip.id);
        let before = Arc::clone(&state.clips);
        Arc::make_mut(&mut state.clip_editor.editor.timeline)
            .ensure(track)
            .note_clips
            .clear();
        state.commit_selected_timeline();
        let mut engine = RecordingEngine::default();
        refresh_edited_clips(&before, &mut state, None, 100.0, &mut engine);
        assert!(
            matches!(&engine.0[..], [EngineCommand::QueueClips { clips, .. }] if clips.len() == 1 && clips[0].track_id == track && clips[0].clip_id.is_none())
        );
    }

    #[test]
    fn piano_roll_edit_publishes_new_source_without_relaunching() {
        let mut state = AppState::default();
        state.perform.layout = PerformLayout::Clips;
        let track = TrackId::new();
        let mut project_track = crate::state::ProjectTrack::new(track, "Test".into(), 0);
        project_track.kind = vibez_core::midi::TrackKind::Midi;
        let tracks = vec![project_track];
        let mut engine = RecordingEngine::default();
        state.perform.update(
            PerformMsg::Clips(ClipMsg::CreateMidi {
                track_id: track,
                row: 0,
            }),
            &mut engine,
            PerformCtx {
                workspace_visible: true,
                project_tracks: &tracks,
                ..Default::default()
            },
        );
        let id = state.perform.clip_editor.selected.unwrap();
        state.perform.clip_editor.running = true;
        let before = Arc::clone(&state.perform.clips);
        state.piano_roll.update(
            PianoRollMsg::AddNote {
                track_id: track,
                clip_id: id,
                pitch: 42,
                start_beat: 0.5,
                duration_beats: 0.25,
            },
            &mut crate::domains::DiscardingEngine,
            state.perform.timeline_editor_mut(),
            PianoRollCtx::default(),
        );
        state.perform.commit_selected_timeline();
        refresh_edited_clips(&before, &mut state.perform, None, 4.0, &mut engine);
        assert_eq!(
            engine.0.len(),
            1,
            "the audible source must follow editor changes"
        );
        let vibez_engine::commands::EngineCommand::EditClip { active, queued } = &engine.0[0]
        else {
            panic!("expected a source edit")
        };
        assert_eq!(active.source.note_clips[0].notes[0].pitch, 42);
        assert_eq!(queued.source.note_clips[0].notes[0].pitch, 42);
        assert_ne!(active.request_id, queued.request_id);
        let acknowledged = state.perform.clip_editor.pending[&active.request_id].clone();
        assert!(before
            .by_id(id)
            .unwrap()
            .timeline
            .get(track)
            .unwrap()
            .note_clips[0]
            .notes
            .is_empty());

        let edited = Arc::clone(&state.perform.clips);
        engine.0.clear();
        refresh_edited_clips(&edited, &mut state.perform, None, 4.0, &mut engine);
        assert!(
            engine.0.is_empty(),
            "selection and timer messages must not refresh content"
        );

        // Undo restores canonical content while the runtime selection can be elsewhere.
        state.perform.clip_editor.selected = None;
        state.perform.clips = before;
        refresh_edited_clips(&edited, &mut state.perform, None, 4.0, &mut engine);
        let vibez_engine::commands::EngineCommand::EditClip { active, .. } = &engine.0[0] else {
            panic!("expected undo source")
        };
        assert!(active.source.note_clips[0].notes.is_empty());
        assert_eq!(
            acknowledged.timeline.get(track).unwrap().note_clips[0]
                .notes
                .len(),
            1,
            "Capture retains the acknowledged version"
        );
        engine.0.clear();
        let undone = Arc::clone(&state.perform.clips);
        state.perform.clips = edited;
        refresh_edited_clips(&undone, &mut state.perform, Some(id), 4.0, &mut engine);
        assert!(
            engine.0.is_empty(),
            "recording owns its live preview updates"
        );
    }
}
