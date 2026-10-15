//! Legacy-compatible linked gating with an explicit, stable native parameter contract.

use vibez_core::effect::{DynamicsMeter, EffectType, GateMeter, GatePhase, ParamDescriptor};

use crate::{butterworth_filter::ButterworthFilter as DetectorFilter, effect::AudioEffect};

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

pub const RAMP_FRAMES: u32 = 512;

pub static GATE_PARAMS: &[ParamDescriptor] = &[
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
    ParamDescriptor {
        name: "Hysteresis",
        min: 0.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Range Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Range",
        min: 0.0,
        max: 120.0,
        default: 60.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Mode",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Expansion Ratio",
        min: 1.0,
        max: 20.0,
        default: 2.0,
        unit: ":1",
    },
    ParamDescriptor {
        name: "Detector HP",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "HP Frequency",
        min: 20.0,
        max: 2000.0,
        default: 80.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Detector LP",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "LP Frequency",
        min: 1000.0,
        max: 20000.0,
        default: 12000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Detector Listen",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
];

/// Simple noise gate with attack / release / hold.
pub struct GateEffect {
    params: [f32; param::COUNT],
    applied: [f32; param::COUNT],
    steps: [f32; param::COUNT],
    remaining: [u32; param::COUNT],
    started: bool,
    attack_coefficient: f32,
    release_coefficient: f32,
    sample_rate: f32,
    filters: [DetectorFilter; 2],
    env_db: f32,
    open: bool,
    inputs: [vibez_core::routing::ExternalInputDescriptor; 1],
    gain: f32,
    expander_gain: f64,
    hold_counter: u32,
    meter: DynamicsMeter,
    window_started: bool,
}

impl GateEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        let params = std::array::from_fn(|index| GATE_PARAMS[index].default);
        let mut effect = Self {
            params,
            applied: params,
            steps: [0.0; param::COUNT],
            remaining: [0; param::COUNT],
            started: false,
            attack_coefficient: Self::coef(params[param::ATTACK], sample_rate),
            release_coefficient: Self::coef(params[param::RELEASE], sample_rate),
            sample_rate,
            filters: [DetectorFilter::EMPTY; 2],
            env_db: -120.0,
            open: false,
            inputs: [vibez_core::routing::ExternalInputDescriptor {
                id: vibez_core::routing::ExternalInputId(0),
                name: "Sidechain".into(),
                channels: 2,
            }],
            gain: 0.0,
            expander_gain: 1.0,
            hold_counter: 0,
            meter: DynamicsMeter {
                gate: Some(GateMeter::default()),
                ..Default::default()
            },
            window_started: false,
        };
        effect.configure_filters();
        effect
    }

    fn target(&self, index: usize) -> f32 {
        if [
            param::RANGE_ENABLED,
            param::MODE,
            param::HP_ENABLED,
            param::LP_ENABLED,
            param::DETECTOR_LISTEN,
        ]
        .contains(&index)
        {
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
        let Some(descriptor) = GATE_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = value.clamp(descriptor.min, descriptor.max);
        if self.params[index] == value && !automated {
            return true;
        }
        self.params[index] = value;
        let target = self.target(index);
        if automated || !self.started {
            self.applied[index] = target;
            self.remaining[index] = 0;
            self.refresh_coefficient(index);
        } else {
            self.steps[index] = (target - self.applied[index]) / RAMP_FRAMES as f32;
            self.remaining[index] = RAMP_FRAMES;
        }
        true
    }

    fn refresh_coefficient(&mut self, index: usize) {
        match index {
            param::ATTACK => {
                self.attack_coefficient = Self::coef(self.applied[index], self.sample_rate)
            }
            param::RELEASE => {
                self.release_coefficient = Self::coef(self.applied[index], self.sample_rate)
            }
            param::HP_FREQUENCY => {
                self.filters[0].configure(self.sample_rate, self.applied[index], true)
            }
            param::LP_FREQUENCY => {
                self.filters[1].configure(self.sample_rate, self.applied[index], false)
            }
            _ => {}
        }
    }

    fn configure_filters(&mut self) {
        self.refresh_coefficient(param::HP_FREQUENCY);
        self.refresh_coefficient(param::LP_FREQUENCY);
    }

    fn advance(&mut self) {
        let mut changed = [false; param::COUNT];
        for (index, changed) in changed.iter_mut().enumerate() {
            if self.remaining[index] == 0 {
                continue;
            }
            self.remaining[index] -= 1;
            self.applied[index] = if self.remaining[index] == 0 {
                self.target(index)
            } else {
                self.applied[index] + self.steps[index]
            };
            *changed = true;
        }
        for index in [
            param::ATTACK,
            param::RELEASE,
            param::HP_FREQUENCY,
            param::LP_FREQUENCY,
        ] {
            if changed[index] {
                self.refresh_coefficient(index);
            }
        }
    }

    fn wet_gain(&self) -> f32 {
        self.gain + (self.expander_gain as f32 - self.gain) * self.applied[param::MODE]
    }

    fn coef(time_ms: f32, sr: f32) -> f32 {
        let t = (time_ms * 0.001).max(1e-5);
        (-1.0 / (t * sr)).exp()
    }
}

