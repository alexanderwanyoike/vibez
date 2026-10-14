//! Multimode stereo filtering with smoothed edits and tempo-aware cutoff modulation.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

#[path = "filter_response.rs"]
mod response;
pub use response::response_magnitude;
use response::Coefficients;

pub mod param {
    pub const CUTOFF: usize = 0;
    pub const RESONANCE: usize = 1;
    pub const MODE: usize = 2;
    pub const SLOPE: usize = 3;
    pub const DRIVE: usize = 4;
    pub const MIX: usize = 5;
    pub const MOD_SOURCE: usize = 6;
    pub const RATE: usize = 7;
    pub const DEPTH: usize = 8;
    pub const SYNC: usize = 9;
    pub const STEREO_PHASE: usize = 10;
    pub const ENV_ATTACK: usize = 11;
    pub const ENV_RELEASE: usize = 12;
    pub const COUNT: usize = 13;
}

pub const CUTOFF_MIN: f32 = 20.0;
pub const CUTOFF_MAX: f32 = 20000.0;
pub const RESONANCE_MIN: f32 = 0.1;
pub const RESONANCE_MAX: f32 = 10.0;
pub const DRIVE_MAX_DB: f32 = 24.0;
pub const RATE_MIN_HZ: f32 = 0.05;
pub const RATE_MAX_HZ: f32 = 20.0;
pub const DEPTH_MAX_OCTAVES: f32 = 6.0;
pub const STEREO_PHASE_MAX: f32 = 180.0;
pub const ENV_ATTACK_MIN_MS: f32 = 1.0;
pub const ENV_ATTACK_MAX_MS: f32 = 500.0;
pub const ENV_RELEASE_MIN_MS: f32 = 10.0;
pub const ENV_RELEASE_MAX_MS: f32 = 2000.0;
pub const SYNC_BEATS: [f32; 8] = [0.0, 4.0, 2.0, 1.0, 0.5, 0.25, 2.0 / 3.0, 0.75];

pub static FILTER_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Cutoff",
        min: CUTOFF_MIN,
        max: CUTOFF_MAX,
        default: 1000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Resonance",
        min: RESONANCE_MIN,
        max: RESONANCE_MAX,
        default: 0.707,
        unit: "Q",
    },
    ParamDescriptor {
        name: "Type",
        min: 0.0,
        max: 3.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Slope",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Drive",
        min: 0.0,
        max: DRIVE_MAX_DB,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "%",
    },
    ParamDescriptor {
        name: "Mod Source",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Rate",
        min: RATE_MIN_HZ,
        max: RATE_MAX_HZ,
        default: 1.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Depth",
        min: 0.0,
        max: DEPTH_MAX_OCTAVES,
        default: 0.0,
        unit: "oct",
    },
    ParamDescriptor {
        name: "Sync",
        min: 0.0,
        max: 7.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Stereo Phase",
        min: 0.0,
        max: STEREO_PHASE_MAX,
        default: 0.0,
        unit: "deg",
    },
    ParamDescriptor {
        name: "Attack",
        min: ENV_ATTACK_MIN_MS,
        max: ENV_ATTACK_MAX_MS,
        default: 10.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Release",
        min: ENV_RELEASE_MIN_MS,
        max: ENV_RELEASE_MAX_MS,
        default: 150.0,
        unit: "ms",
    },
];

const RAMP_FRAMES: u32 = 512;
const COEFFICIENT_INTERVAL: u32 = 8;

#[derive(Clone, Copy, Default)]
struct BiquadState {
    x: [f64; 2],
    y: [f64; 2],
}

impl BiquadState {
    fn process(&mut self, input: f64, c: Coefficients) -> f64 {
        let output = c.b[0] * input + c.b[1] * self.x[0] + c.b[2] * self.x[1]
            - c.a[0] * self.y[0]
            - c.a[1] * self.y[1];
        self.x = [input, self.x[0]];
        self.y = [output, self.y[0]];
        output
    }
}

pub struct FilterEffect {
    targets: [f32; param::COUNT],
    applied: [f32; param::COUNT],
    steps: [f32; param::COUNT],
    ramp_remaining: u32,
    sample_rate: f32,
    coefficients: [Coefficients; 2],
    state: [[BiquadState; 2]; 2],
    started: bool,
    phase: f64,
    bpm: f64,
    envelopes: [f32; 2],
    coefficient_countdown: u32,
    coefficients_dirty: bool,
    drive_db: f32,
    drive_gain: f32,
    drive_norm: f32,
}

