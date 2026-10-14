//! Observation drains retain real sample chronology without advancing the acquisition clock.

use vibez_core::effect::SampleHoldMeter;

#[derive(Default)]
pub(super) struct Scope {
    samples: SampleHoldMeter,
    frames: u64,
    count: u16,
}

impl Scope {
    pub(super) fn record(&mut self, input: f32, output: f32, acquired: bool, interval: u32) {
        let position = self.frames as usize % 128;
        self.samples.input[position] = input;
        self.samples.output[position] = output;
        let mask = &mut self.samples.primary_acquisitions[position / 64];
        let bit = 1_u64 << (position % 64);
        *mask = (*mask & !bit) | if acquired { bit } else { 0 };
        self.samples.primary_interval_frames = interval;
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
            ..SampleHoldMeter::default()
        };
        for point in 0..usize::from(self.count) {
            let position = first.wrapping_add(point as u64) as usize % 128;
            meter.input[point] = self.samples.input[position];
            meter.output[point] = self.samples.output[position];
            let acquired =
                self.samples.primary_acquisitions[position / 64] & (1_u64 << (position % 64)) != 0;
            if acquired {
                meter.primary_acquisitions[point / 64] |= 1_u64 << (point % 64);
            }
        }
        self.count = 0;
        meter
    }
}
