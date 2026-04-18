use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static GAIN_PARAMS: &[ParamDescriptor] = &[ParamDescriptor {
    name: "Gain",
    min: 0.0,
    max: 2.0,
    default: 1.0,
    unit: "x",
}];

pub struct GainEffect {
    gain: f32,
}

impl Default for GainEffect {
    fn default() -> Self {
        Self { gain: 1.0 }
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
        if index == 0 {
            self.gain = value.clamp(0.0, 2.0);
            true
        } else {
            false
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        if index == 0 {
            self.gain
        } else {
            0.0
        }
    }

    fn process(&mut self, buffer: &mut [f32], _channels: usize) {
        for sample in buffer.iter_mut() {
            *sample *= self.gain;
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
        assert!(!fx.set_param(1, 0.5));
        assert!((fx.get_param(1) - 0.0).abs() < 1e-6);
    }
}
