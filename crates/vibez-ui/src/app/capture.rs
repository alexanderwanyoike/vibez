//! Application boundary for Capture timing, project transaction, and engine sync.

use iced::Task;
use std::sync::Arc;

use vibez_engine::commands::EngineCommand;

use crate::domains::perform::capture::CompletedCapture;
use crate::domains::perform::{CaptureAction, MaterializedCapture};
use crate::message::Message;
use crate::state::Workspace;

use super::*;

impl App {
    pub(super) fn apply_capture_action(&mut self, action: CaptureAction) -> Task<Message> {
        match action {
            CaptureAction::Start => {
                if !self.begin_project_transaction() {
                    self.state.perform.capture.cancel();
                    self.state.status_text =
                        "Finish the current project edit before Capture".into();
                    return Task::none();
                }
                self.state.perform.capture.prepare(
                    self.state.transport.position_samples,
                    self.state.transport.sample_rate,
                    self.state.transport.bpm,
                );
                self.send_command(EngineCommand::StartPerformanceCapture);
                self.state.status_text = "Starting Capture into Arrange…".into();
            }
            CaptureAction::Stop => {
                self.send_command(EngineCommand::StopPerformanceCapture);
                self.state.status_text = "Stopping Capture…".into();
            }
        }
        Task::none()
    }

    pub(super) fn finish_performance_capture(&mut self, completed: Option<CompletedCapture>) {
        let Some(completed) = completed else {
            self.discard_capture_transaction();
            self.state.status_text = "Capture stopped · no Section content recorded".into();
            return;
        };
        let materialized = completed.materialize();
        if materialized.is_empty() {
            self.discard_capture_transaction();
            self.state.status_text = "Capture stopped · no Section content recorded".into();
            return;
        }
        let perform_track_ids: Vec<_> = self
            .state
            .project_tracks
            .tracks
            .iter()
            .map(|track| track.id)
            .collect();
        if !materialized
            .target_interval_is_empty(&self.state.arrangement.timeline, &perform_track_ids)
        {
            self.discard_capture_transaction();
            self.state.status_text =
                "Capture needs an empty Arrange interval · replacement arrives next".into();
            return;
        }

        let clip_count = apply_capture_to_arrangement(
            &mut self.state.arrangement.timeline,
            materialized,
            &mut self.cmd_tx,
        );
        self.push_undo_snapshot(None);
        self.mark_project_dirty();
        self.commit_project_transaction();
        self.state.view.workspace = Workspace::Arrange;
        self.state.status_text = format!("Capture committed · {clip_count} clips · one undo step");
    }

    fn discard_capture_transaction(&mut self) {
        if let Some((_, dirty_before)) = self.state.project.history.abandon_transaction() {
            self.state.project.dirty = dirty_before;
        }
    }
}

