//! Application boundary for Capture timing, project transaction, and engine sync.

use iced::Task;
use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::automation::AutomationTarget;
use vibez_core::id::TrackId;
use vibez_core::perform::SwingOffset;
use vibez_engine::commands::EngineCommand;

use crate::domains::arrangement::{lane_overwrite, ArrangementMsg};
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
                if self.state.perform.layout == vibez_project::PerformLayout::Clips {
                    self.state.perform.clip_editor.running = true;
                    self.send_command(EngineCommand::BeginClipPerformance);
                }
                self.send_command(EngineCommand::StartPerformanceCapture);
                self.state.status_text = "Starting Capture into Arrange…".into();
            }
            CaptureAction::Stop => {
                self.end_capture_automation_gesture();
                // Transport Stop publishes the Capture boundary first, then
                // stops Section playback in the same audio callback.
                self.send_command(capture_stop_command());
                self.state.status_text = "Stopping Capture and playback…".into();
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
            self.discard_project_transaction();
            self.state.status_text = "Capture stopped · no performance content recorded".into();
            return;
        };
        let materialized = completed.materialize();
        if materialized.is_empty()
            && !capture_replaces_existing_content(&self.state.arrangement.timeline, &materialized)
        {
            self.discard_project_transaction();
            self.state.status_text = "Capture stopped · no performance content recorded".into();
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
            let overwrite = lane_overwrite(
                existing,
                (captured.start_beat(), captured.end_beat()),
                &HashSet::new(),
                captured.samples_per_beat,
            );
            !overwrite.audio.is_empty()
                || !overwrite.notes.is_empty()
                || !existing.automation.is_empty()
        })
    })
}

