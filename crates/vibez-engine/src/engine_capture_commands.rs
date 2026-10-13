//! Typed audio-thread commands with explicit ownership and arguments.

use super::*;

impl AudioEngine {
    pub(super) fn command_start_performance_capture(&mut self) {
        if !self.compensation_valid || self.local_device_failure_pending() {
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
        let section_id = self.active_section.map(|section| section.section_id);
        let section_position_samples = self.active_section.map(|section| section.position_samples);
        self.capture_active = true;
        self.present_event(EngineEvent::PerformanceCaptureStarted {
            effective_at_samples: self.effective_position(),
            section_id,
            section_position_samples,
        });
        for index in 0..self.tracks.len() {
            if let Some(active) = self.tracks[index].active_clip {
                if !self.presentation_room(1) {
                    return;
                }
                self.clip_event(EngineEvent::ClipCaptureSource {
                    track_id: self.tracks[index].id,
                    position: active.position,
                    effective_at_samples: self.performance_position,
                });
            }
        }
    }

    pub(super) fn command_stop_performance_capture(&mut self) {
        self.capture_active = false;
        self.present_event(EngineEvent::PerformanceCaptureStopped {
            effective_at_samples: self.effective_position(),
        });
    }

    pub(super) fn command_external_note_on(&mut self, track_id: TrackId, pitch: u8, velocity: u8) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.as_mut() {
                instrument.note_on(pitch, velocity);
            }
        }
        self.present_event(EngineEvent::SourceNoteInput {
            track_id,
            pitch,
            velocity,
            on: true,
            position: self.source_recording_position(),
        });
        let section = self.active_section;
        self.present_event(EngineEvent::InstrumentNoteInput {
            track_id,
            pitch,
            velocity,
            on: true,
            effective_at_samples: self.performance_position,
            section_id: section.map(|active| active.section_id),
            section_position_samples: section.map(|active| active.position_samples),
        });
    }

    pub(super) fn command_external_note_off(&mut self, track_id: TrackId, pitch: u8) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.as_mut() {
                instrument.note_off(pitch);
            }
        }
        self.present_event(EngineEvent::SourceNoteInput {
            track_id,
            pitch,
            velocity: 0,
            on: false,
            position: self.source_recording_position(),
        });
        let section = self.active_section;
        self.present_event(EngineEvent::InstrumentNoteInput {
            track_id,
            pitch,
            velocity: 0,
            on: false,
            effective_at_samples: self.performance_position,
            section_id: section.map(|active| active.section_id),
            section_position_samples: section.map(|active| active.position_samples),
        });
    }
}
