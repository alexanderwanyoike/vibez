//! Owns warm feedback HP/LP processing and independent cutoff/enable trajectories.

use super::ramp::ScalarRamp;

#[derive(Default)]
struct ChannelState {
    high_pass: f64,
    low_pass: f64,
}

#[derive(Default)]
struct Cutoff {
    current: f64,
    origin: f64,
    target: f64,
    g: f64,
    rate: f64,
    frame: usize,
    frames: usize,
    started: bool,
}

impl Cutoff {
    fn new(hertz: f32, sample_rate: f32) -> Self {
        let mut cutoff = Self {
            rate: f64::from(sample_rate),
            frames: (sample_rate as usize).div_ceil(100),
            ..Default::default()
        };
        cutoff.snap(f64::from(hertz).min(0.45 * cutoff.rate));
        cutoff
    }

    fn apply(&mut self, hertz: f64) {
        if self.current != hertz {
            // f64 preserves low-rate clamp precision; this guard avoids a static tan cost.
            self.current = hertz;
            let g = (std::f64::consts::PI * hertz / self.rate).tan();
            self.g = g / (1.0 + g);
        }
    }

    fn snap(&mut self, target: f64) {
        self.apply(target);
        self.origin = target;
        self.target = target;
        self.frame = self.frames;
    }

    fn edit(&mut self, hertz: f32, automated: bool) {
        let target = f64::from(hertz).min(0.45 * self.rate);
        if !self.started || automated {
            self.snap(target);
        } else if target != self.target {
            self.origin = self.current;
            self.target = target;
            self.frame = 0;
        }
    }

    fn next_frame(&mut self) -> f64 {
        self.started = true;
        if self.frame < self.frames {
            self.frame += 1;
            let hertz = if self.frame == self.frames {
                self.target
            } else {
                self.origin + (self.target - self.origin) * self.frame as f64 / self.frames as f64
            };
            self.apply(hertz);
        }
        self.g
    }

    fn reset(&mut self) {
        self.snap(self.target);
        self.started = false;
    }
}

pub(crate) struct FeedbackFilters {
    channels: [ChannelState; 2],
    high_pass_enabled: ScalarRamp,
    low_pass_enabled: ScalarRamp,
    high_pass_cutoff: Cutoff,
    low_pass_cutoff: Cutoff,
}

impl Default for FeedbackFilters {
    fn default() -> Self {
        Self {
            channels: Default::default(),
            high_pass_enabled: ScalarRamp::new(0.0),
            low_pass_enabled: ScalarRamp::new(0.0),
            high_pass_cutoff: Cutoff::default(),
            low_pass_cutoff: Cutoff::default(),
        }
    }
}

impl FeedbackFilters {
    pub(crate) fn new(sample_rate: f32, high_pass_hz: f32, low_pass_hz: f32) -> Self {
        Self {
            high_pass_cutoff: Cutoff::new(high_pass_hz, sample_rate),
            low_pass_cutoff: Cutoff::new(low_pass_hz, sample_rate),
            ..Default::default()
        }
    }

    pub(crate) fn set_high_pass_enabled(&mut self, target: f32, automated: bool, frames: usize) {
        self.high_pass_enabled.edit(target, automated, frames);
    }

    pub(crate) fn set_low_pass_enabled(&mut self, target: f32, automated: bool, frames: usize) {
        self.low_pass_enabled.edit(target, automated, frames);
    }

    pub(crate) fn set_high_pass_hz(&mut self, hertz: f32, automated: bool) {
        self.high_pass_cutoff.edit(hertz, automated);
    }

    pub(crate) fn set_low_pass_hz(&mut self, hertz: f32, automated: bool) {
        self.low_pass_cutoff.edit(hertz, automated);
    }

    fn low_pass(input: f32, g: f64, state: &mut f64) -> f64 {
        let v = g * (f64::from(input) - *state);
        let low = *state + v;
        *state = low + v;
        low
    }

    fn finite_sample(value: f64) -> f32 {
        let sample = value as f32;
        // Exceptional bridge overflow must not poison the next filter's persistent state.
        if sample.is_finite() {
            sample
        } else {
            0.0
        }
    }

    fn blend(input: f32, filtered: f32, amount: f32) -> f32 {
        if amount == 0.0 {
            input
        } else if amount == 1.0 {
            filtered
        } else {
            Self::finite_sample(
                (1.0 - f64::from(amount)) * f64::from(input)
                    + f64::from(amount) * f64::from(filtered),
            )
        }
    }

    pub(crate) fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let high_pass = self.high_pass_enabled.next_frame();
        let low_pass = self.low_pass_enabled.next_frame();
        let high_pass_g = self.high_pass_cutoff.next_frame();
        let low_pass_g = self.low_pass_cutoff.next_frame();
        std::array::from_fn(|channel| {
            let state = &mut self.channels[channel];
            let high_low = Self::low_pass(input[channel], high_pass_g, &mut state.high_pass);
            let high = Self::finite_sample(f64::from(input[channel]) - high_low);
            let after_high = Self::blend(input[channel], high, high_pass);
            let low = Self::low_pass(after_high, low_pass_g, &mut state.low_pass);
            Self::blend(after_high, Self::finite_sample(low), low_pass)
        })
    }

    pub(crate) fn reset(&mut self, high_pass: f32, low_pass: f32) {
        self.channels = Default::default();
        self.high_pass_enabled.reset(high_pass);
        self.low_pass_enabled.reset(low_pass);
        self.high_pass_cutoff.reset();
        self.low_pass_cutoff.reset();
    }
}
