//! Gain processing with independently adjustable stereo balance and polarity.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

/// Parameter indices. Gain stays linear amplitude at index 0 so saved projects
/// and automation keep their meaning.
pub mod param {
    pub const GAIN: usize = 0;
    pub const TRIM: [usize; 2] = [1, 2];
    pub const POLARITY: [usize; 2] = [3, 4];
}

pub const MAX_GAIN: f32 = 2.0;
pub const TRIM_RANGE_DB: f32 = 24.0;

static GAIN_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Gain",
        min: 0.0,
        max: MAX_GAIN,
        default: 1.0,
        unit: "x",
    },
    ParamDescriptor {
        name: "L Trim",
        min: -TRIM_RANGE_DB,
        max: TRIM_RANGE_DB,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "R Trim",
        min: -TRIM_RANGE_DB,
        max: TRIM_RANGE_DB,
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

/// Frames a change takes to reach its new gain, about 11 ms at 44.1 kHz: long
/// enough that a polarity flip, which ramps through zero, does not click.
const RAMP_FRAMES: u32 = 512;

pub struct GainEffect {
    gain: f32,
    trim_db: [f32; 2],
    inverted: [bool; 2],
    channel_gains: [f32; 2],
    applied_gains: [f32; 2],
    ramp_steps: [f32; 2],
    ramp_remaining: u32,
    /// Values set before any audio, as a project loads, apply at once.
    started: bool,
}

impl Default for GainEffect {
    fn default() -> Self {
        Self {
            gain: 1.0,
            trim_db: [0.0; 2],
            inverted: [false; 2],
            channel_gains: [1.0; 2],
            applied_gains: [1.0; 2],
            ramp_steps: [0.0; 2],
            ramp_remaining: 0,
            started: false,
        }
    }
}

impl GainEffect {
    pub fn new() -> Self {
        Self::default()
    }

    fn set_targets(&mut self, index: usize, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        if index == param::GAIN {
            self.gain = value.clamp(0.0, MAX_GAIN);
        } else if let Some(channel) = param::TRIM.iter().position(|&trim| trim == index) {
            self.trim_db[channel] = value.clamp(-TRIM_RANGE_DB, TRIM_RANGE_DB);
        } else if let Some(channel) = param::POLARITY.iter().position(|&flip| flip == index) {
            self.inverted[channel] = value >= 0.5;
        } else {
            return false;
        }
        for channel in 0..2 {
            let polarity = if self.inverted[channel] { -1.0 } else { 1.0 };
            self.channel_gains[channel] =
                self.gain * vibez_core::level::db_to_amplitude(self.trim_db[channel]) * polarity;
        }
        true
    }
}

impl AudioEffect for GainEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        GAIN_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        if !self.set_targets(index, value) {
            return false;
        }
        if self.started {
            for channel in 0..2 {
                self.ramp_steps[channel] = (self.channel_gains[channel]
                    - self.applied_gains[channel])
                    / RAMP_FRAMES as f32;
            }
            self.ramp_remaining = RAMP_FRAMES;
        } else {
            self.applied_gains = self.channel_gains;
        }
        true
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        if !self.set_targets(index, value) {
            return false;
        }
        self.reset();
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        if index == param::GAIN {
            self.gain
        } else if let Some(channel) = param::TRIM.iter().position(|&trim| trim == index) {
            self.trim_db[channel]
        } else if param::POLARITY
            .iter()
            .position(|&flip| flip == index)
            .is_some_and(|channel| self.inverted[channel])
        {
            1.0
        } else {
            0.0
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        self.started = true;
        for frame in buffer.chunks_mut(channels.max(1)) {
            if self.ramp_remaining > 0 {
                self.ramp_remaining -= 1;
                self.applied_gains = if self.ramp_remaining == 0 {
                    self.channel_gains
                } else {
                    [
                        self.applied_gains[0] + self.ramp_steps[0],
                        self.applied_gains[1] + self.ramp_steps[1],
                    ]
                };
            }
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample *= self
                    .applied_gains
                    .get(channel)
                    .copied()
                    .unwrap_or(self.gain);
            }
        }
    }

    fn reset(&mut self) {
        self.applied_gains = self.channel_gains;
        self.ramp_remaining = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Largest step between neighbouring frames of a constant input, starting
    /// from the last frame before the change.
    fn largest_step(before: [f32; 2], output: &[f32]) -> f32 {
        std::iter::once(&before)
            .chain(output.as_chunks::<2>().0)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| {
                (pair[1][0] - pair[0][0])
                    .abs()
                    .max((pair[1][1] - pair[0][1]).abs())
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn a_change_while_playing_ramps_instead_of_stepping() {
        let mut fx = GainEffect::new();
        let mut warmup = [1.0; 64];
        fx.process(&mut warmup, 2);

        assert!(fx.set_param(0, 0.0));
        let mut output = [1.0; 2048];
        fx.process(&mut output, 2);

        let step = largest_step([warmup[62], warmup[63]], &output);
        assert!(step < 0.01, "{step}");
        assert_eq!(output[output.len() - 1], 0.0, "reaches the new gain");
    }

    #[test]
    fn a_polarity_flip_while_playing_passes_through_zero_without_a_click() {
        let mut fx = GainEffect::new();
        let mut warmup = [1.0; 64];
        fx.process(&mut warmup, 2);

        assert!(fx.set_param(3, 1.0));
        let mut output = [1.0; 2048];
        fx.process(&mut output, 2);

        let step = largest_step([warmup[62], warmup[63]], &output);
        assert!(step < 0.01, "{step}");
        assert_eq!(output[output.len() - 2], -1.0, "left ends inverted");
        assert_eq!(output[output.len() - 1], 1.0, "right untouched");
    }

    #[test]
    fn values_set_before_any_audio_apply_at_once() {
        let mut fx = GainEffect::new();
        assert!(fx.set_param(0, 0.5));
        let mut output = [1.0; 4];
        fx.process(&mut output, 2);
        assert_eq!(output, [0.5; 4]);
    }

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
        let mut buffer = [0.5; 2 * (RAMP_FRAMES as usize + 1)];
        fx.process(&mut buffer, 2);
        let settled = &buffer[buffer.len() - 2..];
        assert!((settled[0] - 0.5).abs() < 1e-6);
        assert!((settled[1] - (-0.125)).abs() < 1e-6);
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
