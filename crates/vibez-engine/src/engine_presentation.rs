use super::*;

pub(super) struct SectionCaptureTiming {
    section_id: SectionId,
    effective_at_samples: u64,
    position: u64,
    refreshed: bool,
    ended: bool,
    offsets: Arc<[(TrackId, u32)]>,
    early_due: u64,
    full_due: u64,
}

pub(super) struct CaptureReplay {
    next: usize,
    through: usize,
    physical_start: u64,
    clip: usize,
}

impl AudioEngine {
    pub(super) fn cancel_capture_timing(&mut self) {
        self.capture_replay = None;
        self.capture_clip_positions.clear();
        for timing in &mut self.section_capture_timing {
            timing.full_due = self.output_position;
        }
    }

    pub(super) fn section_capture_source(
        &mut self,
        section_id: SectionId,
        effective_at_samples: u64,
        position: u64,
        refreshed: bool,
    ) {
        let early_delay = self.earliest_source_latency();
        let physical = self
            .output_position
            .saturating_add(self.rendered_callback_frames as u64);
        let offsets = self.capture_offsets();
        if early_delay < self.mix_latency()
            && self.section_capture_timing.len() < self.section_capture_timing.capacity()
        {
            self.section_capture_timing.push(SectionCaptureTiming {
                section_id,
                effective_at_samples,
                position,
                refreshed,
                ended: false,
                offsets: Arc::clone(&offsets),
                early_due: physical.saturating_add(early_delay as u64),
                full_due: physical.saturating_add(self.mix_latency() as u64),
            });
        }
        self.present_event_after(
            EngineEvent::SectionCaptureSource {
                section_id,
                effective_at_samples,
                section_position_samples: position,
                refreshed,
                offsets,
            },
            early_delay,
        );
    }

    pub(super) fn section_capture_stopped(&mut self, effective_at_samples: u64) {
        let early_delay = self.earliest_source_latency();
        let physical = self
            .output_position
            .saturating_add(self.rendered_callback_frames as u64);
        if early_delay < self.mix_latency()
            && self.section_capture_timing.len() < self.section_capture_timing.capacity()
        {
            if let Some(section) = self.active_section {
                self.section_capture_timing.push(SectionCaptureTiming {
                    section_id: section.section_id,
                    effective_at_samples,
                    position: 0,
                    refreshed: false,
                    ended: true,
                    offsets: self.capture_offsets(),
                    early_due: physical.saturating_add(early_delay as u64),
                    full_due: physical.saturating_add(self.mix_latency() as u64),
                });
            }
        }
        self.present_event_after(
            EngineEvent::SectionCaptureStopped {
                effective_at_samples,
            },
            early_delay,
        );
    }

    pub(super) fn replay_early_section_capture(&mut self) {
        self.capture_replay = Some(CaptureReplay {
            next: 0,
            through: self.section_capture_timing.len(),
            physical_start: self.output_position,
            clip: 0,
        });
        self.continue_capture_replay();
    }

    pub(super) fn continue_capture_replay(&mut self) {
        let Some(mut replay) = self.capture_replay.take() else {
            return;
        };
        while replay.next < replay.through {
            if self.scheduled_presentation.len() == self.scheduled_presentation.capacity() {
                self.capture_replay = Some(replay);
                return;
            }
            let timing = &self.section_capture_timing[replay.next];
            replay.next += 1;
            if timing.early_due <= replay.physical_start && replay.physical_start < timing.full_due
            {
                let event = if timing.ended {
                    EngineEvent::SectionCaptureStopped {
                        effective_at_samples: timing.effective_at_samples,
                    }
                } else {
                    EngineEvent::SectionCaptureSource {
                        section_id: timing.section_id,
                        effective_at_samples: timing.effective_at_samples,
                        section_position_samples: timing.position,
                        refreshed: timing.refreshed,
                        offsets: Arc::clone(&timing.offsets),
                    }
                };
                self.present_event(event);
            }
        }
        while replay.clip < self.capture_clip_positions.len() {
            if self.scheduled_presentation.len() == self.scheduled_presentation.capacity() {
                self.capture_replay = Some(replay);
                return;
            }
            let (track_id, position, effective_at_samples) =
                self.capture_clip_positions[replay.clip];
            replay.clip += 1;
            self.present_event(EngineEvent::ClipCaptureSource {
                track_id,
                position,
                effective_at_samples,
            });
        }
        self.capture_clip_positions.clear();
    }