impl FilterEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            44100.0
        };
        let targets = std::array::from_fn(|index| FILTER_PARAMS[index].default);
        let coefficients = response::coefficients(
            sample_rate,
            targets[param::CUTOFF],
            targets[param::RESONANCE],
            targets[param::MODE],
        );
        Self {
            targets,
            applied: targets,
            steps: [0.0; param::COUNT],
            ramp_remaining: 0,
            sample_rate,
            coefficients: [coefficients; 2],
            state: [[BiquadState::default(); 2]; 2],
            started: false,
            phase: 0.0,
            bpm: 120.0,
            envelopes: [0.0; 2],
            coefficient_countdown: 0,
            coefficients_dirty: true,
            drive_db: 0.0,
            drive_gain: 1.0,
            drive_norm: 1.0,
        }
    }

    fn set_target(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = FILTER_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = value.clamp(descriptor.min, descriptor.max);
        self.targets[index] = if matches!(
            index,
            param::MODE | param::SLOPE | param::MOD_SOURCE | param::SYNC
        ) {
            value.round()
        } else {
            value
        };
        true
    }

    fn update_coefficients(&mut self) {
        for channel in 0..2 {
            let modulation = match self.applied[param::MOD_SOURCE].round() as u8 {
                1 => {
                    let phase = self.phase
                        + channel as f64 * self.applied[param::STEREO_PHASE] as f64 / 360.0;
                    (std::f64::consts::TAU * phase).sin() as f32
                }
                2 => self.envelopes[channel].min(1.0),
                _ => 0.0,
            };
            let cutoff =
                self.applied[param::CUTOFF] * (modulation * self.applied[param::DEPTH]).exp2();
            self.coefficients[channel] = response::coefficients(
                self.sample_rate,
                cutoff,
                self.applied[param::RESONANCE],
                self.applied[param::MODE],
            );
        }
        self.coefficient_countdown = COEFFICIENT_INTERVAL;
        self.coefficients_dirty = false;
    }

    fn advance_controls(&mut self, input: &[f32]) {
        if self.ramp_remaining > 0 {
            self.coefficients_dirty = true;
            self.ramp_remaining -= 1;
            for index in 0..param::COUNT {
                self.applied[index] = if self.ramp_remaining == 0 {
                    self.targets[index]
                } else {
                    self.applied[index] + self.steps[index]
                };
            }
        }
        if self.applied[param::MOD_SOURCE] == 2.0 {
            for (channel, envelope) in self.envelopes.iter_mut().enumerate() {
                let amplitude = input.get(channel).copied().unwrap_or(0.0).abs();
                let time = if amplitude > *envelope {
                    self.applied[param::ENV_ATTACK]
                } else {
                    self.applied[param::ENV_RELEASE]
                };
                let coefficient = (-1000.0 / (time * self.sample_rate)).exp();
                *envelope = amplitude + coefficient * (*envelope - amplitude);
            }
        }
        let modulated = self.applied[param::MOD_SOURCE] > 0.0 && self.applied[param::DEPTH] > 0.0;
        if self.coefficient_countdown == 0 && (self.coefficients_dirty || modulated) {
            self.update_coefficients();
        }
        self.coefficient_countdown = self.coefficient_countdown.saturating_sub(1);
        let beats = SYNC_BEATS[self.applied[param::SYNC].round().clamp(0.0, 7.0) as usize];
        let rate = if beats > 0.0 {
            self.bpm / (60.0 * beats as f64)
        } else {
            self.applied[param::RATE] as f64
        };
        self.phase = (self.phase + rate / self.sample_rate as f64).fract();
    }
}

