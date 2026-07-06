use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static EQ_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Low",
        min: -18.0,
        max: 18.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Mid Freq",
        min: 200.0,
        max: 4000.0,
        default: 1000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Mid",
        min: -18.0,
        max: 18.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "High",
        min: -18.0,
        max: 18.0,
        default: 0.0,
        unit: "dB",
    },
];

const LOW_SHELF_HZ: f32 = 150.0;
const HIGH_SHELF_HZ: f32 = 4_000.0;
const MID_Q: f32 = 0.707;

#[derive(Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x: [f64; 2],
    y: [f64; 2],
    z: [[f64; 2]; 2], // [channel][delay]: (x1,x2) and (y1,y2) fused per channel
}

impl Biquad {
    fn identity() -> Self {
        Self {
            b0: 1.0,
            ..Self::default()
        }
    }

    fn set_low_shelf(&mut self, f_hz: f32, gain_db: f32, sr: f32) {
        let omega = 2.0 * std::f64::consts::PI * f_hz as f64 / sr as f64;
        let a = 10.0_f64.powf(gain_db as f64 / 40.0);
        let cos_w = omega.cos();
        let sin_w = omega.sin();
        let s = 1.0;
        let alpha = sin_w / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha);
        let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w);
        let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha);
        let a0 = (a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha;
        let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w);
        let a2 = (a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn set_high_shelf(&mut self, f_hz: f32, gain_db: f32, sr: f32) {
        let omega = 2.0 * std::f64::consts::PI * f_hz as f64 / sr as f64;
        let a = 10.0_f64.powf(gain_db as f64 / 40.0);
        let cos_w = omega.cos();
        let sin_w = omega.sin();
        let s = 1.0;
        let alpha = sin_w / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;

        let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w + two_sqrt_a_alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w);
        let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w - two_sqrt_a_alpha);
        let a0 = (a + 1.0) - (a - 1.0) * cos_w + two_sqrt_a_alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w);
        let a2 = (a + 1.0) - (a - 1.0) * cos_w - two_sqrt_a_alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn set_peaking(&mut self, f_hz: f32, q: f32, gain_db: f32, sr: f32) {
        let omega = 2.0 * std::f64::consts::PI * f_hz as f64 / sr as f64;
        let a = 10.0_f64.powf(gain_db as f64 / 40.0);
        let cos_w = omega.cos();
        let sin_w = omega.sin();
        let alpha = sin_w / (2.0 * q as f64);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha / a;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    fn process(&mut self, sample: f32, channel: usize) -> f32 {
        let x0 = sample as f64;
        let z = &mut self.z[channel];
        let y0 = self.b0 * x0 + self.b1 * self.x[channel] + self.b2 * z[0]
            - self.a1 * self.y[channel]
            - self.a2 * z[1];
        z[0] = self.x[channel];
        z[1] = self.y[channel];
        self.x[channel] = x0;
        self.y[channel] = y0;
        y0 as f32
    }

    fn reset(&mut self) {
        self.x = [0.0; 2];
        self.y = [0.0; 2];
        self.z = [[0.0; 2]; 2];
    }
}

/// 3-band EQ: low shelf @ 150Hz, peaking mid (sweepable), high shelf @ 4kHz.
pub struct EqEffect {
    low_db: f32,
    mid_hz: f32,
    mid_db: f32,
    high_db: f32,
    sample_rate: f32,
    low: Biquad,
    mid: Biquad,
    high: Biquad,
}

impl EqEffect {
    pub fn new(sample_rate: f32) -> Self {
        let mut fx = Self {
            low_db: 0.0,
            mid_hz: 1000.0,
            mid_db: 0.0,
            high_db: 0.0,
            sample_rate,
            low: Biquad::identity(),
            mid: Biquad::identity(),
            high: Biquad::identity(),
        };
        fx.recompute();
        fx
    }

    fn recompute(&mut self) {
        self.low
            .set_low_shelf(LOW_SHELF_HZ, self.low_db, self.sample_rate);
        self.mid
            .set_peaking(self.mid_hz, MID_Q, self.mid_db, self.sample_rate);
        self.high
            .set_high_shelf(HIGH_SHELF_HZ, self.high_db, self.sample_rate);
    }
}

impl AudioEffect for EqEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Eq
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        EQ_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.low_db = value.clamp(-18.0, 18.0);
                self.recompute();
                true
            }
            1 => {
                self.mid_hz = value.clamp(200.0, 4000.0);
                self.recompute();
                true
            }
            2 => {
                self.mid_db = value.clamp(-18.0, 18.0);
                self.recompute();
                true
            }
            3 => {
                self.high_db = value.clamp(-18.0, 18.0);
                self.recompute();
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.low_db,
            1 => self.mid_hz,
            2 => self.mid_db,
            3 => self.high_db,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * ch + c;
                let mut s = buffer[idx];
                s = self.low.process(s, c);
                s = self.mid.process(s, c);
                s = self.high.process(s, c);
                buffer[idx] = s;
            }
        }
    }

    fn reset(&mut self) {
        self.low.reset();
        self.mid.reset();
        self.high.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_eq_approximately_passthrough() {
        let mut fx = EqEffect::new(44_100.0);
        let orig: Vec<f32> = (0..512).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        let mut buf = orig.clone();
        fx.process(&mut buf, 1);
        let err: f32 = orig
            .iter()
            .zip(buf.iter())
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / orig.len() as f32;
        assert!(err < 0.01, "err {err}");
    }

    #[test]
    fn low_boost_amplifies_low_frequencies() {
        let mut fx = EqEffect::new(44_100.0);
        fx.set_param(0, 12.0);
        // 60 Hz tone
        let mut buf: Vec<f32> = (0..4_410)
            .map(|i| ((i as f32 / 44_100.0) * 60.0 * std::f32::consts::TAU).sin() * 0.2)
            .collect();
        let in_rms: f32 = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        fx.process(&mut buf, 1);
        let out_rms: f32 = (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt();
        assert!(out_rms > in_rms * 1.5, "in {in_rms} out {out_rms}");
    }
}
