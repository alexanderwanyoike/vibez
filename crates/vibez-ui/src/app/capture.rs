//! Application boundary for Capture timing, project transaction, and engine sync.

use iced::Task;
use std::sync::Arc;

use vibez_core::automation::AutomationTarget;
use vibez_core::id::TrackId;
use vibez_core::perform::SwingOffset;
use vibez_engine::commands::EngineCommand;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::devices::DevicesMsg;
use crate::domains::perform::capture::CompletedCapture;
use crate::domains::perform::{CaptureAction, MaterializedCapture, PerformMsg};
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{UndoGestureId, Workspace};

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
                self.state.perform.capture.prepare_controlled_tracks(
                    self.state
                        .project_tracks
                        .tracks
                        .iter()
                        .map(|track| (track.id, track.mute)),
                );
                self.send_command(EngineCommand::StartPerformanceCapture);
                self.state.status_text = "Starting Capture into Arrange…".into();
            }
            CaptureAction::Stop => {
                self.end_capture_automation_gesture();
                // Transport Stop publishes the Capture boundary first, then
                // stops Section playback in the same audio callback.
                self.send_command(capture_stop_command());
                self.state.status_text = "Stopping Capture and Section playback…".into();
            }
        }
        Task::none()
    }

    pub(super) fn prepare_capture_message(
        &mut self,
        undo_gesture: Option<UndoGestureId>,
        message: &Message,
    ) -> bool {
        if self.state.perform.capture.is_active()
            && matches!(message, Message::Perform(PerformMsg::SetProjectSwing(_)))
        {
            self.state.status_text = "Project Swing is locked while Capture records".to_string();
            return true;
        }
        if matches!(message, Message::View(ViewMsg::MouseReleased)) {
            self.end_capture_automation_gesture();
        }
        let Some(gesture) = undo_gesture else {
            return false;
        };
        let values = self.capture_automation_values(message);
        if values.is_empty() {
            return false;
        }
        let ended = self
            .state
            .perform
            .capture
            .begin_ui_automation_gesture(gesture);
        for (track_id, target) in ended {
            self.send_command(EngineCommand::EndAutomationGesture { track_id, target });
        }
        for value in values {
            if !self
                .state
                .perform
                .capture
                .is_controlled_track(value.track_id)
            {
                continue;
            }
            let begin = self
                .state
                .perform
                .capture
                .register_ui_automation_target(value.track_id, value.target);
            self.send_command(EngineCommand::UpdateAutomationGesture {
                track_id: value.track_id,
                target: value.target,
                normalized_value: value.normalized,
                begin,
            });
        }
        false
    }

    pub(super) fn end_capture_automation_gesture(&mut self) {
        let targets = self.state.perform.capture.end_ui_automation_gesture();
        for (track_id, target) in targets {
            self.send_command(EngineCommand::EndAutomationGesture { track_id, target });
        }
    }

    fn capture_automation_values(&self, message: &Message) -> Vec<CaptureAutomationValue> {
        match message {
            Message::Arrangement(ArrangementMsg::SetTrackGain(track_id, gain)) => {
                vec![CaptureAutomationValue {
                    track_id: *track_id,
                    target: AutomationTarget::TrackGain,
                    normalized: (*gain / 2.0).clamp(0.0, 1.0),
                }]
            }
            Message::Arrangement(ArrangementMsg::SetTrackPan(track_id, pan)) => {
                vec![CaptureAutomationValue {
                    track_id: *track_id,
                    target: AutomationTarget::TrackPan,
                    normalized: (*pan).clamp(0.0, 1.0),
                }]
            }
            Message::Devices(DevicesMsg::SetEffectParam(
                track_id,
                effect_id,
                param_index,
                value,
            )) => self
                .normalize_effect_value(*track_id, *effect_id, *param_index, *value)
                .into_iter()
                .collect(),
            Message::Devices(DevicesMsg::SetEffectParams(track_id, effect_id, updates)) => updates
                .iter()
                .filter_map(|(param_index, value)| {
                    self.normalize_effect_value(*track_id, *effect_id, *param_index, *value)
                })
                .collect(),
            Message::Perform(PerformMsg::SetTrackSwingOffset { track_id, value }) => {
                vec![CaptureAutomationValue {
                    track_id: *track_id,
                    target: AutomationTarget::TrackSwingOffset,
                    normalized: value.map(SwingOffset::new).unwrap_or_default().normalized(),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn normalize_effect_value(
        &self,
        track_id: TrackId,
        effect_id: vibez_core::id::EffectId,
        param_index: usize,
        value: f32,
    ) -> Option<CaptureAutomationValue> {
        let effect = self
            .state
            .project_tracks
            .tracks
            .iter()
            .find(|track| track.id == track_id)?
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)?;
        let descriptor = effect.descriptors.get(param_index)?;
        let span = descriptor.max - descriptor.min;
        Some(CaptureAutomationValue {
            track_id,
            target: AutomationTarget::EffectParam {
                effect_id,
                param_index,
            },
            normalized: if span.abs() > f32::EPSILON {
                ((value - descriptor.min) / span).clamp(0.0, 1.0)
            } else {
                0.0
            },
        })
    }

    pub(super) fn finish_performance_capture(&mut self, completed: Option<CompletedCapture>) {
        let Some(completed) = completed else {
            self.discard_capture_transaction();
            self.state.status_text = "Capture stopped · no Section content recorded".into();
            return;
        };
        let materialized = completed.materialize();
        if materialized.is_empty()
            && !capture_replaces_existing_content(&self.state.arrangement.timeline, &materialized)
        {
            self.discard_capture_transaction();
            self.state.status_text = "Capture stopped · no Section content recorded".into();
            return;
        }
        for (track_id, muted) in &materialized.pre_capture_mutes {
            let captured_mute = materialized.by_track.get(track_id).is_some_and(|content| {
                content
                    .automation
                    .iter()
                    .any(|lane| lane.target == vibez_core::automation::AutomationTarget::TrackMute)
            });
            if captured_mute {
                if let Some(track) =
                    Arc::make_mut(&mut self.state.project_tracks).find_mut(*track_id)
                {
                    track.mute = *muted;
                }
                self.state.automation_ui.set_override(
                    *track_id,
                    vibez_core::automation::AutomationTarget::TrackMute,
                    false,
                );
            }
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

#[derive(Debug, Clone, Copy)]
struct CaptureAutomationValue {
    track_id: TrackId,
    target: AutomationTarget,
    normalized: f32,
}

fn capture_stop_command() -> EngineCommand {
    EngineCommand::Stop
}

fn capture_replaces_existing_content(
    arrange: &crate::state::ArrangementTimeline,
    captured: &MaterializedCapture,
) -> bool {
    captured.controlled_track_ids.iter().any(|track_id| {
        arrange.get(*track_id).is_some_and(|existing| {
            existing.clips.iter().any(|clip| {
                clip.position < captured.arrange_end_samples
                    && clip.position.saturating_add(clip.duration) > captured.arrange_start_samples
            }) || existing.note_clips.iter().any(|clip| {
                let start = (clip.position_beats * captured.samples_per_beat)
                    .round()
                    .max(0.0) as u64;
                let end = ((clip.position_beats + clip.duration_beats) * captured.samples_per_beat)
                    .round()
                    .max(0.0) as u64;
                start < captured.arrange_end_samples && end > captured.arrange_start_samples
            }) || !existing.automation.is_empty()
        })
    })
}

fn apply_capture_to_arrangement(
    arrange: &mut Arc<crate::state::ArrangementTimeline>,
    captured: MaterializedCapture,
    engine: &mut Option<rtrb::Producer<EngineCommand>>,
) -> usize {
    let MaterializedCapture {
        arrange_start_samples,
        arrange_end_samples,
        mut by_track,
        mut controlled_track_ids,
        pre_capture_mutes,
        samples_per_beat,
    } = captured;
    let mut commands = Vec::new();
    let mut clip_count = 0;
    let timeline = Arc::make_mut(arrange);
    for track_id in by_track.keys().copied() {
        if !controlled_track_ids.contains(&track_id) {
            controlled_track_ids.push(track_id);
        }
    }
    let start_beat = arrange_start_samples as f64 / samples_per_beat;
    let end_beat = arrange_end_samples as f64 / samples_per_beat;

    for track_id in controlled_track_ids {
        let mut incoming = by_track.remove(&track_id).unwrap_or_default();
        let captured_mute = incoming
            .automation
            .iter()
            .any(|lane| lane.target == vibez_core::automation::AutomationTarget::TrackMute);
        clip_count += incoming.clips.len() + incoming.note_clips.len();

        let destination = timeline.ensure(track_id);
        for clip in &destination.clips {
            commands.push(EngineCommand::RemoveClip(track_id, clip.id));
        }
        for clip in &destination.note_clips {
            commands.push(EngineCommand::RemoveNoteClip(track_id, clip.id));
        }

        destination.clips = destination
            .clips
            .iter()
            .flat_map(|clip| {
                preserve_audio_outside_interval(clip, arrange_start_samples, arrange_end_samples)
            })
            .collect();
        destination.note_clips = destination
            .note_clips
            .iter()
            .flat_map(|clip| preserve_notes_outside_interval(clip, start_beat, end_beat))
            .collect();
        replace_automation_interval(
            &mut destination.automation,
            incoming.automation,
            start_beat,
            end_beat,
        );
        destination.clips.append(&mut incoming.clips);
        destination.note_clips.append(&mut incoming.note_clips);
        destination.clips.sort_by_key(|clip| clip.position);
        destination
            .note_clips
            .sort_by(|left, right| left.position_beats.total_cmp(&right.position_beats));

        for clip in &destination.clips {
            commands.push(add_audio_clip_command(track_id, clip));
        }
        for clip in &destination.note_clips {
            commands.extend(add_note_clip_commands(track_id, clip));
        }
        for lane in &destination.automation {
            commands.push(EngineCommand::SetAutomationLane {
                track_id,
                lane: lane.clone(),
            });
        }
        if let Some(muted) = pre_capture_mutes.get(&track_id) {
            if captured_mute {
                commands.push(EngineCommand::SetTrackMute(track_id, *muted));
                commands.push(EngineCommand::SetAutomationOverride {
                    track_id,
                    target: vibez_core::automation::AutomationTarget::TrackMute,
                    overridden: false,
                });
            }
        }
    }
    if let Some(engine) = engine {
        for command in commands {
            let _ = engine.push(command);
        }
    }
    clip_count
}

fn preserve_audio_outside_interval(
    clip: &crate::state::UiClip,
    start: u64,
    end: u64,
) -> Vec<crate::state::UiClip> {
    let clip_end = clip.position.saturating_add(clip.duration);
    if clip_end <= start || clip.position >= end {
        return vec![clip.clone()];
    }
    let mut fragments = Vec::with_capacity(2);
    if clip.position < start {
        let mut before = clip.clone();
        before.duration = start - clip.position;
        fragments.push(before);
    }
    if clip_end > end {
        let mut after = clip.clone();
        if !fragments.is_empty() {
            after.id = vibez_core::id::ClipId::new();
        }
        let delta = end.saturating_sub(clip.position);
        after.position = end;
        after.source_offset = crate::domains::perform::capture::captured_audio_offset(clip, delta);
        after.duration = clip_end - end;
        fragments.push(after);
    }
    fragments
}

fn preserve_notes_outside_interval(
    clip: &crate::state::UiNoteClip,
    start: f64,
    end: f64,
) -> Vec<crate::state::UiNoteClip> {
    let clip_end = clip.position_beats + clip.duration_beats;
    if clip_end <= start || clip.position_beats >= end {
        return vec![clip.clone()];
    }
    let mut fragments = Vec::with_capacity(2);
    if clip.position_beats < start {
        fragments.push(note_clip_window(clip, clip.position_beats, start, clip.id));
    }
    if clip_end > end {
        let id = if fragments.is_empty() {
            clip.id
        } else {
            vibez_core::id::ClipId::new()
        };
        fragments.push(note_clip_window(clip, end, clip_end, id));
    }
    fragments
}

fn note_clip_window(
    clip: &crate::state::UiNoteClip,
    window_start: f64,
    window_end: f64,
    id: vibez_core::id::ClipId,
) -> crate::state::UiNoteClip {
    let local_start = window_start - clip.position_beats;
    let local_end = window_end - clip.position_beats;
    crate::state::UiNoteClip {
        id,
        name: clip.name.clone(),
        position_beats: window_start,
        duration_beats: window_end - window_start,
        notes: crate::domains::perform::capture::captured_visible_notes(
            clip,
            local_start,
            local_end,
        ),
        selected_notes: Default::default(),
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: clip.groove_grid,
    }
}

fn replace_automation_interval(
    existing: &mut Vec<vibez_core::automation::AutomationLane>,
    incoming: Vec<vibez_core::automation::AutomationLane>,
    start_beat: f64,
    end_beat: f64,
) {
    for lane in existing.iter_mut() {
        let start_value = lane.value_at(start_beat);
        let end_value = lane.value_at(end_beat);
        lane.points
            .retain(|point| point.beat < start_beat || point.beat > end_beat);
        if let Some(value) = start_value {
            lane.insert_point(vibez_core::automation::AutomationPoint {
                beat: start_beat,
                value,
                curve: 0.0,
            });
        }
        if let Some(value) = end_value {
            lane.insert_point(vibez_core::automation::AutomationPoint {
                beat: end_beat,
                value,
                curve: 0.0,
            });
        }
    }
    for incoming_lane in incoming {
        match existing
            .iter_mut()
            .find(|lane| lane.target == incoming_lane.target)
        {
            Some(lane) => {
                for point in incoming_lane.points {
                    lane.insert_point(point);
                }
            }
            None => existing.push(incoming_lane),
        }
    }
}

fn add_audio_clip_command(
    track_id: vibez_core::id::TrackId,
    clip: &crate::state::UiClip,
) -> EngineCommand {
    EngineCommand::AddClip {
        track_id,
        clip_id: clip.id,
        audio: Arc::clone(&clip.audio),
        position: clip.position,
        source_offset: clip.source_offset,
        duration: clip.duration,
        loop_enabled: clip.loop_enabled,
        loop_start: clip.loop_start,
        loop_end: clip.loop_end,
    }
}

fn add_note_clip_commands(
    track_id: vibez_core::id::TrackId,
    clip: &crate::state::UiNoteClip,
) -> Vec<EngineCommand> {
    let mut commands = vec![EngineCommand::AddNoteClip {
        track_id,
        clip_id: clip.id,
        position_beats: clip.position_beats,
        duration_beats: clip.duration_beats,
        loop_enabled: clip.loop_enabled,
        loop_start_beats: clip.loop_start_beats,
        loop_end_beats: clip.loop_end_beats,
        groove_grid: clip.groove_grid,
    }];
    commands.extend(clip.notes.iter().map(|note| EngineCommand::AddNote {
        track_id,
        clip_id: clip.id,
        note: *note,
    }));
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, ArrangementTimeline, ProjectSnapshot, UiNoteClip};
    use std::collections::HashMap;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    use vibez_core::id::{ClipId, TrackId};
    use vibez_core::perform::GrooveGrid;

    #[test]
    fn capture_stop_uses_the_atomic_transport_stop_boundary() {
        assert!(matches!(capture_stop_command(), EngineCommand::Stop));
    }

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

    fn audio_clip(position: u64, duration: u64) -> crate::state::UiClip {
        crate::state::UiClip {
            id: ClipId::new(),
            name: "Existing audio".into(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.5; 32]],
                sample_rate: 8,
            }),
            source: None,
            position,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
            original_audio: None,
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
            controlled_track_ids: vec![captured_track],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 4.0,
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
    fn silent_capture_detects_material_that_it_will_replace() {
        let track_id = TrackId::new();
        let mut arrange = ArrangementTimeline::default();
        arrange.ensure(track_id).note_clips.push(UiNoteClip {
            id: ClipId::new(),
            name: "Occupied interval".into(),
            position_beats: 4.0,
            duration_beats: 2.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        });

        let captured = MaterializedCapture {
            arrange_start_samples: 16,
            arrange_end_samples: 24,
            controlled_track_ids: vec![track_id],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 4.0,
            ..MaterializedCapture::default()
        };
        assert!(capture_replaces_existing_content(&arrange, &captured));
    }

    #[test]
    fn replacement_splits_straddling_content_and_pins_lane_edges() {
        let track_id = TrackId::new();
        let mut arrange = Arc::new(ArrangementTimeline::default());
        let content = Arc::make_mut(&mut arrange).ensure(track_id);
        let straddling_audio = audio_clip(2, 8);
        let original_audio_id = straddling_audio.id;
        content.clips.push(straddling_audio);
        content.note_clips.push(UiNoteClip {
            id: ClipId::new(),
            name: "Existing notes".into(),
            position_beats: 2.0,
            duration_beats: 8.0,
            notes: vec![
                vibez_core::midi::MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 1.0,
                    duration_beats: 3.0,
                },
                vibez_core::midi::MidiNote {
                    pitch: 64,
                    velocity: 100,
                    start_beat: 5.0,
                    duration_beats: 2.0,
                },
            ],
            selected_notes: Default::default(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        });
        let mut gain = AutomationLane::new(AutomationTarget::TrackGain);
        for (beat, value) in [(0.0, 0.0), (5.0, 1.0), (10.0, 0.0)] {
            gain.insert_point(AutomationPoint {
                beat,
                value,
                curve: 0.0,
            });
        }
        content.automation.push(gain);

        let captured = MaterializedCapture {
            arrange_start_samples: 4,
            arrange_end_samples: 6,
            by_track: HashMap::new(),
            controlled_track_ids: vec![track_id],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 1.0,
        };
        assert_eq!(
            apply_capture_to_arrangement(&mut arrange, captured, &mut None),
            0
        );
        let content = arrange.get(track_id).unwrap();

        assert_eq!(content.clips.len(), 2);
        assert_eq!(
            content
                .clips
                .iter()
                .map(|clip| (clip.position, clip.duration, clip.source_offset))
                .collect::<Vec<_>>(),
            [(2, 2, 0), (6, 4, 4)]
        );
        assert_eq!(content.clips[0].id, original_audio_id);
        assert_ne!(content.clips[1].id, original_audio_id);
        assert_eq!(
            content
                .note_clips
                .iter()
                .map(|clip| (clip.position_beats, clip.duration_beats))
                .collect::<Vec<_>>(),
            [(2.0, 2.0), (6.0, 4.0)]
        );
        assert_eq!(content.note_clips[0].notes[0].duration_beats, 1.0);
        assert_eq!(content.note_clips[1].notes[0].start_beat, 1.0);

        let lane = &content.automation[0];
        assert_eq!(
            lane.points
                .iter()
                .map(|point| point.beat)
                .collect::<Vec<_>>(),
            [0.0, 4.0, 6.0, 10.0]
        );
        assert!((lane.value_at(2.0).unwrap() - 0.4).abs() < 1e-6);
        assert!((lane.value_at(8.0).unwrap() - 0.4).abs() < 1e-6);
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
            controlled_track_ids: vec![track_id],
            pre_capture_mutes: HashMap::new(),
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