impl AudioEffect for GateEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn take_dynamics_meter(&mut self) -> Option<DynamicsMeter> {
        if !self.started {
            let gain = self.wet_gain();
            self.meter.current_reduction_db = [-20.0 * gain.max(1e-10).log10(); 2];
            if let Some(gate) = &mut self.meter.gate {
                gate.open_threshold_db = self.applied[param::THRESHOLD];
                gate.close_threshold_db =
                    self.applied[param::THRESHOLD] - self.applied[param::HYSTERESIS];
            }
        }
        let meter = self.meter;
        self.meter.peak_reduction_db = [0.0; 2];
        self.meter.detector_peak = [0.0; 2];
        if let Some(gate) = &mut self.meter.gate {
            gate.phases_seen = 0;
            gate.peak_envelope_db = gate.envelope_db;
        }
        self.window_started = false;
        Some(meter)
    }

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
        self.set_target(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, true)
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
        let detector = inputs.iter().find(|input| {
            input.id == vibez_core::routing::ExternalInputId(0)
                && input.connected
                && input.channels > 0
        });
        let ch = channels.min(2);
        let frames = buffer.len() / channels;
        for frame in 0..frames {
            self.started = true;
            self.advance();
            let att = self.attack_coefficient;
            let rel = self.release_coefficient;
            let hold_frames = (self.applied[param::HOLD] * 0.001 * self.sample_rate) as u32;
            let closed_gain = 10.0_f32.powf(-self.applied[param::RANGE] / 20.0)
                * self.applied[param::RANGE_ENABLED];
            let detected: [f32; 2] = std::array::from_fn(|channel| {
                let raw = match detector {
                    Some(input) => frame
                        .checked_mul(input.channels)
                        .and_then(|offset| offset.checked_add(channel.min(input.channels - 1)))
                        .and_then(|offset| input.samples.get(offset))
                        .copied()
                        .unwrap_or(0.0),
                    None => buffer[frame * channels + channel.min(ch - 1)],
                };
                let raw = if raw.is_finite() {
                    raw.clamp(-1e12, 1e12)
                } else {
                    0.0
                };
                let high = self.filters[0].process(raw, channel);
                let high = raw + (high - raw) * self.applied[param::HP_ENABLED];
                let low = self.filters[1].process(high, channel);
                high + (low - high) * self.applied[param::LP_ENABLED]
            });
            let peak = detected[0].abs().max(detected[1].abs());
            let peak_db = 20.0 * (peak.max(1e-10)).log10();
            let env_coef = if peak_db > self.env_db { att } else { rel };
            self.env_db = peak_db + env_coef * (self.env_db - peak_db);

            let threshold = self.applied[param::THRESHOLD]
                - if self.open {
                    self.applied[param::HYSTERESIS]
                } else {
                    0.0
                };
            self.open = self.env_db >= threshold;
            let target = if self.open || self.hold_counter > 0 {
                1.0
            } else {
                closed_gain
            };
            let g_coef = if target > self.gain { att } else { rel };
            self.gain = target + g_coef * (self.gain - target);
            let deficit = (self.applied[param::THRESHOLD] - self.env_db).max(0.0)
                * (self.applied[param::EXPANSION_RATIO] - 1.0);
            let unlimited = 10.0_f32.powf(-deficit / 20.0);
            let limited = 10.0_f32.powf(-deficit.min(self.applied[param::RANGE]) / 20.0);
            let expansion_target = if self.open || self.hold_counter > 0 {
                1.0
            } else {
                unlimited + (limited - unlimited) * self.applied[param::RANGE_ENABLED]
            };
            let expansion_target = expansion_target as f64;
            let coefficient = if expansion_target > self.expander_gain {
                att
            } else {
                rel
            } as f64;
            self.expander_gain =
                expansion_target + coefficient * (self.expander_gain - expansion_target);
            let gain = self.wet_gain();

            let phase = if self.open {
                GatePhase::Open
            } else if self.hold_counter > 0 {
                GatePhase::Hold
            } else {
                GatePhase::Closed
            };
            let reduction_db = -20.0 * gain.max(1e-10).log10();
            self.meter.current_reduction_db = [reduction_db; 2];
            for (channel, sample) in detected.into_iter().enumerate() {
                self.meter.peak_reduction_db[channel] =
                    self.meter.peak_reduction_db[channel].max(reduction_db);
                self.meter.detector_peak[channel] =
                    self.meter.detector_peak[channel].max(sample.abs());
            }
            let previous = self.meter.gate.unwrap_or_default();
            self.meter.gate = Some(GateMeter {
                phase,
                envelope_db: self.env_db,
                peak_envelope_db: if self.window_started {
                    previous.peak_envelope_db.max(self.env_db)
                } else {
                    self.env_db
                },
                open_threshold_db: self.applied[param::THRESHOLD],
                close_threshold_db: self.applied[param::THRESHOLD]
                    - self.applied[param::HYSTERESIS],
                hold_remaining_ms: (if self.open {
                    hold_frames
                } else {
                    self.hold_counter
                }) as f32
                    * 1000.0
                    / self.sample_rate,
                phases_seen: previous.phases_seen | phase as u8,
            });
            self.window_started = true;
            if self.open {
                self.hold_counter = hold_frames;
            } else if self.hold_counter > 0 {
                self.hold_counter -= 1;
            }

            for c in 0..ch {
                let sample = &mut buffer[frame * channels + c];
                let dry = if sample.is_finite() { *sample } else { 0.0 };
                let normal =
                    dry * (1.0 - self.applied[param::MIX]) + dry * gain * self.applied[param::MIX];
                *sample = normal + (detected[c] - normal) * self.applied[param::DETECTOR_LISTEN];
            }
        }
    }

    fn reset(&mut self) {
        self.env_db = -120.0;
        self.open = false;
        self.gain = 0.0;
        self.expander_gain = 1.0;
        self.hold_counter = 0;
        self.applied = std::array::from_fn(|index| self.target(index));
        self.remaining = [0; param::COUNT];
        self.started = false;
        self.refresh_coefficient(param::ATTACK);
        self.refresh_coefficient(param::RELEASE);
        self.configure_filters();
        self.meter = DynamicsMeter {
            gate: Some(GateMeter {
                open_threshold_db: self.params[param::THRESHOLD],
                close_threshold_db: self.params[param::THRESHOLD] - self.params[param::HYSTERESIS],
                ..Default::default()
            }),
            ..Default::default()
        };
        self.window_started = false;
        for filter in &mut self.filters {
            filter.reset();
        }
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
