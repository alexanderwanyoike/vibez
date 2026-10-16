//! A resonant state-variable filter, Andrew Simper's (Cytomic) trapezoidal
//! SVF from "Linear Trapezoidal Integrated SVF" (2013), which stays stable
//! and in tune as its cutoff moves every sample.

/// Coefficients for a cutoff and Q at a sample rate.
#[derive(Clone, Copy, Debug)]
pub struct Coefficients {
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Coefficients {
    pub fn new(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let g =
            (std::f32::consts::PI * cutoff.min(sample_rate * 0.45).max(0.001) / sample_rate).tan();
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { a1, a2, a3: g * a2 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

/// Zero for a value too small to hear, so a silent filter stops instead of
/// crawling through slow denormal numbers.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-15 {
        0.0
    } else {
        value
    }
}

impl Svf {
    /// The low-pass output for one input sample.
    pub fn low(&mut self, input: f32, c: Coefficients) -> f32 {
        let v3 = input - self.ic2;
        let band = c.a1 * self.ic1 + c.a2 * v3;
        let low = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * band - self.ic1);
        self.ic2 = flush(2.0 * low - self.ic2);
        low
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
