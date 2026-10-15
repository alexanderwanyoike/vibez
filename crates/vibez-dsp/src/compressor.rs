//! Feedforward compression with smooth controls, filtered detectors and real reduction diagnostics.

use vibez_core::effect::{DynamicsMeter, EffectType, ParamDescriptor};
use vibez_core::level::{amplitude_to_db, db_to_amplitude};
use vibez_core::routing::{ExternalInputBlock, ExternalInputDescriptor, ExternalInputId};

use crate::butterworth_filter::ButterworthFilter as DetectorFilter;

pub mod param {
    pub const THRESHOLD: usize = 0;
    pub const RATIO: usize = 1;
    pub const ATTACK: usize = 2;
    pub const RELEASE: usize = 3;
    pub const MAKEUP: usize = 4;
    pub const MIX: usize = 5;
    pub const KNEE: usize = 6;
    pub const HP_ENABLED: usize = 7;
    pub const HP_FREQUENCY: usize = 8;
    pub const LP_ENABLED: usize = 9;
    pub const LP_FREQUENCY: usize = 10;
    pub const DETECTOR_MODE: usize = 11;
    pub const AUTO_RELEASE: usize = 12;
    pub const STEREO_LINK: usize = 13;
    pub const DETECTOR_LISTEN: usize = 14;
    pub const RANGE_ENABLED: usize = 15;
    pub const RANGE: usize = 16;
    pub const COUNT: usize = 17;
}

pub const RAMP_FRAMES: u32 = 512;
pub const RMS_AVERAGING_MS: f32 = 20.0;
pub const AUTO_MEMORY_MS: f32 = 200.0;

use crate::effect::AudioEffect;

pub static COMPRESSOR_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Threshold",
        min: -60.0,
        max: 0.0,
        default: -18.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Ratio",
        min: 1.0,
        max: 20.0,
        default: 4.0,
        unit: ":1",
    },
    ParamDescriptor {
        name: "Attack",
        min: 0.1,
        max: 100.0,
        default: 5.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Release",
        min: 10.0,
        max: 500.0,
        default: 80.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Makeup",
        min: 0.0,
        max: 24.0,
        default: 3.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Knee",
        min: 0.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
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
        default: 120.0,
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
        name: "Detector Mode",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Auto Release",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Stereo Link",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Detector Listen",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
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
];

fn time_coefficient(milliseconds: f32, rate: f32) -> f32 {
    (-1.0 / ((milliseconds * 0.001).max(1e-5) * rate)).exp()
}

fn reduction_db(level_db: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
    let over = level_db - threshold;
    let slope = 1.0 - 1.0 / ratio;
    if knee <= 0.0 || over >= knee * 0.5 {
        slope * over.max(0.0)
    } else if over <= -knee * 0.5 {
        0.0
    } else {
        slope * (over + knee * 0.5).powi(2) / (2.0 * knee)
    }
}

/// Steady internal-detector transfer; external detection and envelope memory
/// cannot be inferred from main input level alone.
pub fn static_transfer_db(input_db: f32, params: &[f32]) -> f32 {
    if !input_db.is_finite() {
        return input_db;
    }
    let p: [f32; param::COUNT] = std::array::from_fn(|index| {
        let descriptor = &COMPRESSOR_PARAMS[index];
        params
            .get(index)
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(descriptor.default)
            .clamp(descriptor.min, descriptor.max)
    });
    let reduction = reduction_db(
        input_db,
        p[param::THRESHOLD],
        p[param::RATIO],
        p[param::KNEE],
    );
    let reduction = if p[param::RANGE_ENABLED] >= 0.5 {
        reduction.min(p[param::RANGE])
    } else {
        reduction
    };
    let gain = (1.0 - p[param::MIX])
        + p[param::MIX] * db_to_amplitude(p[param::MAKEUP] - reduction);
    input_db + amplitude_to_db(gain.max(1e-10))
}

pub struct CompressorEffect {
    params: [f32; param::COUNT],
    applied: [f32; param::COUNT],
    steps: [f32; param::COUNT],
    remaining: [u32; param::COUNT],
    sample_rate: f32,
    envelopes: [[[[f32; 2]; 2]; 2]; 2],
    activity: [[[f32; 2]; 2]; 2],
    rms_squared: [f32; 2],
    filters: [DetectorFilter; 2],
    attack_coefficient: f32,
    release_coefficient: f32,
    rms_coefficient: f32,
    memory_coefficient: f32,
    makeup_gain: f32,
    coefficient_clock: u32,
    started: bool,
    meter: DynamicsMeter,
    inputs: [ExternalInputDescriptor; 1],
}