impl AudioEffect for FilterEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Filter
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        FILTER_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        if !self.set_target(index, value) {
            return false;
        }
        if self.started {
            for parameter in 0..param::COUNT {
                self.steps[parameter] =
                    (self.targets[parameter] - self.applied[parameter]) / RAMP_FRAMES as f32;
            }
            // Discrete choices retain stable coefficient topologies during the ramp.
            for parameter in [param::MODE, param::MOD_SOURCE, param::SYNC] {
                self.applied[parameter] = self.targets[parameter];
                self.steps[parameter] = 0.0;
            }
            self.ramp_remaining = RAMP_FRAMES;
        } else {
            self.applied = self.targets;
        }
        self.coefficient_countdown = 0;
        self.coefficients_dirty = true;
        true
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        if !self.set_target(index, value) {
            return false;
        }
        self.applied[index] = self.targets[index];
        self.steps[index] = 0.0;
        self.coefficient_countdown = 0;
        self.coefficients_dirty = true;
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.targets.get(index).copied().unwrap_or(0.0)
    }

    fn set_audio_context(&mut self, context: DeviceAudioContext) {
        if context.bpm.is_finite() && context.bpm > 0.0 {
            self.bpm = context.bpm;
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        self.started = true;
        for frame in buffer.chunks_mut(channels.max(1)) {
            self.advance_controls(frame);
            let drive = self.applied[param::DRIVE];
            if drive != self.drive_db {
                self.drive_db = drive;
                self.drive_gain = vibez_core::level::db_to_amplitude(drive);
                self.drive_norm = self.drive_gain.tanh();
            }
            let gain = self.drive_gain;
            let saturation = (drive / 6.0).min(1.0);
            for (channel, sample) in frame.iter_mut().take(2).enumerate() {
                let dry = *sample;
                let driven = if drive == 0.0 {
                    dry
                } else {
                    dry + saturation * ((dry * gain).tanh() / self.drive_norm - dry)
                };
                let first =
                    self.state[channel][0].process(driven as f64, self.coefficients[channel]);
                let second = self.state[channel][1].process(first, self.coefficients[channel]);
                let wet = first + self.applied[param::SLOPE] as f64 * (second - first);
                *sample = match self.applied[param::MIX] {
                    0.0 => dry,
                    1.0 => wet as f32,
                    mix => dry + mix * (wet as f32 - dry),
                };
            }
        }
    }

    fn reset(&mut self) {
        self.state = [[BiquadState::default(); 2]; 2];
        self.envelopes = [0.0; 2];
        self.phase = 0.0;
        self.applied = self.targets;
        self.ramp_remaining = 0;
        self.coefficient_countdown = 0;
        self.coefficients_dirty = true;
    }
}

#[cfg(test)]
#[path = "filter_tests.rs"]
mod refresh_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_passes_dc() {
        let mut fx = FilterEffect::new(44100.0);
        // DC signal should pass through a low-pass filter
        let mut buf = vec![1.0_f32; 200];
        fx.process(&mut buf, 1);
        // After settling, output should be close to 1.0
        assert!((buf[199] - 1.0).abs() < 0.01);
    }

    #[test]
    fn filter_attenuates_nyquist() {
        let mut fx = FilterEffect::new(44100.0);
        fx.set_param(0, 200.0); // very low cutoff
                                // Nyquist-ish signal (alternating +1/-1)
        let mut buf: Vec<f32> = (0..200)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        fx.process(&mut buf, 1);
        // Last samples should be heavily attenuated
        assert!(buf[199].abs() < 0.1);
    }

    #[test]
    fn filter_reset_clears_state() {
        let mut fx = FilterEffect::new(44100.0);
        let mut buf = vec![1.0; 100];
        fx.process(&mut buf, 1);
        fx.reset();
        assert_eq!(fx.state[0][0].x, [0.0; 2]);
        assert_eq!(fx.state[0][0].y, [0.0; 2]);
    }

    #[test]
    fn filter_stereo() {
        let mut fx = FilterEffect::new(44100.0);
        // interleaved stereo: L=1.0, R=0.5 repeated
        let mut buf: Vec<f32> = (0..200)
            .map(|i| if i % 2 == 0 { 1.0 } else { 0.5 })
            .collect();
        fx.process(&mut buf, 2);
        // After settling, L~1.0, R~0.5
        let last_l = buf[198];
        let last_r = buf[199];
        assert!((last_l - 1.0).abs() < 0.05);
        assert!((last_r - 0.5).abs() < 0.05);
    }

    #[test]
    fn filter_param_access() {
        let mut fx = FilterEffect::new(44100.0);
        assert!((fx.get_param(0) - 1000.0).abs() < 1e-3);
        assert!((fx.get_param(1) - 0.707).abs() < 1e-3);
        fx.set_param(0, 5000.0);
        assert!((fx.get_param(0) - 5000.0).abs() < 1e-3);
        assert!(!fx.set_param(param::COUNT, 1.0));
    }
}
