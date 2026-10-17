//! Four-band console EQ with measured response, smooth controls and band audition.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

#[path = "eq_response.rs"]
mod response;
use response::{Biquad, Complex, Pass};

pub mod param {
    pub const GAIN: [usize; 4] = [0, 3, 6, 9];
    pub const FREQUENCY: [usize; 4] = [1, 4, 7, 10];
    pub const SHAPE: [usize; 4] = [2, 5, 8, 11];
    pub const ENABLE: [usize; 4] = [12, 13, 14, 15];
    pub const AUDITION: usize = 16;
    pub const HP_ENABLED: usize = 17;
    pub const HP_FREQUENCY: usize = 18;
    pub const LP_ENABLED: usize = 19;
    pub const LP_FREQUENCY: usize = 20;
    pub const OUTPUT: usize = 21;
    pub const COUNT: usize = 22;
}

pub static EQ_PARAMS: &[ParamDescriptor] = &[
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
    ParamDescriptor {
        name: "LF Enabled",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "LMF Enabled",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "HMF Enabled",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "HF Enabled",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Audition",
        min: 0.0,
        max: 4.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "HP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "HP Frequency",
        min: 20.0,
        max: 2000.0,
        default: 20.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "LP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "LP Frequency",
        min: 1000.0,
        max: 20000.0,
        default: 20000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Output",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
    },
];

const RAMP_FRAMES: u32 = 512;
const SHELF_BELL_Q: f32 = 0.85;
const CUT_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

pub struct EqEffect {
    params: [f32; param::COUNT],
    applied: [f32; param::COUNT],
    steps: [f32; param::COUNT],
    remaining: [u32; param::COUNT],
    audition: [f32; 5],
    audition_steps: [f32; 5],
    audition_remaining: u32,
    sample_rate: f32,
    output_gain: f32,
    bells: [Biquad; 4],
    shelves: [Biquad; 2],
    audition_bells: [Biquad; 4],
    audition_shelves: [Biquad; 2],
    cuts: [Biquad; 2],
    coefficient_clock: u32,
    started: bool,
}

impl EqEffect {
    pub fn new(sample_rate: f32) -> Self {
        let params = std::array::from_fn(|index| EQ_PARAMS[index].default);
        let mut effect = Self {
            params,
            applied: params,
            steps: [0.0; param::COUNT],
            remaining: [0; param::COUNT],
            audition: [1.0, 0.0, 0.0, 0.0, 0.0],
            audition_steps: [0.0; 5],
            audition_remaining: 0,
            sample_rate: if sample_rate.is_finite() && sample_rate >= 2.0 {
                sample_rate
            } else {
                48_000.0
            },
            output_gain: 1.0,
            bells: [Biquad::IDENTITY; 4],
            shelves: [Biquad::IDENTITY; 2],
            audition_bells: [Biquad::IDENTITY; 4],
            audition_shelves: [Biquad::IDENTITY; 2],
            cuts: [Biquad::IDENTITY; 2],
            coefficient_clock: 0,
            started: false,
        };
        effect.recompute();
        effect
    }

    pub fn from_params(sample_rate: f32, params: &[f32]) -> Self {
        let mut effect = Self::new(sample_rate);
        for (index, value) in params.iter().copied().enumerate().take(param::COUNT) {
            effect.set_param(index, value);
        }
        effect
    }

    fn applied_target(&self, index: usize) -> f32 {
        if [2, 11, 12, 13, 14, 15, 17, 19].contains(&index) {
            if self.params[index] >= 0.5 {
                1.0
            } else {
                0.0
            }
        } else {
            self.params[index]
        }
    }