impl CompressorEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48000.0
        };
        let params = std::array::from_fn(|index| COMPRESSOR_PARAMS[index].default);
        let mut effect = Self {
            params,
            applied: params,
            steps: [0.0; param::COUNT],
            remaining: [0; param::COUNT],
            sample_rate,
            envelopes: [[[[-120.0; 2]; 2]; 2]; 2],
            activity: [[[0.0; 2]; 2]; 2],
            rms_squared: [0.0; 2],
            filters: [DetectorFilter::EMPTY; 2],
            attack_coefficient: time_coefficient(params[param::ATTACK], sample_rate),
            release_coefficient: time_coefficient(params[param::RELEASE], sample_rate),
            rms_coefficient: time_coefficient(RMS_AVERAGING_MS, sample_rate),
            memory_coefficient: time_coefficient(AUTO_MEMORY_MS, sample_rate),
            makeup_gain: db_to_amplitude(params[param::MAKEUP]),
            coefficient_clock: 0,
            started: false,
            meter: DynamicsMeter::default(),
            inputs: [ExternalInputDescriptor {
                id: ExternalInputId(0),
                name: "Sidechain".into(),
                channels: 2,
            }],
        };
        effect.configure_filters();
        effect
    }

    pub fn from_params(sample_rate: f32, params: &[f32]) -> Self {
        let mut effect = Self::new(sample_rate);
        for (index, value) in params.iter().copied().enumerate().take(param::COUNT) {
            effect.set_param(index, value);
        }
        effect
    }

    fn target(&self, index: usize) -> f32 {
        if [
            param::HP_ENABLED,
            param::LP_ENABLED,
            param::DETECTOR_MODE,
            param::AUTO_RELEASE,
            param::STEREO_LINK,
            param::DETECTOR_LISTEN,
            param::RANGE_ENABLED,
        ]
        .contains(&index)
        {
            if self.params[index] >= 0.5 { 1.0 } else { 0.0 }
        } else {
            self.params[index]
        }
    }

    fn set_target(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = COMPRESSOR_PARAMS.get(index) else {
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
            self.refresh_coefficients();
        } else {
            self.steps[index] = (target - self.applied[index]) / RAMP_FRAMES as f32;
            self.remaining[index] = RAMP_FRAMES;
        }
        true
    }

    fn configure_filters(&mut self) {
        self.filters[0].configure(self.sample_rate, self.applied[param::HP_FREQUENCY], true);
        self.filters[1].configure(self.sample_rate, self.applied[param::LP_FREQUENCY], false);
    }

    fn refresh_coefficients(&mut self) {
        self.configure_filters();
        self.attack_coefficient = time_coefficient(self.applied[param::ATTACK], self.sample_rate);
        self.release_coefficient = time_coefficient(self.applied[param::RELEASE], self.sample_rate);
        self.makeup_gain = db_to_amplitude(self.applied[param::MAKEUP]);
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
        if changed[param::ATTACK] {
            self.attack_coefficient =
                time_coefficient(self.applied[param::ATTACK], self.sample_rate);
        }
        if changed[param::RELEASE] {
            self.release_coefficient =
                time_coefficient(self.applied[param::RELEASE], self.sample_rate);
        }
        if changed[param::MAKEUP] {
            self.makeup_gain = db_to_amplitude(self.applied[param::MAKEUP]);
        }
        if [param::HP_FREQUENCY, param::LP_FREQUENCY]
            .into_iter()
            .any(|index| {
                changed[index] && (self.coefficient_clock == 0 || self.remaining[index] == 0)
            })
        {
            self.configure_filters();
        }
        self.coefficient_clock = (self.coefficient_clock + 1) % 8;
    }

    fn gains(&mut self, detector: [f32; 2]) -> [f32; 2] {
        let p = &self.applied;
        for (channel, sample) in detector.into_iter().enumerate() {
            self.rms_squared[channel] = sample * sample
                + self.rms_coefficient * (self.rms_squared[channel] - sample * sample);
        }
        let levels = [
            detector.map(f32::abs),
            self.rms_squared.map(|v| v.max(0.0).sqrt()),
        ];
        let mut gains = [0.0; 2];
        for (mode, levels) in levels.into_iter().enumerate() {
            for link in 0..2 {
                for channel in 0..2 {
                    let value = if link == 1 {
                        levels[0].max(levels[1])
                    } else {
                        levels[channel]
                    };
                    let level_db = amplitude_to_db(value.max(1e-10));
                    let desired = reduction_db(
                        level_db,
                        p[param::THRESHOLD],
                        p[param::RATIO],
                        p[param::KNEE],
                    );
                    let activity = &mut self.activity[mode][link][channel];
                    let target = (desired / 12.0).clamp(0.0, 1.0);
                    *activity = target + self.memory_coefficient * (*activity - target);
                    let adaptive_time =
                        (p[param::RELEASE] * (0.5 + 2.5 * *activity)).clamp(5.0, 2000.0);
                    for automatic in 0..2 {
                        let envelope = &mut self.envelopes[mode][link][automatic][channel];
                        let release = if automatic == 0 {
                            self.release_coefficient
                        } else {
                            time_coefficient(adaptive_time, self.sample_rate)
                        };
                        let coefficient = if level_db > *envelope {
                            self.attack_coefficient
                        } else {
                            release
                        };
                        *envelope = level_db + coefficient * (*envelope - level_db);
                        let weight = if mode == 0 {
                            1.0 - p[param::DETECTOR_MODE]
                        } else {
                            p[param::DETECTOR_MODE]
                        } * if link == 0 {
                            1.0 - p[param::STEREO_LINK]
                        } else {
                            p[param::STEREO_LINK]
                        } * if automatic == 0 {
                            1.0 - p[param::AUTO_RELEASE]
                        } else {
                            p[param::AUTO_RELEASE]
                        };
                        if weight == 0.0 {
                            continue;
                        }
                        let reduction = reduction_db(
                            *envelope,
                            p[param::THRESHOLD],
                            p[param::RATIO],
                            p[param::KNEE],
                        );
                        let unlimited = db_to_amplitude(-reduction);
                        let limited = db_to_amplitude(-reduction.min(p[param::RANGE]));
                        let gain = unlimited + (limited - unlimited) * p[param::RANGE_ENABLED];
                        gains[channel] += gain * weight;
                    }
                }
            }
        }
        gains
    }
}

