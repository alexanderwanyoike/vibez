//! Legacy-compatible linked gating with an explicit, stable native parameter contract.

use vibez_core::effect::{DynamicsMeter, EffectType, GateMeter, GatePhase, ParamDescriptor};

use crate::{detector_filter::DetectorFilter, effect::AudioEffect};

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
        let mut effect = Self {
            params: std::array::from_fn(|index| GATE_PARAMS[index].default),
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
            meter: DynamicsMeter { gate: Some(GateMeter::default()), ..Default::default() },
            window_started: false,
        };
        effect.configure_filters();
        effect
    }

    fn configure_filters(&mut self) {
        self.filters[0].configure(self.sample_rate, self.params[param::HP_FREQUENCY], true);
        self.filters[1].configure(self.sample_rate, self.params[param::LP_FREQUENCY], false);
    }

    fn coef(time_ms: f32, sr: f32) -> f32 {
        let t = (time_ms * 0.001).max(1e-5);
        (-1.0 / (t * sr)).exp()
    }
}

impl AudioEffect for GateEffect {
    fn supports_level_meter(&self) -> bool { true }

    fn take_dynamics_meter(&mut self) -> Option<DynamicsMeter> {
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
        let Some(descriptor) = GATE_PARAMS.get(index) else { return false; };
        if !value.is_finite() { return false; }
        self.params[index] = value.clamp(descriptor.min, descriptor.max);
        if index == param::HP_FREQUENCY || index == param::LP_FREQUENCY {
            self.configure_filters();
        }
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
        let closed_gain = if self.params[param::RANGE_ENABLED] >= 0.5 { 10.0_f32.powf(-self.params[param::RANGE] / 20.0) } else { 0.0 };

        for frame in 0..frames {
            let detected: [f32; 2] = std::array::from_fn(|channel| {
                let raw = match detector {
                    Some(input) => input.samples
                        .get(frame * input.channels + channel.min(input.channels - 1))
                        .copied().unwrap_or(0.0),
                    None => buffer[frame * channels + channel.min(ch - 1)],
                };
                let high = self.filters[0].process(raw, channel);
                let high = if self.params[param::HP_ENABLED] >= 0.5 { high } else { raw };
                let low = self.filters[1].process(high, channel);
                if self.params[param::LP_ENABLED] >= 0.5 { low } else { high }
            });
            let peak = detected[0].abs().max(detected[1].abs());
            let peak_db = 20.0 * (peak.max(1e-10)).log10();
            let env_coef = if peak_db > self.env_db { att } else { rel };
            self.env_db = peak_db + env_coef * (self.env_db - peak_db);

            let threshold = self.params[param::THRESHOLD]
                - if self.open { self.params[param::HYSTERESIS] } else { 0.0 };
            self.open = self.env_db >= threshold;
            let target = if self.open || self.hold_counter > 0 {
                1.0
            } else {
                closed_gain
            };
            let g_coef = if target > self.gain { att } else { rel };
            self.gain = target + g_coef * (self.gain - target);
            let deficit = (self.params[param::THRESHOLD] - self.env_db).max(0.0)
                * (self.params[param::EXPANSION_RATIO] - 1.0);
            let reduction = if self.params[param::RANGE_ENABLED] >= 0.5 {
                deficit.min(self.params[param::RANGE])
            } else { deficit };
            let expansion_target = if self.open || self.hold_counter > 0 {
                1.0
            } else { 10.0_f32.powf(-reduction / 20.0) };
            let expansion_target = expansion_target as f64;
            let coefficient = if expansion_target > self.expander_gain { att } else { rel } as f64;
            self.expander_gain = expansion_target + coefficient * (self.expander_gain - expansion_target);
            let gain = if self.params[param::MODE] >= 0.5 { self.expander_gain as f32 } else { self.gain };

            let phase = if self.open { GatePhase::Open }
                else if self.hold_counter > 0 { GatePhase::Hold } else { GatePhase::Closed };
            let reduction_db = -20.0 * gain.max(1e-10).log10();
            self.meter.current_reduction_db = [reduction_db; 2];
            for (channel, sample) in detected.into_iter().enumerate() {
                self.meter.peak_reduction_db[channel] = self.meter.peak_reduction_db[channel].max(reduction_db);
                self.meter.detector_peak[channel] = self.meter.detector_peak[channel].max(sample.abs());
            }
            let previous = self.meter.gate.unwrap_or_default();
            self.meter.gate = Some(GateMeter {
                phase, envelope_db: self.env_db,
                peak_envelope_db: if self.window_started { previous.peak_envelope_db.max(self.env_db) } else { self.env_db },
                open_threshold_db: self.params[param::THRESHOLD],
                close_threshold_db: self.params[param::THRESHOLD] - self.params[param::HYSTERESIS],
                hold_remaining_ms: (if self.open { hold_frames } else { self.hold_counter }) as f32 * 1000.0 / self.sample_rate,
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
                *sample = if self.params[param::DETECTOR_LISTEN] >= 0.5 {
                    detected[c]
                } else { *sample * gain };
            }
        }
    }

    fn reset(&mut self) {
        self.env_db = -120.0;
        self.open = false;
        self.gain = 0.0;
        self.expander_gain = 1.0;
        self.hold_counter = 0;
        self.meter = DynamicsMeter {
            gate: Some(GateMeter {
                open_threshold_db: self.params[param::THRESHOLD],
                close_threshold_db: self.params[param::THRESHOLD] - self.params[param::HYSTERESIS],
                ..Default::default()
            }), ..Default::default()
        };
        self.window_started = false;
        for filter in &mut self.filters { filter.reset(); }
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
