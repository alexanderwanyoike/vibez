use super::*;

impl AudioEngine {
    // Returning the inline owner avoids allocating a command box in the callback.
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
                let context = self.presentation_context(self.mix_latency());
                let section_id = context.section_id;
                let section_position_samples = context.section;
                self.present_event(
                    EngineEvent::PerformanceCaptureStarted {
                        offsets: self.capture_offsets(),
                        effective_at_samples: self.heard_capture_position(),
                        section_id,
                        section_position_samples,
                    },
                    0,
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
                            .as_ref()
                            .and_then(|routing| {
                                routing
                                    .channel_clocks
                                    .iter()
                                    .find(|clock| clock.track == id)
                            })
                            .and_then(|clock| clock.before_block(delay))
                            .unwrap_or(current.saturating_sub(delay as u64));
                        if self.capture_clip_positions.len()
                            < self.capture_clip_positions.capacity()
                        {
                            self.capture_clip_positions.push((
                                id,
                                position,
                                self.heard_capture_position(),
                            ));
                        }
                    }
                }
                self.replay_early_section_capture();
            }
            EngineCommand::StopPerformanceCapture => {
                self.pending_capture_stop
                    .get_or_insert(self.heard_capture_position());
                self.flush_presentation();
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
                let context = self.live_capture_context(track_id);
                self.present_event(
                    EngineEvent::SourceNoteInput {
                        track_id,
                        pitch,
                        velocity,
                        on: true,
                        position: self.source_recording_position(),
                    },
                    0,
                );
                self.present_event(
                    EngineEvent::InstrumentNoteInput {
                        recording: self.source_recording_position(),
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
            EngineCommand::ExternalNoteOff { track_id, pitch } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.as_mut() {
                        instrument.note_off(pitch);
                    }
                }
                let context = self.live_capture_context(track_id);
                self.present_event(
                    EngineEvent::SourceNoteInput {
                        track_id,
                        pitch,
                        velocity: 0,
                        on: false,
                        position: self.source_recording_position(),
                    },
                    0,
                );
                self.present_event(
                    EngineEvent::InstrumentNoteInput {
                        recording: self.source_recording_position(),
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
            cmd => return Err(cmd),
        }
        Ok(())
    }
}