impl AudioEffect for CompressorEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }
    fn take_dynamics_meter(&mut self) -> Option<DynamicsMeter> {
        let meter = self.meter;
        self.meter.peak_reduction_db = [0.0; 2];
        self.meter.detector_peak = [0.0; 2];
        Some(meter)
    }
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Compressor
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        COMPRESSOR_PARAMS
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
    fn external_inputs(&self) -> &[ExternalInputDescriptor] {
        &self.inputs
    }
    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        self.process_with_inputs(buffer, channels, &[]);
    }

    fn process_with_inputs(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        inputs: &[ExternalInputBlock<'_>],
    ) {
        if channels == 0 {
            return;
        }
        let external = inputs
            .iter()
            .find(|input| input.id == ExternalInputId(0) && input.connected && input.channels > 0);
        for (frame_index, frame) in buffer.chunks_exact_mut(channels).enumerate() {
            self.started = true;
            self.advance();
            let raw: [f32; 2] = std::array::from_fn(|channel| {
                let sample = if let Some(input) = external {
                    frame_index
                        .checked_mul(input.channels)
                        .and_then(|offset| offset.checked_add(channel.min(input.channels - 1)))
                        .and_then(|index| input.samples.get(index))
                        .copied()
                        .unwrap_or(0.0)
                } else {
                    frame[channel.min(channels - 1)]
                };
                if sample.is_finite() {
                    sample.clamp(-1e12, 1e12)
                } else {
                    0.0
                }
            });
            let detector = std::array::from_fn(|channel| {
                let high = self.filters[0].process(raw[channel], channel);
                let high = raw[channel] + (high - raw[channel]) * self.applied[param::HP_ENABLED];
                let low = self.filters[1].process(high, channel);
                high + (low - high) * self.applied[param::LP_ENABLED]
            });
            let gains = self.gains(detector);
            for channel in 0..2 {
                let reduction = -amplitude_to_db(gains[channel].max(1e-10));
                self.meter.current_reduction_db[channel] = reduction.max(0.0);
                self.meter.peak_reduction_db[channel] =
                    self.meter.peak_reduction_db[channel].max(reduction);
                self.meter.detector_peak[channel] =
                    self.meter.detector_peak[channel].max(detector[channel].abs());
            }
            for (channel, sample) in frame.iter_mut().take(2).enumerate() {
                let wet = *sample * gains[channel] * self.makeup_gain;
                let normal =
                    *sample * (1.0 - self.applied[param::MIX]) + wet * self.applied[param::MIX];
                *sample =
                    normal + (detector[channel] - normal) * self.applied[param::DETECTOR_LISTEN];
            }
        }
    }

    fn reset(&mut self) {
        self.envelopes = [[[[-120.0; 2]; 2]; 2]; 2];
        self.activity = [[[0.0; 2]; 2]; 2];
        self.rms_squared = [0.0; 2];
        for filter in &mut self.filters {
            filter.reset();
        }
        self.applied = std::array::from_fn(|index| self.target(index));
        self.remaining = [0; param::COUNT];
        self.meter = DynamicsMeter::default();
        self.coefficient_clock = 0;
        self.started = false;
        self.refresh_coefficients();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_loud_signal_gain() {
        let mut fx = CompressorEffect::new(44_100.0);
        fx.set_param(0, -20.0);
        fx.set_param(1, 10.0);
        fx.set_param(4, 0.0);
        fx.set_param(5, 1.0);

        // 1 second buffer so the envelope has time to settle.
        let mut buf = vec![0.5_f32; 44_100];
        fx.process(&mut buf, 2);
        // After the envelope catches up the tail should be heavily reduced.
        let tail = &buf[buf.len() - 1024..];
        let peak_tail = tail.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(
            peak_tail < 0.3,
            "expected compression in tail, got peak {peak_tail}"
        );
    }

    #[test]
    fn leaves_quiet_signal_untouched() {
        let mut fx = CompressorEffect::new(44_100.0);
        fx.set_param(0, -12.0);
        fx.set_param(1, 8.0);
        fx.set_param(4, 0.0);
        fx.set_param(5, 1.0);

        let mut buf = vec![0.05_f32; 4_410];
        fx.process(&mut buf, 2);
        for s in &buf {
            assert!((s - 0.05).abs() < 0.01, "{s}");
        }
    }
}

#[cfg(test)]
#[path = "compressor_device_tests.rs"]
mod device_tests;
