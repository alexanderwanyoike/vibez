//! A resonant state-variable filter, Andrew Simper's (Cytomic) trapezoidal
//! SVF from "Linear Trapezoidal Integrated SVF" (2013), which stays stable
//! and in tune as its cutoff moves every sample.

/// Coefficients for a cutoff and Q at a sample rate.
#[derive(Clone, Copy, Debug)]
pub struct Coefficients {
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Coefficients {
    pub fn new(sample_rate: f32, cutoff: f32, q: f32) -> Self {
        let g =
            (std::f32::consts::PI * cutoff.min(sample_rate * 0.45).max(0.001) / sample_rate).tan();
        Self::from_g(g, q)
    }

    fn from_g(g: f32, q: f32) -> Self {
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self {
            g,
            k,
            a1,
            a2,
            a3: g * a2,
        }
    }

    /// The same cutoff with another Q.
    pub fn with_q(self, q: f32) -> Self {
        Self::from_g(self.g, q)
    }
}

/// The filter's outputs for one input sample; the band-pass is scaled to
/// unity at its centre.
#[derive(Clone, Copy, Debug)]
pub struct Outputs {
    pub low: f32,
    pub band: f32,
    pub high: f32,
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
    pub fn process(&mut self, input: f32, c: Coefficients) -> Outputs {
        let v3 = input - self.ic2;
        let band = c.a1 * self.ic1 + c.a2 * v3;
        let low = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * band - self.ic1);
        self.ic2 = flush(2.0 * low - self.ic2);
        Outputs {
            low,
            band: c.k * band,
            high: input - c.k * band - low,
        }
    }
}