fn apply_capture_to_arrangement(
    arrange: &mut Arc<crate::state::ArrangementTimeline>,
    captured: MaterializedCapture,
    engine: &mut impl crate::domains::EngineHandle,
) -> usize {
    let start_beat = captured.start_beat();
    let end_beat = captured.end_beat();
    let MaterializedCapture {
        mut by_track,
        mut controlled_track_ids,
        pre_capture_mutes,
        samples_per_beat,
        ..
    } = captured;
    let mut clip_count = 0;
    let timeline = Arc::make_mut(arrange);
    for track_id in by_track.keys().copied() {
        if !controlled_track_ids.contains(&track_id) {
            controlled_track_ids.push(track_id);
        }
    }

    engine.send(EngineCommand::BeginContentRebuild);
    for track_id in controlled_track_ids {
        let mut incoming = by_track.remove(&track_id).unwrap_or_default();
        let captured_mute = incoming
            .automation
            .iter()
            .any(|lane| lane.target == vibez_core::automation::AutomationTarget::TrackMute);
        clip_count += incoming.clips.len() + incoming.note_clips.len();

        let destination = timeline.ensure(track_id);
        let overwrite = lane_overwrite(
            destination,
            (start_beat, end_beat),
            &HashSet::new(),
            samples_per_beat,
        );
        let replaced_audio: HashSet<_> = overwrite.audio.iter().map(|(id, _)| *id).collect();
        let replaced_notes: HashSet<_> = overwrite.notes.iter().map(|(id, _)| *id).collect();
        destination
            .clips
            .retain(|clip| !replaced_audio.contains(&clip.id));
        destination
            .note_clips
            .retain(|clip| !replaced_notes.contains(&clip.id));
        let kept_audio = std::mem::take(&mut destination.clips);
        let kept_notes = std::mem::take(&mut destination.note_clips);
        for &clip_id in &replaced_audio {
            engine.send(EngineCommand::RemoveClip(track_id, clip_id));
        }
        for &clip_id in &replaced_notes {
            engine.send(EngineCommand::RemoveNoteClip(track_id, clip_id));
        }
        let mut added_audio: Vec<_> = overwrite
            .audio
            .into_iter()
            .flat_map(|(_, pieces)| pieces)
            .collect();
        added_audio.append(&mut incoming.clips);
        let mut added_notes: Vec<_> = overwrite
            .notes
            .into_iter()
            .flat_map(|(_, pieces)| pieces)
            .collect();
        added_notes.append(&mut incoming.note_clips);
        replace_automation_interval(
            &mut destination.automation,
            incoming.automation,
            start_beat,
            end_beat,
        );

        let audio_lane = kept_audio.len() + added_audio.len();
        for clip in &added_audio {
            engine.send(clip.add_command_beside(track_id, audio_lane));
        }
        let note_lane = kept_notes.len() + added_notes.len();
        for clip in &added_notes {
            engine.send(clip.add_command_beside(track_id, note_lane));
        }
        destination.clips = kept_audio;
        destination.clips.append(&mut added_audio);
        destination.clips.sort_by_key(|clip| clip.position);
        destination.note_clips = kept_notes;
        destination.note_clips.append(&mut added_notes);
        destination
            .note_clips
            .sort_by(|left, right| left.position_beats.total_cmp(&right.position_beats));
        for lane in &destination.automation {
            engine.send(EngineCommand::SetAutomationLane {
                track_id,
                insertion: vibez_engine::clip_insertion::PreparedAutomationLaneInsertion::new(
                    lane.clone(),
                ),
            });
        }
        if let Some(muted) = pre_capture_mutes.get(&track_id) {
            if captured_mute {
                engine.send(EngineCommand::SetTrackMute(track_id, *muted));
                engine.send(EngineCommand::SetAutomationOverride {
                    track_id,
                    target: vibez_core::automation::AutomationTarget::TrackMute,
                    overridden: false,
                });
            }
        }
    }
    engine.send(EngineCommand::EndContentRebuild);
    clip_count
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::{DiscardingEngine, EngineCommandQueue, EngineHandle};
    use crate::state::{AppState, ArrangementTimeline, ProjectSnapshot, UiNoteClip};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    use vibez_core::id::{ClipId, TrackId};
    use vibez_core::midi::{InstrumentKind, MidiNote};
    use vibez_core::perform::GrooveGrid;
    use vibez_engine::engine::AudioProcessBlock;

    struct NoteLogInstrument(Arc<Mutex<Vec<u8>>>);

    impl vibez_instruments::Instrument for NoteLogInstrument {
        fn instrument_kind(&self) -> InstrumentKind {
            InstrumentKind::SubtractiveSynth
        }

        fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
            &[]
        }

        fn set_param(&mut self, _index: usize, _value: f32) -> bool {
            false
        }

        fn get_param(&self, _index: usize) -> f32 {
            0.0
        }

        fn note_on(&mut self, pitch: u8, _velocity: u8) {
            self.0.lock().unwrap().push(pitch);
        }

        fn note_off(&mut self, _pitch: u8) {}

        fn render(&mut self, _buffer: &mut [f32], _channels: usize) {}

        fn reset(&mut self) {}
    }

    #[test]
    fn capture_stop_uses_the_atomic_transport_stop_boundary() {
        assert!(matches!(capture_stop_command(), EngineCommand::Stop));
    }

    fn snapshot(state: &AppState) -> ProjectSnapshot {
        state.project_snapshot()
    }

    fn audio_clip(position: u64, duration: u64) -> crate::state::UiClip {
        crate::state::UiClip {
            id: ClipId::new(),
            name: "Existing audio".into(),
            audio: Arc::new(DecodedAudio {
                channels: vec![vec![0.5; 32]],
                sample_rate: 8,
            }),
            source: crate::state::UiClip::test_source(),
            position,
            source_offset: 0,
            start_marker: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
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

        let mut engine = DiscardingEngine;
        assert_eq!(
            apply_capture_to_arrangement(&mut arrange, captured, &mut engine),
            0
        );
        assert_eq!(
            arrange.get(untouched_track).unwrap().automation,
            untouched_before
        );
        assert!(arrange.get(captured_track).is_some());
    }

    #[test]
    fn a_capture_holds_the_song_end_while_it_re_adds_its_tracks() {
        let track_id = TrackId::new();
        let mut arrange = Arc::new(ArrangementTimeline::default());
        Arc::make_mut(&mut arrange)
            .ensure(track_id)
            .clips
            .push(audio_clip(12, 8));
        let captured = MaterializedCapture {
            arrange_start_samples: 16,
            arrange_end_samples: 24,
            controlled_track_ids: vec![track_id],
            samples_per_beat: 4.0,
            ..MaterializedCapture::default()
        };

        let mut engine = crate::domains::test_support::RecordingEngine::default();
        apply_capture_to_arrangement(&mut arrange, captured, &mut engine);

        let edits: Vec<_> = engine
            .0
            .iter()
            .filter(|command| command.is_content_edit())
            .collect();
        assert!(edits.len() > 2, "the Capture re-adds the track's clip");
        assert!(matches!(
            edits.first(),
            Some(EngineCommand::BeginContentRebuild)
        ));
        assert!(matches!(
            edits.last(),
            Some(EngineCommand::EndContentRebuild)
        ));
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
            start_marker_beats: 0.0,
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
            start_marker_beats: 0.0,
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
        let mut engine = DiscardingEngine;
        assert_eq!(
            apply_capture_to_arrangement(&mut arrange, captured, &mut engine),
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
        // Each piece keeps the notes and stops where it ends, and the tail
        // picks its content up where the capture stopped, as a drop leaves it.
        assert_eq!(content.note_clips[0].notes.len(), 2);
        assert_eq!(content.note_clips[1].start_marker_beats, 4.0);

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
    fn a_looped_clip_a_capture_cuts_through_keeps_looping_either_side() {
        let track_id = TrackId::new();
        let mut arrange = Arc::new(ArrangementTimeline::default());
        Arc::make_mut(&mut arrange)
            .ensure(track_id)
            .note_clips
            .push(UiNoteClip {
                name: "Loop".into(),
                notes: vec![MidiNote {
                    pitch: 36,
                    velocity: 100,
                    start_beat: 1.0,
                    duration_beats: 0.5,
                }],
                loop_enabled: true,
                loop_end_beats: 4.0,
                ..UiNoteClip::test_clip(0.0, 16.0)
            });
        let captured = MaterializedCapture {
            arrange_start_samples: 6,
            arrange_end_samples: 9,
            by_track: HashMap::new(),
            controlled_track_ids: vec![track_id],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 1.0,
        };

        apply_capture_to_arrangement(&mut arrange, captured, &mut DiscardingEngine);

        let pieces = &arrange.get(track_id).unwrap().note_clips;
        assert_eq!(
            pieces
                .iter()
                .map(|clip| (clip.position_beats, clip.duration_beats, clip.loop_enabled))
                .collect::<Vec<_>>(),
            [(0.0, 6.0, true), (9.0, 7.0, true)]
        );
        // Nine beats into a four beat loop, the tail picks up one beat in.
        assert_eq!(pieces[1].start_marker_beats, 1.0);
    }

    #[test]
    fn a_song_sized_capture_sends_commands_per_clip_not_per_note() {
        let track_id = TrackId::new();
        let pattern = |position_beats: f64| UiNoteClip {
            name: "Pattern".into(),
            notes: (0..8)
                .map(|step| MidiNote {
                    pitch: 36,
                    velocity: 100,
                    start_beat: f64::from(step) * 0.5,
                    duration_beats: 0.25,
                })
                .collect(),
            ..UiNoteClip::test_clip(position_beats, 4.0)
        };
        let mut arrange = Arc::new(ArrangementTimeline::default());
        let song = Arc::make_mut(&mut arrange).ensure(track_id);
        song.note_clips = (0..40).map(|bar| pattern(f64::from(bar) * 4.0)).collect();
        let mut incoming = crate::state::TrackTimelineContent::default();
        incoming.note_clips.push(pattern(160.0));
        let captured = MaterializedCapture {
            arrange_start_samples: 640,
            arrange_end_samples: 656,
            by_track: HashMap::from([(track_id, incoming)]),
            controlled_track_ids: vec![track_id],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 4.0,
        };
        let mut engine = crate::domains::test_support::RecordingEngine::default();
        apply_capture_to_arrangement(&mut arrange, captured, &mut engine);

        let clips = arrange.get(track_id).unwrap().note_clips.len();
        assert_eq!(clips, 41);
        let rebuild_bracket = 2;
        assert!(
            engine.0.len() <= 2 * clips + rebuild_bracket,
            "{} commands for {clips} clips",
            engine.0.len()
        );
    }

    #[test]
    fn a_capture_leaves_the_engine_clips_outside_its_interval_alone() {
        let track_id = TrackId::new();
        let pattern = |position_beats: f64| UiNoteClip {
            id: ClipId::new(),
            name: "Pattern".into(),
            position_beats,
            duration_beats: 4.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        };
        let mut arrange = Arc::new(ArrangementTimeline::default());
        let song = Arc::make_mut(&mut arrange).ensure(track_id);
        song.note_clips = (0..40).map(|bar| pattern(f64::from(bar) * 4.0)).collect();
        song.clips = vec![audio_clip(0, 16), audio_clip(36, 8), audio_clip(400, 16)];
        let mut incoming = crate::state::TrackTimelineContent::default();
        incoming.note_clips.push(pattern(8.0));
        let captured = MaterializedCapture {
            arrange_start_samples: 32,
            arrange_end_samples: 48,
            by_track: HashMap::from([(track_id, incoming)]),
            controlled_track_ids: vec![track_id],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 4.0,
        };
        let mut engine = crate::domains::test_support::RecordingEngine::default();
        apply_capture_to_arrangement(&mut arrange, captured, &mut engine);

        let count = |matches: fn(&EngineCommand) -> bool| {
            engine.0.iter().filter(|command| matches(command)).count()
        };
        assert_eq!(
            count(|command| matches!(command, EngineCommand::RemoveNoteClip(..))),
            1
        );
        assert_eq!(
            count(|command| matches!(command, EngineCommand::AddNoteClip { .. })),
            1
        );
        assert_eq!(
            count(|command| matches!(command, EngineCommand::RemoveClip(..))),
            1
        );
        assert_eq!(
            count(|command| matches!(command, EngineCommand::AddClip { .. })),
            0
        );
        let content = arrange.get(track_id).unwrap();
        assert_eq!((content.note_clips.len(), content.clips.len()), (40, 2));
    }

    #[test]
    fn a_captured_clip_meets_the_content_it_keeps_after_the_take() {
        // 124 BPM at 44.1 kHz puts the stop between two ticks.
        let samples_per_beat: f64 = 44_100.0 * 60.0 / 124.0;
        let track_id = TrackId::new();
        let existing = UiNoteClip {
            id: ClipId::new(),
            name: "Existing".into(),
            position_beats: 0.0,
            duration_beats: 32.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        };
        let mut section = crate::domains::perform::Section::new(0);
        let mut groove = existing.clone();
        groove.duration_beats = section.length_beats;
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .note_clips
            .push(groove);
        let mut capture = crate::domains::perform::CaptureState::default();
        capture.update(crate::domains::perform::CaptureMsg::Toggle);
        capture.prepare(0, 44_100, 124.0);
        capture.start(
            0,
            Some((
                crate::domains::perform::CapturedTimelineSource::from_section(&section),
                0,
            )),
        );
        let stop = (10.3 * samples_per_beat).round() as u64;
        let take = capture.finish(stop).unwrap().materialize();
        let mut arrange = Arc::new(ArrangementTimeline::default());
        Arc::make_mut(&mut arrange)
            .ensure(track_id)
            .note_clips
            .push(existing);
        apply_capture_to_arrangement(&mut arrange, take, &mut DiscardingEngine);

        let clips = &arrange.get(track_id).unwrap().note_clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(
            clips[0].position_beats + clips[0].duration_beats,
            clips[1].position_beats
        );
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
            start_marker_beats: 0.0,
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
        let mut engine = DiscardingEngine;
        assert_eq!(
            apply_capture_to_arrangement(&mut state.arrangement.timeline, captured, &mut engine,),
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

    #[test]
    fn long_capture_reaches_later_tracks_after_the_command_queue_boundary() {
        let (mut audio_engine, mut producer, mut events) = vibez_engine::engine::AudioEngine::new();
        let filler_track = TrackId::new();
        let audible_track = TrackId::new();
        let heard = Arc::new(Mutex::new(Vec::new()));
        producer
            .push(EngineCommand::add_track(filler_track))
            .unwrap();
        producer
            .push(EngineCommand::add_track(audible_track))
            .unwrap();
        producer
            .push(EngineCommand::SetInstrument {
                track_id: audible_track,
                instrument: Box::new(NoteLogInstrument(Arc::clone(&heard))),
            })
            .unwrap();
        let channels = [filler_track, audible_track, TrackId::MASTER].map(|id| {
            vibez_core::routing::RoutingChannel {
                id,
                is_bus: id.is_master(),
                sends: vec![],
                effects: vec![],
            }
        });
        producer
            .push(EngineCommand::SetRouting(
                vibez_engine::routing::PreparedRouting::prepare(&channels, 512).unwrap(),
            ))
            .unwrap();
        audio_engine.process_block(AudioProcessBlock::new(&mut [0.0; 2], 2));

        let note_clip = |pitch, note_count| UiNoteClip {
            id: ClipId::new(),
            name: "Captured notes".into(),
            position_beats: 0.0,
            duration_beats: 1.0,
            notes: (0..note_count)
                .map(|_| MidiNote {
                    pitch,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 0.25,
                })
                .collect(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        };
        let filler = crate::state::TrackTimelineContent {
            note_clips: (0..vibez_core::constants::RING_BUFFER_CAPACITY)
                .map(|_| note_clip(36, 1))
                .collect(),
            ..Default::default()
        };
        let mut audible = crate::state::TrackTimelineContent::default();
        audible.note_clips.push(note_clip(72, 1));

        let captured = MaterializedCapture {
            arrange_start_samples: 0,
            arrange_end_samples: 22_050,
            by_track: HashMap::from([(filler_track, filler), (audible_track, audible)]),
            controlled_track_ids: vec![filler_track, audible_track],
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: 22_050.0,
        };
        let mut timeline = Arc::new(ArrangementTimeline::default());
        let mut commands =
            EngineCommandQueue::new(producer, audio_engine.take_transport_sender().unwrap());
        apply_capture_to_arrangement(&mut timeline, captured, &mut commands);
        assert_eq!(
            commands.pending_len(),
            3,
            "the last clip and the rebuild bracket"
        );

        let operations = vibez_core::constants::RING_BUFFER_CAPACITY + 1;
        audio_engine.process_block(AudioProcessBlock::new(&mut [], 2));
        commands.flush();
        assert_eq!(commands.pending_len(), 0);
        let mut completed = Vec::new();
        for _ in 0..=operations {
            while let Ok(event) = events.pop() {
                match event {
                    vibez_engine::events::EngineEvent::ClipInsertionStorageRequested(request) => {
                        request.fulfill();
                    }
                    vibez_engine::events::EngineEvent::ClipInsertionCompleted(owner) => {
                        assert_eq!(
                            owner.outcome,
                            vibez_engine::clip_insertion::ClipInsertionOutcome::Applied
                        );
                        completed.push(owner.track_id);
                    }
                    _ => {}
                }
            }
            commands.flush();
            if completed.len() == operations {
                break;
            }
            audio_engine.process_block(AudioProcessBlock::new(&mut [], 2));
        }
        assert_eq!(commands.pending_len(), 0);
        assert_eq!(completed.len(), operations);
        assert_eq!(completed.last(), Some(&audible_track));
        for track_id in [filler_track, audible_track] {
            let actual = &audio_engine
                .tracks()
                .iter()
                .find(|track| track.id == track_id)
                .unwrap()
                .playback_source
                .note_clips;
            let expected = &timeline.get(track_id).unwrap().note_clips;
            assert_eq!(actual.len(), expected.len());
            for (actual, expected) in actual.iter().zip(expected) {
                assert_eq!(actual.id, expected.id);
                assert_eq!(actual.notes, expected.notes);
            }
        }
        assert_eq!(audio_engine.transport().position(), 0);
        assert!(!audio_engine.transport().is_playing());
        assert!(heard.lock().unwrap().is_empty());
        commands.send(EngineCommand::Seek(0));
        commands.send(EngineCommand::Play);
        audio_engine.process_block(AudioProcessBlock::new(&mut [0.0; 512], 2));

        assert!(
            heard.lock().unwrap().contains(&72),
            "the UI committed the later Track, but its commands were dropped"
        );
    }
}
