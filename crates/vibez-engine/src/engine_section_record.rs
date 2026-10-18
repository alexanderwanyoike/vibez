//! Engine-clock ownership for Section Record arm/start/stop boundaries.

use super::presentation_queue::{PresentationAdmission, ScheduledPresentation};
use super::*;
use crate::events::{
    SectionRecordArmedEvent, SectionRecordStartedEvent, SectionRecordStoppedEvent,
};

pub(super) struct PendingSectionRecord {
    pub(super) section_id: SectionId,
    pub(super) track_id: TrackId,
    pub(super) prepared: Option<Box<PreparedSectionPlaybackSource>>,
    pub(super) effective_at_samples: u64,
    pub(super) section_position_samples: u64,
    pub(super) count_in_start_samples: u64,
    pub(super) count_in_beat_samples: u64,
    pub(super) replace_existing: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CountInClickTiming {
    pub(super) start: u64,
    pub(super) beat_samples: u64,
    pub(super) boundary: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ActiveSectionRecord {
    pub(super) section_id: SectionId,
    pub(super) track_id: TrackId,
    pub(super) effective_at_samples: u64,
    pub(super) replace_first_pass: bool,
    pub(super) replace_source_flushed: bool,
}

pub(crate) fn section_sample_for_performance(
    local_segment_start: u64,
    performance_segment_start: u64,
    event_sample: u64,
    section_length: u64,
) -> u64 {
    if section_length == 0 {
        return 0;
    }
    let local = i128::from(local_segment_start) + i128::from(event_sample)
        - i128::from(performance_segment_start);
    local.rem_euclid(i128::from(section_length)) as u64
}

impl AudioEngine {
    pub(super) fn arm_section_record(
        &mut self,
        section_id: SectionId,
        track_id: TrackId,
        prepared: Option<Box<PreparedSectionPlaybackSource>>,
        count_in_bars: u8,
        replace_existing: bool,
        allowance: &mut RetirementAllowance,
    ) {
        if self.pending_section_record.is_some() || self.active_section_record.is_some() {
            return;
        }
        self.cancel_section_queue();
        self.begin_performance_clock(allowance);

        let now = self.performance_position;
        let count_in_beat_samples = self.section_length_samples(1.0);
        let (effective_at_samples, section_position_samples, count_in_start_samples, prepared) =
            if let Some(active) = self
                .active_section
                .filter(|active| active.section_id == section_id && self.transport.is_playing())
            {
                let bar_samples = self.section_length_samples(4.0);
                let next_local = active
                    .position_samples
                    .checked_div(bar_samples)
                    .unwrap_or(0)
                    .saturating_add(1)
                    .saturating_mul(bar_samples);
                let (delta, local) = if next_local >= active.length_samples {
                    (
                        active
                            .length_samples
                            .saturating_sub(active.position_samples),
                        0,
                    )
                } else {
                    (
                        next_local.saturating_sub(active.position_samples),
                        next_local,
                    )
                };
                (now.saturating_add(delta), local, now, None)
            } else {
                let Some(prepared) = prepared else {
                    return;
                };
                let count_in_samples = self
                    .section_length_samples(4.0)
                    .saturating_mul(u64::from(count_in_bars));
                if !self.transport.is_playing() {
                    self.transport.play();
                    self.stopped_note_repeat_anchor = None;
                    self.present_event(EngineEvent::PlaybackStarted, allowance);
                }
                let count_in_start_samples = self.performance_position;
                (
                    count_in_start_samples.saturating_add(count_in_samples),
                    0,
                    count_in_start_samples,
                    Some(prepared),
                )
            };

        self.pending_section_record = Some(PendingSectionRecord {
            section_id,
            track_id,
            prepared,
            effective_at_samples,
            section_position_samples,
            count_in_start_samples,
            count_in_beat_samples,
            replace_existing,
        });
        self.present_event(
            EngineEvent::SectionRecordArmed(SectionRecordArmedEvent {
                section_id,
                track_id,
                effective_at_samples,
                section_position_samples,
            }),
            allowance,
        );
        self.start_section_record_if_due(self.performance_position);
    }

    pub(super) fn stop_section_record(&mut self) {
        // The preceding stop keeps this recording slot until its handoff succeeds.
        if self.pending_section_record_stop.is_some() {
            return;
        }
        let now = self.performance_position;
        let local = self
            .active_section
            .map(|section| section.position_samples)
            .unwrap_or(0);
        let (section_id, track_id, started, retired) =
            if let Some(active) = self.active_section_record.take() {
                (active.section_id, active.track_id, true, None)
            } else if let Some(pending) = self.pending_section_record.take() {
                (
                    pending.section_id,
                    pending.track_id,
                    false,
                    pending.prepared,
                )
            } else {
                return;
            };
        self.pending_section_record_stop = Some(SectionRecordStoppedEvent {
            section_id,
            track_id,
            effective_at_samples: now,
            section_position_samples: local,
            started,
            retired: retired.map(crate::events::DisposalCell::new),
        });
        self.flush_section_record_stop();
    }

    pub(crate) fn flush_section_record_stop(&mut self) -> bool {
        if self.pending_section_record_stop.is_none() {
            return true;
        }
        self.flush_presentation();
        if !self.presentation_return_has_room() {
            // Preserve the original failure boundary even when an earlier
            // lifetime still owns the inline discovery slot.
            self.fail_presentation();
            if !self.presentation_return_has_room() {
                return false;
            }
        }
        let mut allowance = self.pending_retirements.reserve(1);
        let stopped = self
            .pending_section_record_stop
            .take()
            .expect("one retained Section recording stop");
        let now = self.output_position + self.rendered_callback_frames as u64;
        // This literal remains SectionRecordStopped through cancellation. The
        // typed slot survives a failed handoff without owning a waiting credit.
        if let Err(EngineEvent::SectionRecordStopped(stopped)) =
            self.try_present_event_at(EngineEvent::SectionRecordStopped(stopped), now)
        {
            if let Some(allowance) = allowance.as_mut() {
                self.retire_event(EngineEvent::SectionRecordStopped(stopped), allowance);
            } else {
                self.pending_section_record_stop = Some(stopped);
                return false;
            }
        }
        true
    }

    pub(super) fn start_section_record_if_due(&mut self, through_sample: u64) {
        let due = self
            .pending_section_record
            .as_ref()
            .is_some_and(|pending| pending.effective_at_samples <= through_sample);
        if !due || !self.presentation_room(4) {
            return;
        }
        let mut pending = self.pending_section_record.take().expect("due recording");
        if let Some(prepared) = pending.prepared.take() {
            self.activate_section(prepared, pending.effective_at_samples);
            if self.presentation_state.is_faulted() {
                return;
            }
        }
        let Some(admission) = PresentationAdmission::<1>::new(
            &mut self.event_tx,
            &mut self.scheduled_presentation,
            &mut self.presentation_state.terminals,
        ) else {
            self.pending_section_record = Some(pending);
            return;
        };
        self.active_section_record = Some(ActiveSectionRecord {
            section_id: pending.section_id,
            track_id: pending.track_id,
            effective_at_samples: pending.effective_at_samples,
            replace_first_pass: pending.replace_existing,
            replace_source_flushed: false,
        });
        let event = EngineEvent::SectionRecordStarted(SectionRecordStartedEvent {
            section_id: pending.section_id,
            track_id: pending.track_id,
            effective_at_samples: pending.effective_at_samples,
            section_position_samples: pending.section_position_samples,
        });
        let now = self.output_position + self.rendered_callback_frames as u64;
        admission.publish(
            [Some(ScheduledPresentation { due: now, event })],
            now,
            self.output_position,
        );
    }

    /// A stopped-start count-in may end inside an audio callback. Render the
    /// count-in prefix, activate the Section exactly at the boundary, then
    /// render from Section beat zero for the remainder of the callback.
    pub(super) fn process_section_record_count_in(
        &mut self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
    ) -> bool {
        let Some(timing) = self.pending_section_record.as_ref().and_then(|pending| {
            pending.prepared.as_ref().map(|_| CountInClickTiming {
                start: pending.count_in_start_samples,
                beat_samples: pending.count_in_beat_samples,
                boundary: pending.effective_at_samples,
            })
        }) else {
            return false;
        };
        let block_start = self.performance_position;
        let block_end = block_start.saturating_add(frames as u64);
        if timing.boundary <= block_start {
            self.start_section_record_if_due(block_start);
            return false;
        }
        if timing.boundary >= block_end {
            self.render_idle_instruments(output, frames, channels, block_start, None, None);
            self.mix_record_count_in_click(output, frames, channels, block_start, timing);
            return true;
        }

        let frames_before = (timing.boundary - block_start) as usize;
        if frames_before > 0 {
            self.render_idle_instruments(
                &mut output[..frames_before * channels],
                frames_before,
                channels,
                block_start,
                None,
                None,
            );
            self.mix_record_count_in_click(
                &mut output[..frames_before * channels],
                frames_before,
                channels,
                block_start,
                timing,
            );
        }
        self.start_section_record_if_due(timing.boundary);
        let frames_after = frames - frames_before;
        let Some(section) = self
            .active_section
            .filter(|_| !self.presentation_state.is_faulted())
        else {
            output[frames_before * channels..].fill(0.0);
            return true;
        };
        self.render_section_frames(
            &mut output[frames_before * channels..],
            frames_after,
            channels,
            section,
            timing.boundary,
        );
        self.section_advance_override = Some(frames_after as u64);
        true
    }

    pub(super) fn mix_record_count_in_click(
        &self,
        output: &mut [f32],
        frames: usize,
        channels: usize,
        block_start: u64,
        timing: CountInClickTiming,
    ) {
        if channels == 0 || timing.beat_samples == 0 || self.sample_rate == 0 {
            return;
        }
        let click_samples = ((self.sample_rate as f32 * 0.03).round() as u64)
            .max(1)
            .min(timing.beat_samples);
        for frame in 0..frames {
            let sample_position = block_start.saturating_add(frame as u64);
            if sample_position < timing.start || sample_position >= timing.boundary {
                continue;
            }
            let elapsed = sample_position - timing.start;
            let click_position = elapsed % timing.beat_samples;
            if click_position >= click_samples {
                continue;
            }
            let beat = elapsed / timing.beat_samples;
            let accent = beat.is_multiple_of(4);
            let frequency = if accent { 1_760.0 } else { 1_320.0 };
            let amplitude = if accent { 0.32 } else { 0.22 };
            let time = click_position as f32 / self.sample_rate as f32;
            let envelope = 1.0 - click_position as f32 / click_samples as f32;
            let click =
                (std::f32::consts::TAU * frequency * time).cos() * envelope.powi(4) * amplitude;
            for channel in 0..channels {
                output[frame * channels + channel] += click;
            }
        }
    }
}