    fn set_target(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = EQ_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = value.clamp(descriptor.min, descriptor.max);
        let value = if index == param::AUDITION {
            value.round()
        } else {
            value
        };
        if self.params[index] == value && !automated {
            return true;
        }
        self.params[index] = value;
        let snap = automated || !self.started;
        if index == param::AUDITION {
            for choice in 0..5 {
                let target = if choice == value as usize { 1.0 } else { 0.0 };
                if snap {
                    self.audition[choice] = target;
                }
                self.audition_steps[choice] = (target - self.audition[choice]) / RAMP_FRAMES as f32;
            }
            self.audition_remaining = if snap { 0 } else { RAMP_FRAMES };
        } else {
            let target = self.applied_target(index);
            if snap {
                self.applied[index] = target;
            }
            self.steps[index] = (target - self.applied[index]) / RAMP_FRAMES as f32;
            self.remaining[index] = if snap { 0 } else { RAMP_FRAMES };
        }
        if snap && index == param::OUTPUT {
            self.output_gain = vibez_core::level::db_to_amplitude(self.applied[param::OUTPUT]);
        }
        self.recompute();
        true
    }

    fn recompute(&mut self) {
        let p = &self.applied;
        let rate = self.sample_rate;
        for band in 0..4 {
            let frequency = p[param::FREQUENCY[band]];
            let q = if band == 0 || band == 3 {
                SHELF_BELL_Q
            } else {
                p[param::SHAPE[band]]
            };
            self.bells[band].coefficients =
                response::peaking(rate, frequency, q, p[param::GAIN[band]]);
            self.audition_bells[band].coefficients = response::pass(rate, frequency, q, Pass::Band);
        }
        for (shelf, band) in [0, 3].into_iter().enumerate() {
            self.shelves[shelf].coefficients = response::shelf(
                rate,
                p[param::FREQUENCY[band]],
                p[param::GAIN[band]],
                band == 3,
            );
            self.audition_shelves[shelf].coefficients = response::pass(
                rate,
                p[param::FREQUENCY[band]],
                CUT_Q,
                if band == 3 { Pass::High } else { Pass::Low },
            );
        }
        self.cuts[0].coefficients = response::pass(rate, p[param::HP_FREQUENCY], CUT_Q, Pass::High);
        self.cuts[1].coefficients = response::pass(rate, p[param::LP_FREQUENCY], CUT_Q, Pass::Low);
    }

