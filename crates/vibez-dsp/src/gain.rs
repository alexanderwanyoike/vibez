//! Gain processing with independently adjustable stereo balance and polarity.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static GAIN_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Gain",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        unit: "x",
    },
    ParamDescriptor {
        name: "L Trim",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "R Trim",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "L Polarity",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "R Polarity",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
];

pub struct GainEffect {
    gain: f32,
    trim_db: [f32; 2],
    inverted: [bool; 2],
    channel_gains: [f32; 2],
}

impl Default for GainEffect {
    fn default() -> Self {
        Self {
            gain: 1.0,
            trim_db: [0.0; 2],
            inverted: [false; 2],
            channel_gains: [1.0; 2],
        }
    }
}

impl GainEffect {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AudioEffect for GainEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        GAIN_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => self.gain = value.clamp(0.0, 2.0),
            1 | 2 => self.trim_db[index - 1] = value.clamp(-24.0, 24.0),
            3 | 4 => self.inverted[index - 3] = value >= 0.5,
            _ => return false,
        }
        for channel in 0..2 {
            let polarity = if self.inverted[channel] { -1.0 } else { 1.0 };
            self.channel_gains[channel] =
                self.gain * 10.0_f32.powf(self.trim_db[channel] / 20.0) * polarity;
        }
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.gain,
            1 | 2 => self.trim_db[index - 1],
            3 | 4 => {
                if self.inverted[index - 3] {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        for frame in buffer.chunks_mut(channels.max(1)) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample *= self.channel_gains.get(channel).copied().unwrap_or(self.gain);
            }
        }
    }

    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_unity_passthrough() {
        let mut fx = GainEffect::new();
        let mut buf = vec![0.5, -0.3, 0.8, -0.1];
        let orig = buf.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn gain_half() {
        let mut fx = GainEffect::new();
        fx.set_param(0, 0.5);
        let mut buf = vec![1.0, -1.0, 0.5, -0.5];
        fx.process(&mut buf, 2);
        assert!((buf[0] - 0.5).abs() < 1e-6);
        assert!((buf[1] - (-0.5)).abs() < 1e-6);
    }

    #[test]
    fn gain_stereo_trim_and_polarity_are_independent() {
        let mut fx = GainEffect::new();
        let double_amplitude_db = 20.0 * 2.0_f32.log10();
        assert!(fx.set_param(0, 0.5));
        assert!(fx.set_param(1, double_amplitude_db));
        assert!(fx.set_param(2, -double_amplitude_db));
        assert!(fx.set_param(3, 1.0));
        assert!(fx.set_param(4, 0.0));

        let mut buffer = [0.5, 0.5, -0.25, -0.25];
        fx.process(&mut buffer, 2);
        for (actual, expected) in buffer.iter().zip([-0.5, 0.125, 0.25, -0.0625]) {
            assert!((actual - expected).abs() < 1e-6);
        }

        assert!(fx.set_param(3, 0.0));
        assert!(fx.set_param(4, 1.0));
        let mut buffer = [0.5, 0.5];
        fx.process(&mut buffer, 2);
        assert!((buffer[0] - 0.5).abs() < 1e-6);
        assert!((buffer[1] - (-0.125)).abs() < 1e-6);
    }

    #[test]
    fn gain_param_access() {
        let mut fx = GainEffect::new();
        assert!((fx.get_param(0) - 1.0).abs() < 1e-6);
        fx.set_param(0, 1.5);
        assert!((fx.get_param(0) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn gain_clamps() {
        let mut fx = GainEffect::new();
        fx.set_param(0, 5.0);
        assert!((fx.get_param(0) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn gain_invalid_param() {
        let mut fx = GainEffect::new();
        assert!(!fx.set_param(5, 0.5));
        assert!((fx.get_param(5) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn gain_rejects_nonfinite_parameters_without_changing_output() {
        let parameters = [0.5, 6.0, -6.0, 1.0, 1.0];
        for index in 0..parameters.len() {
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let mut fx = GainEffect::new();
                for (parameter, value) in parameters.iter().copied().enumerate() {
                    assert!(fx.set_param(parameter, value));
                }
                let input = [0.5, -0.5, -0.25, 0.25];
                let mut expected = input;
                fx.process(&mut expected, 2);

                assert!(!fx.set_param(index, invalid));
                for (parameter, expected) in parameters.iter().copied().enumerate() {
                    assert_eq!(fx.get_param(parameter), expected);
                }
                let mut actual = input;
                fx.process(&mut actual, 2);
                assert!(actual.iter().all(|sample| sample.is_finite()));
                assert_eq!(actual, expected);
            }
        }
    }
}
