use super::*;
use crate::domains::arrangement::ArrangementMsg;

impl App {
    pub(super) fn update_audio_edits(
        &mut self,
        message: Message,
        undo_gesture: Option<crate::state::UndoGestureId>,
    ) -> Task<Message> {
        match message {
            Message::QuantizeAudioClip { track_id, clip_id } => {
                let grid = self
                    .state
                    .view
                    .grid_config()
                    .effective_grid(self.active_editor_pixels_per_beat());
                return self.dispatch_audio_quantize(
                    self.active_timeline_location(),
                    track_id,
                    clip_id,
                    grid,
                );
            }
            Message::QuantizeAudioClipAt {
                track_id,
                clip_id,
                grid,
            } => {
                return self.dispatch_audio_quantize(
                    self.active_timeline_location(),
                    track_id,
                    clip_id,
                    grid,
                );
            }
            Message::AudioQuantizeReady {
                location,
                track_id,
                old_clip_id,
                result,
            } => match result {
                Ok(success) => {
                    let sample_rate = self.state.transport.sample_rate;
                    let action = self.apply_audio_quantize_success_at(
                        location,
                        track_id,
                        old_clip_id,
                        success,
                        sample_rate,
                    );
                    return self.apply_arrangement_action_at(action, location);
                }
                Err(err) => {
                    self.state.status_text = format!("Quantize failed: {err}");
                }
            },

            // -- Warping --
            Message::DetectClipBpm {
                location,
                track_id,
                clip_id,
            } => {
                return self.dispatch_detect_clip_bpm(location, track_id, clip_id);
            }
            Message::DetectClipTransients {
                location,
                track_id,
                clip_id,
                sensitivity,
            } => {
                return self.dispatch_detect_clip_transients(
                    location,
                    track_id,
                    clip_id,
                    sensitivity,
                    true,
                );
            }
            Message::ClipTransientsDetected(completion) => {
                return self.finish_detect_clip_transients(completion, undo_gesture);
            }
            Message::ClipBpmDetected {
                location,
                track_id,
                clip_id,
                bpm,
                confidence,
            } => {
                let action =
                    self.apply_clip_bpm_detected_at(location, track_id, clip_id, bpm, confidence);
                return self.apply_arrangement_action_at(action, location);
            }
            Message::WarpClipToProject {
                location,
                track_id,
                clip_id,
            } => {
                return self.dispatch_warp_clip_to_project(location, track_id, clip_id, true);
            }
            Message::ClipWarpReady {
                location,
                track_id,
                clip_id,
                record_undo: _,
                result,
            } => match result {
                Ok(success) => {
                    let action =
                        self.apply_clip_warp_success_at(location, track_id, clip_id, success);
                    return self.apply_arrangement_action_at(action, location);
                }
                Err(err) => {
                    self.state.status_text = format!("Warp failed: {err}");
                }
            },
            Message::ClipTransposeReady {
                location,
                track_id,
                clip_id,
                result,
            } => match result {
                Ok(success) => {
                    let action =
                        self.with_timeline_editor_at(location, |editor, _tracks, engine| {
                            editor.apply_clip_transpose_success(engine, track_id, clip_id, success)
                        });
                    return self.apply_arrangement_action_at(action, location);
                }
                Err(error) => {
                    self.state.status_text = format!("Transpose failed: {error}");
                }
            },
            Message::CommitAudioClipTransposeAfterDelay {
                location,
                track_id,
                clip_id,
                expected_semitones,
                expected_revision,
            } => {
                if self.active_timeline_location() != location {
                    return Task::none();
                }
                let expected = expected_semitones.to_string();
                let still_current = self
                    .state
                    .active_timeline_editor()
                    .audio_clip_inspector_edits
                    .get(&(clip_id, crate::state::AudioClipInspectorField::Transpose))
                    == Some(&expected)
                    && self
                        .state
                        .active_timeline_editor()
                        .audio_clip_transpose_debounce
                        .get(&clip_id)
                        == Some(&expected_revision);
                if !still_current {
                    return Task::none();
                }
                return self.update(Message::Arrangement(
                    ArrangementMsg::SubmitAudioClipInspectorField {
                        track_id,
                        clip_id,
                        field: crate::state::AudioClipInspectorField::Transpose,
                    },
                ));
            }

            // -- Undo / redo --

            // -- Export --
            _ => unreachable!("audio edit messages are routed by App::update"),
        }
        Task::none()
    }
}
