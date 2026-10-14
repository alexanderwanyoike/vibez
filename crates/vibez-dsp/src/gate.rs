//! Legacy-compatible linked gating with an explicit, stable native parameter contract.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

pub mod param {
    pub const THRESHOLD: usize = 0;
    pub const ATTACK: usize = 1;
    pub const RELEASE: usize = 2;
    pub const HOLD: usize = 3;
    pub const HYSTERESIS: usize = 4;
    pub const RANGE_ENABLED: usize = 5;
    pub const RANGE: usize = 6;
    pub const MODE: usize = 7;
    pub const EXPANSION_RATIO: usize = 8;
    pub const HP_ENABLED: usize = 9;
    pub const HP_FREQUENCY: usize = 10;
    pub const LP_ENABLED: usize = 11;
    pub const LP_FREQUENCY: usize = 12;
    pub const DETECTOR_LISTEN: usize = 13;
    pub const MIX: usize = 14;
    pub const COUNT: usize = 15;
}

pub static GATE_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor { name: "Threshold", min: -80.0, max: 0.0, default: -40.0, unit: "dB" },
    ParamDescriptor { name: "Attack", min: 0.1, max: 50.0, default: 1.0, unit: "ms" },
    ParamDescriptor { name: "Release", min: 5.0, max: 500.0, default: 50.0, unit: "ms" },
    ParamDescriptor { name: "Hold", min: 0.0, max: 200.0, default: 10.0, unit: "ms" },
    ParamDescriptor { name: "Hysteresis", min: 0.0, max: 24.0, default: 0.0, unit: "dB" },
    ParamDescriptor { name: "Range Enabled", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "Range", min: 0.0, max: 120.0, default: 60.0, unit: "dB" },
    ParamDescriptor { name: "Mode", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "Expansion Ratio", min: 1.0, max: 20.0, default: 2.0, unit: ":1" },
    ParamDescriptor { name: "Detector HP", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "HP Frequency", min: 20.0, max: 2000.0, default: 80.0, unit: "Hz" },
    ParamDescriptor { name: "Detector LP", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "LP Frequency", min: 1000.0, max: 20000.0, default: 12000.0, unit: "Hz" },
    ParamDescriptor { name: "Detector Listen", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "Mix", min: 0.0, max: 1.0, default: 1.0, unit: "" },
];

/// Simple noise gate with attack / release / hold.
pub struct GateEffect {
    params: [f32; param::COUNT],
    sample_rate: f32,
    env_db: f32,
    inputs: [vibez_core::routing::ExternalInputDescriptor; 1],
    gain: f32,
    hold_counter: u32,
}

impl GateEffect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            params: std::array::from_fn(|index| GATE_PARAMS[index].default),
            sample_rate,
            env_db: -120.0,
            inputs: [vibez_core::routing::ExternalInputDescriptor {
                id: vibez_core::routing::ExternalInputId(0),
                name: "Sidechain".into(),
                channels: 2,
            }],
            gain: 0.0,
            hold_counter: 0,
        }
    }

    fn coef(time_ms: f32, sr: f32) -> f32 {
        let t = (time_ms * 0.001).max(1e-5);
        (-1.0 / (t * sr)).exp()
    }
}

impl AudioEffect for GateEffect {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Gate
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        GATE_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = GATE_PARAMS.get(index) else { return false; };
        if !value.is_finite() { return false; }
        self.params[index] = value.clamp(descriptor.min, descriptor.max);
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn external_inputs(&self) -> &[vibez_core::routing::ExternalInputDescriptor] {
        &self.inputs
    }
    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        self.process_with_inputs(buffer, channels, &[]);
    }

    fn process_with_inputs(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        inputs: &[vibez_core::routing::ExternalInputBlock<'_>],
    ) {
        if channels == 0 {
            return;
        }
        let detector = inputs
            .iter()
            .find(|input| input.id == vibez_core::routing::ExternalInputId(0) && input.connected);
        let ch = channels.min(2);
        let frames = buffer.len() / channels;
        let att = Self::coef(self.params[param::ATTACK], self.sample_rate);
        let rel = Self::coef(self.params[param::RELEASE], self.sample_rate);
        let hold_frames = (self.params[param::HOLD] * 0.001 * self.sample_rate) as u32;
        let closed_gain = if (self.params[param::RANGE_ENABLED] >= 0.5) { 10.0_f32.powf(-self.params[param::RANGE] / 20.0) } else { 0.0 };

        for frame in 0..frames {
            let mut peak = 0.0_f32;
            for c in 0..ch {
                let sample = match detector {
                    Some(input) => input
                        .samples
                        .get(frame * input.channels + c.min(input.channels - 1))
                        .copied()
                        .unwrap_or(0.0),
                    None => buffer[frame * channels + c],
                };
                peak = peak.max(sample.abs());
            }
            let peak_db = 20.0 * (peak.max(1e-10)).log10();
            let env_coef = if peak_db > self.env_db { att } else { rel };
            self.env_db = peak_db + env_coef * (self.env_db - peak_db);

            let open = self.env_db >= self.params[param::THRESHOLD];
            let target = if open || self.hold_counter > 0 {
                1.0
            } else {
                closed_gain
            };
            let g_coef = if target > self.gain { att } else { rel };
            self.gain = target + g_coef * (self.gain - target);

            if open {
                self.hold_counter = hold_frames;
            } else if self.hold_counter > 0 {
                self.hold_counter -= 1;
            }

            for c in 0..ch {
                buffer[frame * channels + c] *= self.gain;
            }
        }
    }

    fn reset(&mut self) {
        self.env_db = -120.0;
        self.gain = 0.0;
        self.hold_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_opens_on_loud_signal() {
        let mut fx = GateEffect::new(44_100.0);
        fx.set_param(0, -40.0);
        fx.set_param(1, 0.5);

        let mut buf = vec![0.5_f32; 4_410];
        fx.process(&mut buf, 2);
        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak > 0.4, "gate should open: peak {peak}");
    }

    #[test]
    fn gate_closes_on_quiet_signal() {
        let mut fx = GateEffect::new(44_100.0);
        fx.set_param(0, -20.0);
        fx.set_param(2, 5.0);

        let mut buf = vec![0.005_f32; 22_050];
        fx.process(&mut buf, 2);
        let tail = &buf[buf.len() - 2_000..];
        let peak_tail = tail.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak_tail < 0.003, "gate should close: tail {peak_tail}");
    }
}