    fn advance(&mut self) {
        let output_ramping = self.remaining[param::OUTPUT] > 0;
        let mut coefficients_changed = false;
        let mut coefficient_ramp_ended = false;
        for index in 0..param::COUNT {
            if self.remaining[index] == 0 {
                continue;
            }
            self.remaining[index] -= 1;
            self.applied[index] = if self.remaining[index] == 0 {
                self.applied_target(index)
            } else {
                self.applied[index] + self.steps[index]
            };
            if param::GAIN.contains(&index)
                || param::FREQUENCY.contains(&index)
                || [5, 8, param::HP_FREQUENCY, param::LP_FREQUENCY].contains(&index)
            {
                coefficients_changed = true;
                coefficient_ramp_ended |= self.remaining[index] == 0;
            }
        }
        if output_ramping {
            self.output_gain = vibez_core::level::db_to_amplitude(self.applied[param::OUTPUT]);
        }
        if self.audition_remaining > 0 {
            self.audition_remaining -= 1;
            for choice in 0..5 {
                self.audition[choice] = if self.audition_remaining == 0 {
                    if choice == self.params[param::AUDITION] as usize {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    self.audition[choice] + self.audition_steps[choice]
                };
            }
        }
        // A fixed frame clock makes coefficient work independent of callback
        // partitioning, while final and automated values still land exactly.
        if coefficients_changed && (self.coefficient_clock == 0 || coefficient_ramp_ended) {
            self.recompute();
        }
        self.coefficient_clock = (self.coefficient_clock + 1) % 8;
    }

    pub fn response_db(&self, frequency: f32) -> f32 {
        if !frequency.is_finite() {
            return 0.0;
        }
        let omega = std::f64::consts::TAU * frequency.clamp(0.0, self.sample_rate * 0.5) as f64
            / self.sample_rate as f64;
        let mut combined = Complex::ONE;
        for band in 0..4 {
            let bell = self.bells[band].coefficients.response(omega);
            let band_response = if band == 0 || band == 3 {
                let shelf = usize::from(band == 3);
                self.shelves[shelf]
                    .coefficients
                    .response(omega)
                    .blend(bell, self.applied[param::SHAPE[band]])
            } else {
                bell
            };
            combined = combined
                .times(Complex::ONE.blend(band_response, self.applied[param::ENABLE[band]]));
        }
        for (cut, enabled) in [param::HP_ENABLED, param::LP_ENABLED]
            .into_iter()
            .enumerate()
        {
            combined = combined.times(Complex::ONE.blend(
                self.cuts[cut].coefficients.response(omega),
                self.applied[enabled],
            ));
        }
        let mut output = combined.scaled(self.audition[0]);
        for band in 0..4 {
            let bell = self.audition_bells[band].coefficients.response(omega);
            let band_response = if band == 0 || band == 3 {
                self.audition_shelves[usize::from(band == 3)]
                    .coefficients
                    .response(omega)
                    .blend(bell, self.applied[param::SHAPE[band]])
            } else {
                bell
            };
            output = output.plus(band_response.scaled(self.audition[band + 1]));
        }
        (20.0 * output.magnitude().max(1e-12).log10()) as f32 + self.applied[param::OUTPUT]
    }

    fn process_channel(&mut self, input: f32, channel: usize) -> f32 {
        let mut normal = input;
        for band in 0..4 {
            let bell = self.bells[band].process(normal, channel);
            let wet = if band == 0 || band == 3 {
                let shelf = self.shelves[usize::from(band == 3)].process(normal, channel);
                shelf + (bell - shelf) * self.applied[param::SHAPE[band]]
            } else {
                bell
            };
            normal += (wet - normal) * self.applied[param::ENABLE[band]];
        }
        for (cut, enabled) in [param::HP_ENABLED, param::LP_ENABLED]
            .into_iter()
            .enumerate()
        {
            let wet = self.cuts[cut].process(normal, channel);
            normal += (wet - normal) * self.applied[enabled];
        }
        let mut output = normal * self.audition[0];
        // Inactive paths keep their history warm so enabling or auditioning
        // starts from the actual signal rather than an empty filter state.
        for band in 0..4 {
            let bell = self.audition_bells[band].process(input, channel);
            let wet = if band == 0 || band == 3 {
                let shelf = self.audition_shelves[usize::from(band == 3)].process(input, channel);
                shelf + (bell - shelf) * self.applied[param::SHAPE[band]]
            } else {
                bell
            };
            output += wet * self.audition[band + 1];
        }
        output * self.output_gain
    }
}

impl AudioEffect for EqEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Eq
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        EQ_PARAMS
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, false)
    }
    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, true)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        for frame in buffer.chunks_exact_mut(channels) {
            self.started = true;
            self.advance();
            for (channel, sample) in frame.iter_mut().take(2).enumerate() {
                *sample = self.process_channel(*sample, channel);
            }
        }
    }

    fn reset(&mut self) {
        for filter in self
            .bells
            .iter_mut()
            .chain(&mut self.shelves)
            .chain(&mut self.audition_bells)
            .chain(&mut self.audition_shelves)
            .chain(&mut self.cuts)
        {
            filter.reset();
        }
        self.applied = std::array::from_fn(|index| self.applied_target(index));
        self.output_gain = vibez_core::level::db_to_amplitude(self.applied[param::OUTPUT]);
        self.remaining = [0; param::COUNT];
        self.audition = std::array::from_fn(|choice| {
            if choice == self.params[param::AUDITION] as usize {
                1.0
            } else {
                0.0
            }
        });
        self.audition_remaining = 0;
        self.coefficient_clock = 0;
        self.started = false;
        self.recompute();
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

#[cfg(test)]
#[path = "eq_device_tests.rs"]
mod device_tests;
