//! Processes warm first-order HP then LP exclusively in Delay's routed feedback.

#[derive(Default)]
struct ChannelState {
    high_pass: f64,
    low_pass: f64,
}

#[derive(Default)]
pub(super) struct FeedbackFilters {
    channels: [ChannelState; 2],
    high_pass_g: f64,
    low_pass_g: f64,
}

impl FeedbackFilters {
    pub(super) fn new(sample_rate: f32, high_pass_hz: f32, low_pass_hz: f32) -> Self {
        Self {
            high_pass_g: Self::coefficient(high_pass_hz, sample_rate),
            low_pass_g: Self::coefficient(low_pass_hz, sample_rate),
            ..Default::default()
        }
    }

    fn coefficient(hertz: f32, sample_rate: f32) -> f64 {
        let rate = f64::from(sample_rate);
        let effective = f64::from(hertz).min(0.45 * rate);
        let g = (std::f64::consts::PI * effective / rate).tan();
        g / (1.0 + g)
    }

    pub(super) fn set_high_pass_hz(&mut self, hertz: f32, sample_rate: f32) {
        self.high_pass_g = Self::coefficient(hertz, sample_rate);
    }

    pub(super) fn set_low_pass_hz(&mut self, hertz: f32, sample_rate: f32) {
        self.low_pass_g = Self::coefficient(hertz, sample_rate);
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
        if sample.is_finite() { sample } else { 0.0 }
    }

    pub(super) fn process(&mut self, input: [f32; 2], high_pass: bool, low_pass: bool) -> [f32; 2] {
        std::array::from_fn(|channel| {
            let state = &mut self.channels[channel];
            let high_low = Self::low_pass(input[channel], self.high_pass_g, &mut state.high_pass);
            let after_high = if high_pass {
                Self::finite_sample(f64::from(input[channel]) - high_low)
            } else {
                input[channel]
            };
            let low = Self::low_pass(after_high, self.low_pass_g, &mut state.low_pass);
            if low_pass { Self::finite_sample(low) } else { after_high }
        })
    }

    pub(super) fn reset(&mut self) {
        self.channels = Default::default();
    }
}
