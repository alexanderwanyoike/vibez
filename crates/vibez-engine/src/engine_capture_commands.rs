//! Typed audio-thread capture commands helpers.

use super::*;

impl AudioEngine {
    pub(super) fn command_start_performance_capture(&mut self) {
        if self.local_device_failure_pending() {
            self.close_capture_for_device_failure();
            self.report_compensation_failure(
                TrackId::MASTER,
                None,
                "Capture requires recovery or reload of the failed device",
            );
            return;
        }
        if !self.compensation_valid {
            self.close_capture_for_device_failure();
            self.report_compensation_failure(
                TrackId::MASTER,
                None,
                "Capture requires a valid applied audio configuration",
            );
            return;
        }
        if self.clip_performance {
            self.apply_clip_boundaries(self.performance_position);
        }
        let context = self.presentation_context(self.mix_latency());
        let section_id = context.section_id;
        let section_position_samples = context.section;
        self.capture_active = true;
        self.present_event(EngineEvent::PerformanceCaptureStarted {
            offsets: self.capture_offsets(),
            effective_at_samples: self.heard_capture_position(),
            section_id,
            section_position_samples,
        });
        self.capture_clip_positions.clear();
        for index in 0..self.tracks.len() {
            {
                let id = self.tracks[index].id;
                let delay = self.live_path_latency(id);
                let current = self.tracks[index]
                    .active_clip
                    .map_or(0, |active| active.position);
                let position = self
                    .routing
                    .as_ref()
                    .and_then(|routing| {
                        routing
                            .channel_clocks
                            .iter()
                            .find(|clock| clock.track == id)
                    })
                    .and_then(|clock| clock.before_block(delay))
                    .unwrap_or(current.saturating_sub(delay as u64));
                if self.capture_clip_positions.len() < self.capture_clip_positions.capacity() {
                    self.capture_clip_positions
                        .push((id, position, self.heard_capture_position()));
                }
            }
        }
        self.replay_early_section_capture();
    }

    pub(super) fn command_stop_performance_capture(&mut self) {
        self.capture_active = false;
        self.present_event(EngineEvent::PerformanceCaptureStopped {
            effective_at_samples: self.heard_capture_position(),
        });
    }

    pub(super) fn command_external_note_on(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pitch: u8,
        velocity: u8,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.as_mut() {
                instrument.note_on(pitch, velocity);
            }
        }
        let context = self.live_capture_context(track_id);
        self.present_event(EngineEvent::SourceNoteInput {
            track_id,
            pitch,
            velocity,
            on: true,
            position: self.source_recording_position(),
        });
        self.present_event_after(
            EngineEvent::InstrumentNoteInput {
                track_id,
                pitch,
                velocity,
                on: true,
                effective_at_samples: context.perform,
                section_id: context.section_id,
                section_position_samples: context.section,
            },
            self.live_path_latency(track_id),
        );
    }

    pub(super) fn command_external_note_off(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pitch: u8,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.as_mut() {
                instrument.note_off(pitch);
            }
        }
        let context = self.live_capture_context(track_id);
        self.present_event(EngineEvent::SourceNoteInput {
            track_id,
            pitch,
            velocity: 0,
            on: false,
            position: self.source_recording_position(),
        });
        self.present_event_after(
            EngineEvent::InstrumentNoteInput {
                track_id,
                pitch,
                velocity: 0,
                on: false,
                effective_at_samples: context.perform,
                section_id: context.section_id,
                section_position_samples: context.section,
            },
            self.live_path_latency(track_id),
        );
    }
}
