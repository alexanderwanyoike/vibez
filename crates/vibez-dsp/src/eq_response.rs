//! Cookbook biquad coefficients shared by EQ audio and its complex response.

use std::f64::consts::TAU;

#[derive(Clone, Copy)]
pub(super) struct Coefficients {
    b: [f64; 3],
    a: [f64; 2],
}

impl Coefficients {
    pub const IDENTITY: Self = Self {
        b: [1.0, 0.0, 0.0],
        a: [0.0; 2],
    };

    fn normalized(b: [f64; 3], a: [f64; 3]) -> Self {
        Self {
            b: b.map(|value| value / a[0]),
            a: [a[1] / a[0], a[2] / a[0]],
        }
    }

    pub fn response(self, omega: f64) -> Complex {
        let (sin, cos) = omega.sin_cos();
        let (sin2, cos2) = (2.0 * omega).sin_cos();
        let nr = self.b[0] + self.b[1] * cos + self.b[2] * cos2;
        let ni = -self.b[1] * sin - self.b[2] * sin2;
        let dr = 1.0 + self.a[0] * cos + self.a[1] * cos2;
        let di = -self.a[0] * sin - self.a[1] * sin2;
        let denominator = dr * dr + di * di;
        Complex(
            (nr * dr + ni * di) / denominator,
            (ni * dr - nr * di) / denominator,
        )
    }
}

#[derive(Clone, Copy)]
pub(super) struct Complex(pub f64, pub f64);

impl Complex {
    pub const ONE: Self = Self(1.0, 0.0);

    pub fn blend(self, other: Self, weight: f32) -> Self {
        Self(
            self.0 + (other.0 - self.0) * weight as f64,
            self.1 + (other.1 - self.1) * weight as f64,
        )
    }

    pub fn times(self, other: Self) -> Self {
        Self(
            self.0 * other.0 - self.1 * other.1,
            self.0 * other.1 + self.1 * other.0,
        )
    }

    pub fn scaled(self, amount: f32) -> Self {
        Self(self.0 * amount as f64, self.1 * amount as f64)
    }

    pub fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0, self.1 + other.1)
    }

    pub fn magnitude(self) -> f64 {
        self.0.hypot(self.1)
    }
}

fn angle(rate: f32, frequency: f32) -> f64 {
    TAU * frequency.clamp((rate * 0.001).min(1.0), rate * 0.49) as f64 / rate as f64
}

pub(super) fn peaking(rate: f32, frequency: f32, q: f32, gain_db: f32) -> Coefficients {
    let (sin, cos) = angle(rate, frequency).sin_cos();
    let a = 10.0_f64.powf(gain_db as f64 / 40.0);
    let alpha = sin / (2.0 * q as f64);
    Coefficients::normalized(
        [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a],
        [1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
    )
}

pub(super) fn shelf(rate: f32, frequency: f32, gain_db: f32, high: bool) -> Coefficients {
    let (sin, cos) = angle(rate, frequency).sin_cos();
    let a = 10.0_f64.powf(gain_db as f64 / 40.0);
    let k = (2.0 * a).sqrt() * sin;
    let sign = if high { 1.0 } else { -1.0 };
    Coefficients::normalized(
        [
            a * (a + 1.0 + sign * (a - 1.0) * cos + k),
            -sign * 2.0 * a * (a - 1.0 + sign * (a + 1.0) * cos),
            a * (a + 1.0 + sign * (a - 1.0) * cos - k),
        ],
        [
            a + 1.0 - sign * (a - 1.0) * cos + k,
            sign * 2.0 * (a - 1.0 - sign * (a + 1.0) * cos),
            a + 1.0 - sign * (a - 1.0) * cos - k,
        ],
    )
}

#[derive(Clone, Copy)]
pub(super) enum Pass {
    Low,
    High,
    Band,
}

pub(super) fn pass(rate: f32, frequency: f32, q: f32, kind: Pass) -> Coefficients {
    let (sin, cos) = angle(rate, frequency).sin_cos();
    let alpha = sin / (2.0 * q as f64);
    let b = match kind {
        Pass::Low => [(1.0 - cos) * 0.5, 1.0 - cos, (1.0 - cos) * 0.5],
        Pass::High => [(1.0 + cos) * 0.5, -(1.0 + cos), (1.0 + cos) * 0.5],
        Pass::Band => [alpha, 0.0, -alpha],
    };
    Coefficients::normalized(b, [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
}

#[derive(Clone, Copy)]
pub(super) struct Biquad {
    pub coefficients: Coefficients,
    state: [[f64; 4]; 2],
}

impl Biquad {
    pub const IDENTITY: Self = Self {
        coefficients: Coefficients::IDENTITY,
        state: [[0.0; 4]; 2],
    };

    pub fn process(&mut self, sample: f32, channel: usize) -> f32 {
        let [x1, x2, y1, y2] = self.state[channel];
        let c = self.coefficients;
        let y = c.b[0] * sample as f64 + c.b[1] * x1 + c.b[2] * x2 - c.a[0] * y1 - c.a[1] * y2;
        self.state[channel] = [sample as f64, x1, y, y1];
        y as f32
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 4]; 2];
    }
}
