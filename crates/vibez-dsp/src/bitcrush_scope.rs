//! Observation drains retain real sample chronology without advancing the acquisition clock.

use super::lanes::Frame;
use vibez_core::effect::SampleHoldMeter;

#[derive(Default)]
pub(super) struct Scope {
    samples: SampleHoldMeter,
    frames: u64,
    count: u16,
}

impl Scope {
    pub(super) fn record(&mut self, input: f32, output: f32, frame: &Frame) {
        let position = self.frames as usize % 128;
        self.samples.input[position] = input;
        self.samples.output[position] = output;
        let bit = 1_u64 << (position % 64);
        for (mask, present) in [
            (
                &mut self.samples.primary_acquisitions,
                frame.primary_acquired,
            ),
            (
                &mut self.samples.secondary_acquisitions,
                frame.secondary_acquired,
            ),
            (&mut self.samples.blending, frame.transition),
        ] {
            mask[position / 64] = (mask[position / 64] & !bit) | if present { bit } else { 0 };
        }
        self.samples.primary_interval_frames = frame.primary_interval;
        self.samples.secondary_interval_frames = frame.secondary_interval;
        self.samples.transition_active = frame.transition_active;
        self.frames = self.frames.wrapping_add(1);
        self.count = (self.count + 1).min(128);
    }

    pub(super) fn take(&mut self, sample_rate: f32) -> SampleHoldMeter {
        let first = self.frames.wrapping_sub(u64::from(self.count));
        let mut meter = SampleHoldMeter {
            count: self.count,
            sample_rate,
            first_source_frame: first,
            primary_interval_frames: self.samples.primary_interval_frames,
            secondary_interval_frames: self.samples.secondary_interval_frames,
            transition_active: self.samples.transition_active,
            ..SampleHoldMeter::default()
        };
        for point in 0..usize::from(self.count) {
            let position = first.wrapping_add(point as u64) as usize % 128;
            meter.input[point] = self.samples.input[position];
            meter.output[point] = self.samples.output[position];
            for (source, destination) in [
                (
                    &self.samples.primary_acquisitions,
                    &mut meter.primary_acquisitions,
                ),
                (
                    &self.samples.secondary_acquisitions,
                    &mut meter.secondary_acquisitions,
                ),
                (&self.samples.blending, &mut meter.blending),
            ] {
                if source[position / 64] & (1_u64 << (position % 64)) != 0 {
                    destination[point / 64] |= 1_u64 << (point % 64);
                }
            }
        }
        self.count = 0;
        meter
    }
}
