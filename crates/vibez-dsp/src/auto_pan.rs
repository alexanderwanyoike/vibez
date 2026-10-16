//! Auto-Pan: an LFO per channel turning each side's gain up and down, a
//! set phase apart. The controls follow Calf Studio Gear's Pulsator, an
//! open-source auto-panner (waveforms, stereo phase, offset), and Ableton's
//! Auto Pan (Pan and Tremolo modes, sync, retrigger). Pan mode moves the
//! sound between the speakers, Tremolo mode only turns it down. Every new
//! control defaults to the original sine/square pan, so older projects play
//! the same.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

pub mod param {
    pub const RATE: usize = 0;
    pub const DEPTH: usize = 1;
    pub const SHAPE: usize = 2;
    pub const MODE: usize = 3;
    pub const PHASE: usize = 4;
    pub const OFFSET: usize = 5;
    pub const SMOOTHING: usize = 6;
    pub const SYNC: usize = 7;
    pub const RETRIGGER: usize = 8;
    pub const COUNT: usize = 9;
}

/// The Shape choices, in order. Values between two of them blend the pair,
/// which is how the original mixed its sine into a square.
pub mod shape {
    pub const SINE: usize = 0;
    pub const SQUARE: usize = 1;
    pub const TRIANGLE: usize = 2;
    pub const SAW_UP: usize = 3;
    pub const SAW_DOWN: usize = 4;
}

/// Beats per LFO cycle for each Sync choice after "Free".
pub const SYNC_BEATS: [f64; 7] = [8.0, 4.0, 2.0, 1.0, 0.5, 0.25, 0.125];

/// Continuous controls glide this long after an edit, so a moved knob does
/// not click.
const GLIDE_SECONDS: f32 = 0.01;
const GLIDED: [usize; 4] = [param::DEPTH, param::MODE, param::PHASE, param::OFFSET];

pub static AUTO_PAN_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Rate",
        min: 0.05,
        max: 16.0,
        default: 2.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Depth",
        min: 0.0,
        max: 1.0,
        default: 0.7,
        unit: "",
    },
    ParamDescriptor {
        name: "Shape",
        min: 0.0,
        max: shape::SAW_DOWN as f32,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Mode",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Phase",
        min: 0.0,
        max: 360.0,
        default: 180.0,
        unit: "deg",
    },
    ParamDescriptor {
        name: "Offset",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Smoothing",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Sync",
        min: 0.0,
        max: SYNC_BEATS.len() as f32,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Retrigger",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
];

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(AUTO_PAN_PARAMS[index].default)
}

/// A triangle in step with a sine: 0 rising at the cycle's start, 1 a
/// quarter in.
fn triangle(x: f32) -> f32 {
    4.0 * ((x - 0.25).rem_euclid(1.0) - 0.5).abs() - 1.0
}

/// One pure shape, -1 to 1, at `theta` radians. `smoothing` widens the
/// square's and saws' edges, up to half a cycle.
fn pure(shape: usize, theta: f32, smoothing: f32) -> f32 {
    let x = (theta / std::f32::consts::TAU).rem_euclid(1.0);
    match shape {
        shape::SQUARE if smoothing > 0.0 => (triangle(x) / smoothing).clamp(-1.0, 1.0),
        // The original's square, sample for sample.
        shape::SQUARE => {
            if theta.sin() >= 0.0 {
                1.0
            } else {
                -1.0
            }
        }
        shape::TRIANGLE => triangle(x),
        shape::SAW_UP | shape::SAW_DOWN => {
            let y = (x + 0.5).rem_euclid(1.0);
            let fall = smoothing * 0.5;
            let saw = if y < 1.0 - fall {
                -1.0 + 2.0 * y / (1.0 - fall)
            } else {
                1.0 - 2.0 * (y - (1.0 - fall)) / fall
            };
            if shape == shape::SAW_UP {
                saw
            } else {
                -saw
            }
        }
        _ => theta.sin(),
    }
}

/// The LFO, -1 to 1, at `theta` radians for a Shape value.
fn wave(shape: f32, theta: f32, smoothing: f32) -> f32 {
    let low = (shape.floor() as usize).min(shape::SAW_DOWN);
    let blend = shape - low as f32;
    if blend == 0.0 {
        return pure(low, theta, smoothing);
    }
    pure(low, theta, smoothing) * (1.0 - blend)
        + pure((low + 1).min(shape::SAW_DOWN), theta, smoothing) * blend
}

/// Both channels' LFO values with the left at `theta` radians.
fn waves(params: &[f32], theta: f32) -> [f32; 2] {
    let shape = value(params, param::SHAPE);
    let smoothing = value(params, param::SMOOTHING);
    let left_theta = theta + value(params, param::OFFSET) * std::f32::consts::TAU;
    let left = wave(shape, left_theta, smoothing);
    let phase = value(params, param::PHASE);
    // Half a cycle apart, a sine and a square are each other's negative;
    // taking it that way keeps the original's samples exact.
    let right = if phase == 180.0 && shape <= shape::SQUARE as f32 {
        -left
    } else {
        wave(
            shape,
            left_theta + phase / 360.0 * std::f32::consts::TAU,
            smoothing,
        )
    };
    [left, right]
}

