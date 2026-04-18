use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static FILTER_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Cutoff",
        min: 20.0,
        max: 20000.0,
        default: 1000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Resonance",
        min: 0.1,
        max: 10.0,
        default: 0.707,
        unit: "Q",
    },
];

/// Simple biquad low-pass filter (stereo).
pub struct FilterEffect {
    cutoff: f32,
    resonance: f32,
    sample_rate: f32,
    // Biquad coefficients
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    // Per-channel state (stereo)
    x1: [f64; 2],
    x2: [f64; 2],
    y1: [f64; 2],
    y2: [f64; 2],
}

impl FilterEffect {
    pub fn new(sample_rate: f32) -> Self {
        let mut f = Self {
            cutoff: 1000.0,
            resonance: 0.707,
            sample_rate,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: [0.0; 2],
            x2: [0.0; 2],
            y1: [0.0; 2],
            y2: [0.0; 2],
        };
        f.compute_coefficients();
        f
    }

    fn compute_coefficients(&mut self) {
        let omega = 2.0 * std::f64::consts::PI * self.cutoff as f64 / self.sample_rate as f64;
        let sin_w = omega.sin();
        let cos_w = omega.cos();
        let alpha = sin_w / (2.0 * self.resonance as f64);

        let b0 = (1.0 - cos_w) / 2.0;
        let b1 = 1.0 - cos_w;
        let b2 = (1.0 - cos_w) / 2.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cos_w;
        let a2 = 1.0 - alpha;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
}

impl AudioEffect for FilterEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Filter
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        FILTER_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.cutoff = value.clamp(20.0, 20000.0);
                self.compute_coefficients();
                true
            }
            1 => {
                self.resonance = value.clamp(0.1, 10.0);
                self.compute_coefficients();
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.cutoff,
            1 => self.resonance,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;

        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * ch + c;
                let x0 = buffer[idx] as f64;
                let y0 = self.b0 * x0 + self.b1 * self.x1[c] + self.b2 * self.x2[c]
                    - self.a1 * self.y1[c]
                    - self.a2 * self.y2[c];
                self.x2[c] = self.x1[c];
                self.x1[c] = x0;
                self.y2[c] = self.y1[c];
                self.y1[c] = y0;
                buffer[idx] = y0 as f32;
            }
        }
    }

    fn reset(&mut self) {
        self.x1 = [0.0; 2];
        self.x2 = [0.0; 2];
        self.y1 = [0.0; 2];
        self.y2 = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_passes_dc() {
        let mut fx = FilterEffect::new(44100.0);
        // DC signal should pass through a low-pass filter
        let mut buf = vec![1.0_f32; 200];
        fx.process(&mut buf, 1);
        // After settling, output should be close to 1.0
        assert!((buf[199] - 1.0).abs() < 0.01);
    }

    #[test]
    fn filter_attenuates_nyquist() {
        let mut fx = FilterEffect::new(44100.0);
        fx.set_param(0, 200.0); // very low cutoff
                                // Nyquist-ish signal (alternating +1/-1)
        let mut buf: Vec<f32> = (0..200)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        fx.process(&mut buf, 1);
        // Last samples should be heavily attenuated
        assert!(buf[199].abs() < 0.1);
    }

    #[test]
    fn filter_reset_clears_state() {
        let mut fx = FilterEffect::new(44100.0);
        let mut buf = vec![1.0; 100];
        fx.process(&mut buf, 1);
        fx.reset();
        assert_eq!(fx.x1, [0.0; 2]);
        assert_eq!(fx.y1, [0.0; 2]);
    }

    #[test]
    fn filter_stereo() {
        let mut fx = FilterEffect::new(44100.0);
        // interleaved stereo: L=1.0, R=0.5 repeated
        let mut buf: Vec<f32> = (0..200)
            .map(|i| if i % 2 == 0 { 1.0 } else { 0.5 })
            .collect();
        fx.process(&mut buf, 2);
        // After settling, L~1.0, R~0.5
        let last_l = buf[198];
        let last_r = buf[199];
        assert!((last_l - 1.0).abs() < 0.05);
        assert!((last_r - 0.5).abs() < 0.05);
    }

    #[test]
    fn filter_param_access() {
        let mut fx = FilterEffect::new(44100.0);
        assert!((fx.get_param(0) - 1000.0).abs() < 1e-3);
        assert!((fx.get_param(1) - 0.707).abs() < 1e-3);
        fx.set_param(0, 5000.0);
        assert!((fx.get_param(0) - 5000.0).abs() < 1e-3);
        assert!(!fx.set_param(2, 1.0));
    }
}