fn apply_capture_to_arrangement(
    arrange: &mut Arc<crate::state::ArrangementTimeline>,
    captured: MaterializedCapture,
    engine: &mut Option<rtrb::Producer<EngineCommand>>,
) -> usize {
    let mut commands = Vec::new();
    let mut clip_count = 0;
    let timeline = Arc::make_mut(arrange);
    for (track_id, mut incoming) in captured.by_track {
        clip_count += incoming.clips.len() + incoming.note_clips.len();
        for clip in &incoming.clips {
            commands.push(EngineCommand::AddClip {
                track_id,
                clip_id: clip.id,
                audio: Arc::clone(&clip.audio),
                position: clip.position,
                source_offset: clip.source_offset,
                duration: clip.duration,
                loop_enabled: clip.loop_enabled,
                loop_start: clip.loop_start,
                loop_end: clip.loop_end,
            });
        }
        for clip in &incoming.note_clips {
            commands.push(EngineCommand::AddNoteClip {
                track_id,
                clip_id: clip.id,
                position_beats: clip.position_beats,
                duration_beats: clip.duration_beats,
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
                groove_grid: clip.groove_grid,
            });
            for note in &clip.notes {
                commands.push(EngineCommand::AddNote {
                    track_id,
                    clip_id: clip.id,
                    note: *note,
                });
            }
        }
        let destination = timeline.ensure(track_id);
        destination.clips.append(&mut incoming.clips);
        destination.note_clips.append(&mut incoming.note_clips);
    }
    if let Some(engine) = engine {
        for command in commands {
            let _ = engine.push(command);
        }
    }
    clip_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, ArrangementTimeline, ProjectSnapshot, UiNoteClip};
    use std::collections::HashMap;
    use vibez_core::id::{ClipId, TrackId};
    use vibez_core::perform::GrooveGrid;

    fn snapshot(state: &AppState) -> ProjectSnapshot {
        ProjectSnapshot {
            project_tracks: Arc::clone(&state.project_tracks),
            arrange_timeline: Arc::clone(&state.arrangement.timeline),
            sections: Arc::clone(&state.perform.sections),
            bpm: state.transport.bpm,
            project_swing: state.perform.project_swing(),
            loop_enabled: state.transport.loop_enabled,
            loop_start_beats: state.transport.loop_start_beats,
            loop_end_beats: state.transport.loop_end_beats,
        }
    }

    #[test]
    fn applying_capture_changes_only_named_tracks_and_preserves_outside_content() {
        let captured_track = vibez_core::id::TrackId::new();
        let untouched_track = vibez_core::id::TrackId::new();
        let mut arrange = Arc::new(ArrangementTimeline::default());
        Arc::make_mut(&mut arrange)
            .ensure(untouched_track)
            .automation
            .push(vibez_core::automation::AutomationLane::new(
                vibez_core::automation::AutomationTarget::TrackGain,
            ));
        let untouched_before = arrange.get(untouched_track).unwrap().automation.clone();
        let mut by_track = HashMap::new();
        by_track.insert(captured_track, Default::default());
        let captured = MaterializedCapture {
            arrange_start_samples: 100,
            arrange_end_samples: 200,
            by_track,
            ..MaterializedCapture::default()
        };

        assert_eq!(
            apply_capture_to_arrangement(&mut arrange, captured, &mut None),
            0
        );
        assert_eq!(
            arrange.get(untouched_track).unwrap().automation,
            untouched_before
        );
        assert!(arrange.get(captured_track).is_some());
    }

    #[test]
    fn one_capture_transaction_round_trips_through_undo_and_redo() {
        let mut state = AppState::default();
        let track_id = TrackId::new();
        let before = Arc::clone(&state.arrangement.timeline);
        assert!(state
            .project
            .history
            .begin_transaction(snapshot(&state), state.project.dirty));

        let mut incoming = crate::state::TrackTimelineContent::default();
        incoming.note_clips.push(UiNoteClip {
            id: ClipId::new(),
            name: "Capture · Groove A · Hats".into(),
            position_beats: 4.0,
            duration_beats: 4.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        });
        let captured = MaterializedCapture {
            arrange_start_samples: 16,
            arrange_end_samples: 32,
            by_track: HashMap::from([(track_id, incoming)]),
            samples_per_beat: 4.0,
        };
        assert_eq!(
            apply_capture_to_arrangement(&mut state.arrangement.timeline, captured, &mut None,),
            1
        );
        let changed = snapshot(&state);
        state.project.history.push_edit(changed, None);
        assert!(state.project.history.commit_transaction());
        let captured_timeline = Arc::clone(&state.arrangement.timeline);
        assert_eq!(state.project.history.undo.len(), 1);

        let current = snapshot(&state);
        let undo = state.project.history.pop_undo().unwrap();
        state.project.history.push_redo(current);
        assert!(Arc::ptr_eq(&undo.arrange_timeline, &before));
        state.arrangement.timeline = undo.arrange_timeline;
        assert!(state.arrangement.timeline.by_track.is_empty());

        let redo = state.project.history.pop_redo().unwrap();
        assert!(Arc::ptr_eq(&redo.arrange_timeline, &captured_timeline));
        assert_eq!(
            redo.arrange_timeline
                .get(track_id)
                .unwrap()
                .note_clips
                .len(),
            1
        );
    }
}
