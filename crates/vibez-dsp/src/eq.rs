use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static EQ_PARAMS: &[ParamDescriptor] = &[
    // ── LF band ──
    ParamDescriptor {
        name: "LF Gain",
        min: -15.0,
        max: 15.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "LF Freq",
        min: 30.0,
        max: 450.0,
        default: 80.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "LF Bell",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    // ── LMF band ──
    ParamDescriptor {
        name: "LMF Gain",
        min: -15.0,
        max: 15.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "LMF Freq",
        min: 200.0,
        max: 2500.0,
        default: 700.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "LMF Q",
        min: 0.3,
        max: 4.0,
        default: 0.7,
        unit: "",
    },
    // ── HMF band ──
    ParamDescriptor {
        name: "HMF Gain",
        min: -15.0,
        max: 15.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "HMF Freq",
        min: 600.0,
        max: 7000.0,
        default: 2500.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "HMF Q",
        min: 0.3,
        max: 4.0,
        default: 0.7,
        unit: "",
    },
    // ── HF band ──
    ParamDescriptor {
        name: "HF Gain",
        min: -15.0,
        max: 15.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "HF Freq",
        min: 1500.0,
        max: 16000.0,
        default: 8000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "HF Bell",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
];

/// Fixed bell width for the LF/HF bands when switched out of shelf
/// mode, matching the console's fairly broad bell.
const SHELF_BELL_Q: f32 = 0.85;

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

    /// Magnitude of the transfer function at normalized angular
    /// frequency `w` (radians/sample). Used for drawing the response
    /// curve; never called on the audio thread.
    fn magnitude_at(&self, w: f64) -> f64 {
        let (cos_w, sin_w) = (w.cos(), w.sin());
        let (cos_2w, sin_2w) = ((2.0 * w).cos(), (2.0 * w).sin());
        let num_re = self.b0 + self.b1 * cos_w + self.b2 * cos_2w;
        let num_im = -(self.b1 * sin_w + self.b2 * sin_2w);
        let den_re = 1.0 + self.a1 * cos_w + self.a2 * cos_2w;
        let den_im = -(self.a1 * sin_w + self.a2 * sin_2w);
        ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).sqrt()
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

/// SSL E-series style four-band parametric EQ.
///
/// LF and HF are shelves with a bell option; LMF and HMF are fully
/// parametric bells with sweepable Q. Parameter indices follow
/// [`EQ_PARAMS`].
pub struct EqEffect {
    params: [f32; 12],
    sample_rate: f32,
    lf: Biquad,
    lmf: Biquad,
    hmf: Biquad,
    hf: Biquad,
}

impl EqEffect {
    pub fn new(sample_rate: f32) -> Self {
        let mut params = [0.0_f32; 12];
        for (i, d) in EQ_PARAMS.iter().enumerate() {
            params[i] = d.default;
        }
        let mut fx = Self {
            params,
            sample_rate,
            lf: Biquad::identity(),
            lmf: Biquad::identity(),
            hmf: Biquad::identity(),
            hf: Biquad::identity(),
        };
        fx.recompute();
        fx
    }

    /// Build an EQ from a parameter vector without touching audio
    /// state; the UI uses this to evaluate the response curve.
    pub fn from_params(sample_rate: f32, params: &[f32]) -> Self {
        let mut fx = Self::new(sample_rate);
        for (i, v) in params.iter().copied().enumerate().take(12) {
            if let Some(d) = EQ_PARAMS.get(i) {
                fx.params[i] = v.clamp(d.min, d.max);
            }
        }
        fx.recompute();
        fx
    }

    /// Combined magnitude response of all four bands at `freq_hz`,
    /// in dB. Drawing-time helper; not for the audio thread.
    pub fn response_db(&self, freq_hz: f32) -> f32 {
        let w = 2.0 * std::f64::consts::PI * freq_hz as f64 / self.sample_rate as f64;
        let mag = self.lf.magnitude_at(w)
            * self.lmf.magnitude_at(w)
            * self.hmf.magnitude_at(w)
            * self.hf.magnitude_at(w);
        (20.0 * mag.max(1e-9).log10()) as f32
    }

    fn recompute(&mut self) {
        let p = &self.params;
        let sr = self.sample_rate;
        if p[2] >= 0.5 {
            self.lf.set_peaking(p[1], SHELF_BELL_Q, p[0], sr);
        } else {
            self.lf.set_low_shelf(p[1], p[0], sr);
        }
        self.lmf.set_peaking(p[4], p[5], p[3], sr);
        self.hmf.set_peaking(p[7], p[8], p[6], sr);
        if p[11] >= 0.5 {
            self.hf.set_peaking(p[10], SHELF_BELL_Q, p[9], sr);
        } else {
            self.hf.set_high_shelf(p[10], p[9], sr);
        }
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
        let Some(d) = EQ_PARAMS.get(index) else {
            return false;
        };
        self.params[index] = value.clamp(d.min, d.max);
        self.recompute();
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * ch + c;
                let mut s = buffer[idx];
                s = self.lf.process(s, c);
                s = self.lmf.process(s, c);
                s = self.hmf.process(s, c);
                s = self.hf.process(s, c);
                buffer[idx] = s;
            }
        }
    }

    fn reset(&mut self) {
        self.lf.reset();
        self.lmf.reset();
        self.hmf.reset();
        self.hf.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn tone(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| ((i as f32 / sr) * freq * std::f32::consts::TAU).sin() * 0.2)
            .collect()
    }

    #[test]
    fn response_curve_matches_the_settings() {
        // Flat EQ: ~0 dB everywhere.
        let flat = EqEffect::new(48_000.0);
        for f in [30.0, 200.0, 1_000.0, 8_000.0, 18_000.0] {
            assert!(
                flat.response_db(f).abs() < 0.05,
                "flat response at {f} Hz should be ~0 dB"
            );
        }

        // LF shelf boost: +12 dB below the corner, ~0 dB well above.
        let mut params: Vec<f32> = EQ_PARAMS.iter().map(|d| d.default).collect();
        params[0] = 12.0; // LF gain
        params[1] = 120.0; // LF freq
        let boosted = EqEffect::from_params(48_000.0, &params);
        assert!(
            boosted.response_db(40.0) > 10.0,
            "lows should read near +12 dB, got {}",
            boosted.response_db(40.0)
        );
        assert!(
            boosted.response_db(8_000.0).abs() < 0.5,
            "highs should stay flat under an LF shelf, got {}",
            boosted.response_db(8_000.0)
        );

        // HMF bell cut reads back at its center.
        params[0] = 0.0;
        params[6] = -9.0; // HMF gain
        params[7] = 2_000.0; // HMF freq
        params[8] = 1.5; // Q
        let cut = EqEffect::from_params(48_000.0, &params);
        assert!(
            cut.response_db(2_000.0) < -8.0,
            "bell center should read near -9 dB, got {}",
            cut.response_db(2_000.0)
        );
    }

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
    fn lf_shelf_boost_amplifies_lows_only() {
        let mut fx = EqEffect::new(44_100.0);
        fx.set_param(0, 12.0); // LF gain
        fx.set_param(1, 120.0); // LF freq
        let mut low = tone(60.0, 44_100.0, 4_410);
        let in_rms = rms(&low);
        fx.process(&mut low, 1);
        assert!(rms(&low) > in_rms * 1.5, "low should boost");

        fx.reset();
        let mut high = tone(5_000.0, 44_100.0, 4_410);
        let in_rms = rms(&high);
        fx.process(&mut high, 1);
        assert!(
            (rms(&high) / in_rms - 1.0).abs() < 0.15,
            "highs should be untouched by an LF shelf"
        );
    }

    #[test]
    fn hmf_bell_cuts_at_its_center() {
        let mut fx = EqEffect::new(44_100.0);
        fx.set_param(6, -12.0); // HMF gain
        fx.set_param(7, 2_000.0); // HMF freq
        fx.set_param(8, 1.0); // Q
        let mut buf = tone(2_000.0, 44_100.0, 8_820);
        let in_rms = rms(&buf);
        fx.process(&mut buf, 1);
        assert!(rms(&buf) < in_rms * 0.5, "bell cut should attenuate center");
    }

    #[test]
    fn hf_bell_mode_leaves_extreme_highs_alone() {
        // Shelf boosts everything above the corner; a bell at the same
        // spot returns to unity well above its center.
        let sr = 96_000.0;
        let mut shelf = EqEffect::new(sr);
        shelf.set_param(9, 12.0); // HF gain
        shelf.set_param(10, 4_000.0);
        let mut bell = EqEffect::new(sr);
        bell.set_param(9, 12.0);
        bell.set_param(10, 4_000.0);
        bell.set_param(11, 1.0); // bell mode

        let mut a = tone(30_000.0, sr, 9_600);
        let mut b = a.clone();
        shelf.process(&mut a, 1);
        bell.process(&mut b, 1);
        assert!(
            rms(&a) > rms(&b) * 1.3,
            "shelf should lift extreme highs more than bell: shelf {} bell {}",
            rms(&a),
            rms(&b)
        );
    }
}
