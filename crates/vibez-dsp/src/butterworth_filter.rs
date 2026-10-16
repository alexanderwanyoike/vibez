//! Second-order Butterworth filtering with persistent stereo histories.
//!
//! The coefficients are the low- and high-pass biquads of Robert
//! Bristow-Johnson's Audio EQ Cookbook at Q = 1/sqrt(2).

#[derive(Clone, Copy)]
pub(crate) struct ButterworthFilter {
    b: [f64; 3],
    a: [f64; 2],
    state: [[f64; 4]; 2],
}

impl ButterworthFilter {
    pub const EMPTY: Self = Self {
        b: [1.0, 0.0, 0.0],
        a: [0.0; 2],
        state: [[0.0; 4]; 2],
    };

    pub fn configure(&mut self, rate: f32, frequency: f32, high: bool) {
        let frequency = frequency.clamp((rate * 0.001).min(1.0), rate * 0.49);
        let (sin, cos) = (std::f64::consts::TAU * frequency as f64 / rate as f64).sin_cos();
        let alpha = sin / std::f64::consts::SQRT_2;
        let value = if high {
            (1.0 + cos) * 0.5
        } else {
            (1.0 - cos) * 0.5
        };
        self.b = [value, if high { -2.0 * value } else { 2.0 * value }, value]
            .map(|v| v / (1.0 + alpha));
        self.a = [-2.0 * cos / (1.0 + alpha), (1.0 - alpha) / (1.0 + alpha)];
    }

    pub fn process(&mut self, input: f32, channel: usize) -> f32 {
        let [x1, x2, y1, y2] = self.state[channel];
        let output = self.b[0] * input as f64 + self.b[1] * x1 + self.b[2] * x2
            - self.a[0] * y1
            - self.a[1] * y2;
        self.state[channel] = [input as f64, x1, output, y1];
        output as f32
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 4]; 2];
    }
}
