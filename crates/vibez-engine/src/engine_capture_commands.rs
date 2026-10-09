//! Ownership-preserving dispatch for this engine command boundary.

use super::*;

impl AudioEngine {
    // Inline error ownership avoids allocating a command box in the callback.
    #[allow(clippy::result_large_err)]
    pub(super) fn handle_capture_command(
        &mut self,
        cmd: EngineCommand,
    ) -> Result<(), EngineCommand> {
        match cmd {
            EngineCommand::StartPerformanceCapture => {
                if !self.compensation_valid {
                    self.close_capture_on_failure();
                    self.report_compensation_failure(
                        TrackId::MASTER,
                        None,
                        "Capture requires a valid applied audio configuration",
                    );
                    return Ok(());
                }
                if self.clip_performance {
                    self.apply_clip_boundaries(self.performance_position);
                }
                let section_id = self.active_section.map(|section| section.section_id);
                let section_position_samples =
                    self.active_section.map(|section| section.position_samples);
                let _ = self.event_tx.push(EngineEvent::PerformanceCaptureStarted {
                    effective_at_samples: self.effective_position(),
                    section_id,
                    section_position_samples,
                });
                for index in 0..self.tracks.len() {
                    if let Some(active) = self.tracks[index].active_clip {
                        self.clip_event(EngineEvent::ClipCaptureSource {
                            track_id: self.tracks[index].id,
                            position: active.position,
                            effective_at_samples: self.performance_position,
                        });
                    }
                }
            }
            EngineCommand::StopPerformanceCapture => {
                let _ = self.event_tx.push(EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples: self.effective_position(),
                });
            }
            EngineCommand::ExternalNoteOn {
                track_id,
                pitch,
                velocity,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.note_on(pitch, velocity);
                    }
                }
                let section = self.active_section;
                let _ = self.event_tx.push(EngineEvent::InstrumentNoteInput {
                    track_id,
                    pitch,
                    velocity,
                    on: true,
                    effective_at_samples: self.performance_position,
                    section_id: section.map(|active| active.section_id),
                    section_position_samples: section.map(|active| active.position_samples),
                });
            }
            EngineCommand::ExternalNoteOff { track_id, pitch } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.note_off(pitch);
                    }
                }
                let section = self.active_section;
                let _ = self.event_tx.push(EngineEvent::InstrumentNoteInput {
                    track_id,
                    pitch,
                    velocity: 0,
                    on: false,
                    effective_at_samples: self.performance_position,
                    section_id: section.map(|active| active.section_id),
                    section_position_samples: section.map(|active| active.position_samples),
                });
            }
            other => return Err(other),
        }
        Ok(())
    }
}