/// One channel's gain for an LFO value: around unity in Pan mode, at most
/// unity in Tremolo mode, with `mode` blending the two while it glides.
fn gain(depth: f32, mode: f32, wave: f32) -> f32 {
    let pan = (1.0 - wave * depth).clamp(0.0, 2.0);
    if mode == 0.0 {
        return pan;
    }
    let tremolo = 1.0 - depth * (wave + 1.0) * 0.5;
    pan * (1.0 - mode) + tremolo * mode
}

/// Each channel's gain with the LFO `cycles` into its run, as the device
/// applies it to stereo audio.
pub fn gains_at(params: &[f32], cycles: f32) -> [f32; 2] {
    let depth = value(params, param::DEPTH);
    let mode = value(params, param::MODE);
    waves(params, cycles * std::f32::consts::TAU).map(|wave| gain(depth, mode, wave))
}

/// LFO cycles per second, synced or free.
pub fn rate_hz(params: &[f32], bpm: f64) -> f64 {
    let sync = value(params, param::SYNC).round() as usize;
    if sync == 0 {
        f64::from(value(params, param::RATE))
    } else {
        bpm.max(1.0) / 60.0 / SYNC_BEATS[sync.min(SYNC_BEATS.len()) - 1]
    }
}

pub struct AutoPanEffect {
    /// Where each control is heading.
    params: [f32; param::COUNT],
    /// What each control is now; only the glided ones lag.
    current: [f32; param::COUNT],
    glide_left: u32,
    started: bool,
    sample_rate: f32,
    bpm: f64,
    /// LFO phase in radians, as the original kept it.
    phase: f32,
}

impl AutoPanEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 1.0 {
            sample_rate
        } else {
            48_000.0
        };
        let params = std::array::from_fn(|index| AUTO_PAN_PARAMS[index].default);
        Self {
            params,
            current: params,
            glide_left: 0,
            started: false,
            sample_rate,
            bpm: vibez_core::constants::DEFAULT_BPM,
            phase: 0.0,
        }
    }

    fn glide(&mut self) {
        if self.glide_left == 0 {
            return;
        }
        let steps = self.glide_left as f32;
        for index in GLIDED {
            self.current[index] += (self.params[index] - self.current[index]) / steps;
        }
        self.glide_left -= 1;
        if self.glide_left == 0 {
            self.current = self.params;
        }
    }
}

impl AudioEffect for AutoPanEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn set_audio_context(&mut self, context: DeviceAudioContext) {
        if context.bpm.is_finite() && context.bpm > 0.0 {
            self.bpm = context.bpm;
        }
        // Locked to the song, the LFO is wherever the song position puts it,
        // through loops and jumps alike.
        if context.playing && self.params[param::RETRIGGER] >= 0.5 && context.sample_rate > 0 {
            let sync = self.params[param::SYNC].round() as usize;
            let cycles = if sync == 0 {
                context.seconds() * f64::from(self.params[param::RATE])
            } else {
                context.beats() / SYNC_BEATS[sync - 1]
            };
            self.phase = (cycles.rem_euclid(1.0) * std::f64::consts::TAU) as f32;
        }
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn effect_type(&self) -> EffectType {
        EffectType::AutoPan
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        AUTO_PAN_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = AUTO_PAN_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = match index {
            param::SYNC => value.round(),
            param::MODE | param::RETRIGGER => f32::from(value >= 0.5),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        self.params[index] = value;
        if self.started && GLIDED.contains(&index) {
            self.glide_left = (self.sample_rate * GLIDE_SECONDS).round().max(1.0) as u32;
        } else {
            self.current[index] = value;
        }
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        self.started = true;
        let phase_inc =
            rate_hz(&self.params, self.bpm) as f32 * std::f32::consts::TAU / self.sample_rate;
        for frame in buffer.chunks_exact_mut(channels) {
            self.glide();
            let depth = self.current[param::DEPTH];
            let mode = self.current[param::MODE];
            let waves = waves(&self.current, self.phase);
            if channels == 1 {
                // On mono, the original fell back to a tremolo.
                let pan = 1.0 - (waves[0] * depth).abs();
                let tremolo = 1.0 - depth * (waves[0] + 1.0) * 0.5;
                frame[0] *= if mode == 0.0 {
                    pan
                } else {
                    pan * (1.0 - mode) + tremolo * mode
                };
            } else {
                for (sample, wave) in frame.iter_mut().zip(waves) {
                    *sample *= gain(depth, mode, wave);
                }
            }
            for sample in frame.iter_mut().take(2) {
                if !sample.is_finite() {
                    *sample = 0.0;
                }
            }

            self.phase += phase_inc;
            if self.phase > std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.current = self.params;
        self.glide_left = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_zero_is_identity() {
        let mut fx = AutoPanEffect::new(44_100.0);
        fx.set_param(1, 0.0);
        let orig = vec![0.5_f32; 64];
        let mut buf = orig.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn creates_stereo_motion() {
        let mut fx = AutoPanEffect::new(44_100.0);
        fx.set_param(0, 1.0);
        fx.set_param(1, 1.0);
        let mut buf = vec![0.5_f32; 44_100 * 2];
        fx.process(&mut buf, 2);
        let mut any_diff = false;
        for frame in 0..44_100 {
            if (buf[frame * 2] - buf[frame * 2 + 1]).abs() > 0.01 {
                any_diff = true;
                break;
            }
        }
        assert!(any_diff);
    }
}

#[cfg(test)]
#[path = "auto_pan_legacy_tests.rs"]
mod legacy_tests;

#[cfg(test)]
#[path = "auto_pan_controls_tests.rs"]
mod controls_tests;
