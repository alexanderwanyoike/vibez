use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static GATE_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Threshold",
        min: -80.0,
        max: 0.0,
        default: -40.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Attack",
        min: 0.1,
        max: 50.0,
        default: 1.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Release",
        min: 5.0,
        max: 500.0,
        default: 50.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Hold",
        min: 0.0,
        max: 200.0,
        default: 10.0,
        unit: "ms",
    },
];

/// Simple noise gate with attack / release / hold.
pub struct GateEffect {
    threshold_db: f32,
    attack_ms: f32,
    release_ms: f32,
    hold_ms: f32,
    sample_rate: f32,
    env_db: f32,
    gain: f32,
    hold_counter: u32,
}

impl GateEffect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            threshold_db: -40.0,
            attack_ms: 1.0,
            release_ms: 50.0,
            hold_ms: 10.0,
            sample_rate,
            env_db: -120.0,
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
    fn effect_type(&self) -> EffectType {
        EffectType::Gate
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        GATE_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.threshold_db = value.clamp(-80.0, 0.0);
                true
            }
            1 => {
                self.attack_ms = value.clamp(0.1, 50.0);
                true
            }
            2 => {
                self.release_ms = value.clamp(5.0, 500.0);
                true
            }
            3 => {
                self.hold_ms = value.clamp(0.0, 200.0);
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.threshold_db,
            1 => self.attack_ms,
            2 => self.release_ms,
            3 => self.hold_ms,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let att = Self::coef(self.attack_ms, self.sample_rate);
        let rel = Self::coef(self.release_ms, self.sample_rate);
        let hold_frames = (self.hold_ms * 0.001 * self.sample_rate) as u32;

        for frame in 0..frames {
            let mut peak = 0.0_f32;
            for c in 0..ch {
                peak = peak.max(buffer[frame * ch + c].abs());
            }
            let peak_db = 20.0 * (peak.max(1e-10)).log10();
            let env_coef = if peak_db > self.env_db { att } else { rel };
            self.env_db = peak_db + env_coef * (self.env_db - peak_db);

            let open = self.env_db >= self.threshold_db;
            let target = if open || self.hold_counter > 0 {
                1.0
            } else {
                0.0
            };
            let g_coef = if target > self.gain { att } else { rel };
            self.gain = target + g_coef * (self.gain - target);

            if open {
                self.hold_counter = hold_frames;
            } else if self.hold_counter > 0 {
                self.hold_counter -= 1;
            }

            for c in 0..ch {
                buffer[frame * ch + c] *= self.gain;
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