    pub(super) fn retire_section_capture_timing(&mut self) {
        if self.capture_replay.is_some() {
            return;
        }
        while self.pending_retirements.len() < self.pending_retirements.capacity() {
            let Some(index) = self
                .section_capture_timing
                .iter()
                .position(|timing| timing.full_due <= self.output_position)
            else {
                break;
            };
            let timing = self.section_capture_timing.remove(index);
            self.retire_event(EngineEvent::CaptureTimingRetired(timing.offsets));
        }
    }

    pub(super) fn report_compensation_failure(
        &mut self,
        track_id: TrackId,
        effect_id: Option<vibez_core::id::EffectId>,
        reason: &'static str,
    ) {
        self.pending_compensation_failure
            .get_or_insert((track_id, effect_id, reason));
        self.flush_presentation();
    }

    pub(super) fn close_capture_for_device_failure(&mut self) {
        self.cancel_capture_presentation();
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.flush_presentation();
    }

    pub(super) fn close_capture_on_failure(&mut self) {
        self.compensation_failure_reported = true;
        self.cancel_presentation();
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.flush_presentation();
    }

    pub(super) fn stop_heard_capture(&mut self) {
        let boundary = self.heard_capture_position();
        self.cancel_capture_presentation();
        self.present_event(EngineEvent::PerformanceCaptureStopped {
            effective_at_samples: boundary,
        });
    }

    pub(super) fn capture_offsets(&self) -> Arc<[(TrackId, u32)]> {
        self.routing.as_ref().map_or_else(
            || Arc::clone(&self.empty_capture_offsets),
            |routing| Arc::clone(&routing.capture_offsets),
        )
    }
    pub(super) fn earliest_source_latency(&self) -> u32 {
        self.routing.as_ref().map_or(0, |routing| {
            routing
                .channels
                .iter()
                .filter(|channel| channel.source)
                .map(|channel| channel.direct_latency)
                .min()
                .unwrap_or(routing.compensation.output_latency)
        })
    }
    pub(super) fn mix_latency(&self) -> u32 {
        self.routing
            .as_ref()
            .map_or(0, |routing| routing.compensation.output_latency)
    }

    pub(super) fn live_path_latency(&self, track: TrackId) -> u32 {
        self.routing.as_ref().map_or(0, |routing| {
            routing
                .channels
                .iter()
                .find(|channel| channel.id == track)
                .map_or(routing.compensation.output_latency, |channel| {
                    channel.direct_latency
                })
        })
    }

    pub(super) fn presentation_context(
        &self,
        delay: u32,
    ) -> crate::compensation_clock::PresentationPosition {
        self.routing
            .as_ref()
            .and_then(|routing| routing.presentation.before_block(delay))
            .unwrap_or_else(|| {
                let active = self.active_section;
                crate::compensation_clock::PresentationPosition {
                    arrange: self.transport.position().saturating_sub(delay as u64),
                    perform: self.performance_position.saturating_sub(delay as u64),
                    section: active
                        .map(|section| section.position_samples.saturating_sub(delay as u64)),
                    section_id: active.map(|section| section.section_id),
                    section_length: active.map_or(0, |section| section.length_samples),
                    generation: self
                        .routing
                        .as_ref()
                        .map_or(0, |routing| routing.compensation.generation),
                }
            })
    }

    pub(super) fn live_capture_context(
        &self,
        track: TrackId,
    ) -> crate::compensation_clock::PresentationPosition {
        self.presentation_context(
            self.mix_latency()
                .saturating_sub(self.live_path_latency(track)),
        )
    }

    pub(super) fn heard_capture_position(&self) -> u64 {
        let context = self.presentation_context(self.mix_latency());
        if self.clock_domain == ClockDomain::Perform || context.section_id.is_some() {
            context.perform
        } else {
            context.arrange
        }
    }
}
