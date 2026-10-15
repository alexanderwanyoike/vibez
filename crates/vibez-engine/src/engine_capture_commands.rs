//! Typed audio-thread capture commands helpers.

use super::*;
use crate::events::{
    InstrumentNoteInputEvent, PerformanceCaptureStartedEvent, SourceNoteInputEvent,
};

impl AudioEngine {
    pub(super) fn command_start_performance_capture(
        &mut self,
        allowance: &mut RetirementAllowance,
    ) {
        if let Some(reason) = self.observe_capture_admission().refusal_reason() {
            self.close_capture_for_device_failure();
            self.report_compensation_failure(TrackId::MASTER, None, reason);
            return;
        }
        if self.clip_performance {
            self.apply_clip_boundaries(self.performance_position);
        }
        let context = self.presentation_context(self.mix_latency());
        let section_id = context.section_id;
        let section_position_samples = context.section;
        self.capture_lifecycle.begin();
        self.present_event(
            EngineEvent::PerformanceCaptureStarted(PerformanceCaptureStartedEvent {
                offsets: self.capture_offsets(),
                effective_at_samples: self.heard_capture_position(),
                section_id,
                section_position_samples,
            }),
            allowance,
        );
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
                    .channel_clocks
                    .iter()
                    .find(|clock| clock.track == id)
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

    pub(super) fn command_stop_performance_capture(&mut self, allowance: &mut RetirementAllowance) {
        self.capture_lifecycle.end();
        self.present_event(
            EngineEvent::PerformanceCaptureStopped {
                effective_at_samples: self.heard_capture_position(),
            },
            allowance,
        );
    }

    pub(super) fn command_external_note_on(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pitch: u8,
        velocity: u8,
        allowance: &mut RetirementAllowance,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.as_mut() {
                instrument.note_on(pitch, velocity);
            }
        }
        let context = self.live_capture_context(track_id);
        self.present_event(
            EngineEvent::SourceNoteInput(SourceNoteInputEvent {
                track_id,
                pitch,
                velocity,
                on: true,
                position: self.source_recording_position(),
            }),
            allowance,
        );
        self.present_event_after(
            EngineEvent::InstrumentNoteInput(InstrumentNoteInputEvent {
                track_id,
                pitch,
                velocity,
                on: true,
                effective_at_samples: context.perform,
                section_id: context.section_id,
                section_position_samples: context.section,
            }),
            self.live_path_latency(track_id),
            allowance,
        );
    }

    pub(super) fn command_external_note_off(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pitch: u8,
        allowance: &mut RetirementAllowance,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.as_mut() {
                instrument.note_off(pitch);
            }
        }
        let context = self.live_capture_context(track_id);
        self.present_event(
            EngineEvent::SourceNoteInput(SourceNoteInputEvent {
                track_id,
                pitch,
                velocity: 0,
                on: false,
                position: self.source_recording_position(),
            }),
            allowance,
        );
        self.present_event_after(
            EngineEvent::InstrumentNoteInput(InstrumentNoteInputEvent {
                track_id,
                pitch,
                velocity: 0,
                on: false,
                effective_at_samples: context.perform,
                section_id: context.section_id,
                section_position_samples: context.section,
            }),
            self.live_path_latency(track_id),
            allowance,
        );
    }
}
