//! Admits Delay parameter edits and processes finite linear feedback echoes.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

#[path = "delay_history.rs"]
mod history;
#[path = "delay_time.rs"]
mod time;
#[path = "delay_ramp.rs"]
mod ramp;
#[path = "delay_filter.rs"]
mod filter;
#[path = "delay_duck.rs"]
mod duck;
use duck::Duck;
use filter::FeedbackFilters;
use history::DelayHistory;
use ramp::ScalarRamp;
use time::DelayTime;

pub mod param {
    pub const TIME_MS: usize = 0;
    pub const FEEDBACK: usize = 1;
    pub const MIX: usize = 2;
    pub const SYNC: usize = 3;
    pub const LEFT_DIVISION: usize = 4;
    pub const LEFT_MODIFIER: usize = 5;
    pub const ROUTING: usize = 6;
    pub const LINK: usize = 7;
    pub const RIGHT_TIME_MS: usize = 8;
    pub const RIGHT_DIVISION: usize = 9;
    pub const RIGHT_MODIFIER: usize = 10;
    pub const FEEDBACK_HP_ENABLED: usize = 11;
    pub const FEEDBACK_HP_HZ: usize = 12;
    pub const FEEDBACK_LP_ENABLED: usize = 13;
    pub const FEEDBACK_LP_HZ: usize = 14;
    pub const DUCK: usize = 15;
    pub const TIME_CHANGE: usize = 16;
    pub const COUNT: usize = 17;
}

pub static DELAY_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Time",
        min: 1.0,
        max: 2000.0,
        default: 500.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Feedback",
        min: 0.0,
        max: 0.95,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 0.3,
        unit: "",
    },
    ParamDescriptor {
        name: "Sync",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Left Division",
        min: 0.0,
        max: 6.0,
        default: 4.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Left Modifier",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Routing",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "L/R Link",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Right Free Time",
        min: 1.0,
        max: 2000.0,
        default: 500.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Right Division",
        min: 0.0,
        max: 6.0,
        default: 4.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Right Modifier",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Feedback HP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Feedback HP Frequency",
        min: 20.0,
        max: 1000.0,
        default: 120.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Feedback LP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Feedback LP Frequency",
        min: 1000.0,
        max: 20000.0,
        default: 8000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Duck",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Time Change",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
];

/// Stereo delay effect with feedback.
pub struct DelayEffect {
    targets: [f32; param::COUNT],
    history: DelayHistory,
    timing: DelayTime,
    processed_frames: u64,
    mix: ScalarRamp,
    feedback: ScalarRamp,
    routing: ScalarRamp,
    filters: FeedbackFilters,
    duck: Duck,
}

impl DelayEffect {
    pub fn new(sample_rate: f32) -> Self {
        let history = DelayHistory::new(sample_rate);
        let filters = if history.is_valid() {
            FeedbackFilters::new(sample_rate, DELAY_PARAMS[param::FEEDBACK_HP_HZ].default,
                DELAY_PARAMS[param::FEEDBACK_LP_HZ].default)
        } else {
            FeedbackFilters::default()
        };
        let duck = if history.is_valid() { Duck::new(sample_rate) } else { Duck::default() };
        Self {
            targets: std::array::from_fn(|index| DELAY_PARAMS[index].default),
            history,
            timing: DelayTime::new(),
            processed_frames: 0,
            mix: ScalarRamp::new(DELAY_PARAMS[param::MIX].default),
            feedback: ScalarRamp::new(DELAY_PARAMS[param::FEEDBACK].default),
            routing: ScalarRamp::new(DELAY_PARAMS[param::ROUTING].default),
            filters,
            duck,
        }
    }

    fn finite_or_zero(sample: f32) -> f32 {
        if sample.is_finite() {
            sample
        } else {
            0.0
        }
    }

    fn set_target(&mut self, index: usize, value: f32, automated: bool) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(descriptor) = DELAY_PARAMS.get(index) else {
            return false;
        };
        let target = match index {
            param::SYNC | param::LINK | param::FEEDBACK_HP_ENABLED | param::FEEDBACK_LP_ENABLED => {
                if value >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            param::LEFT_DIVISION
            | param::LEFT_MODIFIER
            | param::ROUTING
            | param::RIGHT_DIVISION
            | param::RIGHT_MODIFIER
            | param::TIME_CHANGE => {
                // Ordinals have one zero representation for stored/preset matching.
                value.round().clamp(descriptor.min, descriptor.max) as u32 as f32
            }
            _ => value.clamp(descriptor.min, descriptor.max),
        };
        if !automated && self.targets[index].to_bits() == target.to_bits() {
            return true;
        }
        self.targets[index] = target;
        if index == param::MIX {
            self.mix.edit(target, automated, (self.history.sample_rate() as usize).div_ceil(100));
        } else if index == param::FEEDBACK {
            self.feedback.edit(target, automated, (self.history.sample_rate() as usize).div_ceil(100));
        } else if index == param::DUCK {
            self.duck.edit(target, automated, (self.history.sample_rate() as usize).div_ceil(100));
        } else if index == param::ROUTING {
            self.routing.edit(target, automated, (self.history.sample_rate() as usize).div_ceil(50));
        }
        if self.history.is_valid() {
            match index {
                param::FEEDBACK_HP_ENABLED => self.filters.set_high_pass_enabled(target, automated,
                    (self.history.sample_rate() as usize).div_ceil(100)),
                param::FEEDBACK_LP_ENABLED => self.filters.set_low_pass_enabled(target, automated,
                    (self.history.sample_rate() as usize).div_ceil(100)),
                param::FEEDBACK_HP_HZ => self.filters.set_high_pass_hz(target, automated),
                param::FEEDBACK_LP_HZ => self.filters.set_low_pass_hz(target, automated),
                _ => {}
            }
        }
        self.timing.parameter_edited(index, &self.targets, self.history.sample_rate(), automated);
        true
    }
}

impl AudioEffect for DelayEffect {
    fn delay_snapshot(&self) -> Option<vibez_core::delay_state::DelayState> {
        if self.processed_frames == 0 || !self.history.is_valid() {
            return None;
        }
        let (bpm, lines) = self.timing.snapshot(&self.targets, self.history.sample_rate());
        let (filters, filters_moving) = self.filters.snapshot();
        let reference_moving = lines.iter().any(|line| line.moving) || filters_moving
            || self.feedback.moving() || self.routing.moving();
        Some(vibez_core::delay_state::DelayState {
            mix: self.mix.current(), feedback: self.feedback.current(), routing: self.routing.current(),
            filters, duck_attenuation_db: self.duck.attenuation_db(), reference_moving,
            sample_rate: self.history.activation_sample_rate()?, bpm,
            processed_frames: self.processed_frames, targets: self.targets, lines,
        })
    }

    fn supports_level_meter(&self) -> bool {
        true
    }

    fn set_audio_context(&mut self, context: vibez_core::audio_context::DeviceAudioContext) {
        self.timing.set_bpm(context.bpm, &self.targets, self.history.sample_rate());
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        self.history.activation_sample_rate()
    }

    fn processing_configuration_valid(&self) -> bool {
        self.history.is_valid()
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Delay
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        DELAY_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, true)
    }

    fn get_param(&self, index: usize) -> f32 {
        self.targets.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        if !self.history.is_valid() {
            for frame in buffer.chunks_exact_mut(channels) {
                for sample in frame.iter_mut().take(2) {
                    if !sample.is_finite() {
                        *sample = 0.0;
                    }
                }
            }
            return;
        }

        let ch = channels;
        let frames = buffer.len() / ch;
        if frames == 0 {
            return;
        }
        let sample_rate = self.history.sample_rate();
        self.timing.begin_audio(&self.targets, sample_rate);
        let fade_frames = (sample_rate as usize).div_ceil(50);
        let use_glide = self.targets[param::TIME_CHANGE] >= 0.5;

        for frame in 0..frames {
            let [left, right] = self.timing.next_frame(fade_frames, sample_rate, use_glide);
            let (delayed_l, delayed_r) = self.history.read(left, right);
            let mix = self.mix.next_frame();
            let feedback = self.feedback.next_frame();
            let routing = self.routing.next_frame();
            // Exact endpoints retain the original channel samples without extra blend rounding.
            let (routed_l, routed_r) = if routing == 0.0 {
                (delayed_l, delayed_r)
            } else if routing == 1.0 {
                (delayed_r, delayed_l)
            } else {
                let cross = f64::from(routing);
                let own = 1.0 - cross;
                (
                    (f64::from(delayed_l) * own + f64::from(delayed_r) * cross) as f32,
                    (f64::from(delayed_r) * own + f64::from(delayed_l) * cross) as f32,
                )
            };
            let [filtered_l, filtered_r] = self.filters.process([routed_l, routed_r]);

            let dry_l = Self::finite_or_zero(buffer[frame * ch]);
            // Missing mono input is zero, but its feedback continues while unobserved.
            let dry_r = if ch >= 2 {
                Self::finite_or_zero(buffer[frame * ch + 1])
            } else {
                0.0
            };
            let [wet_l, wet_r] = self.duck.process([dry_l, dry_r], [delayed_l, delayed_r]);
            let write_l = Self::finite_or_zero(dry_l + filtered_l * feedback);
            let write_r = Self::finite_or_zero(dry_r + filtered_r * feedback);
            buffer[frame * ch] = Self::finite_or_zero(dry_l * (1.0 - mix) + wet_l * mix);
            if ch >= 2 {
                buffer[frame * ch + 1] =
                    Self::finite_or_zero(dry_r * (1.0 - mix) + wet_r * mix);
            }

            self.history.write_and_advance(write_l, write_r);
        }
        self.processed_frames = self.processed_frames.saturating_add(frames as u64);
    }

    fn reset(&mut self) {
        self.processed_frames = 0;
        self.history.reset();
        self.timing.reset(&self.targets, self.history.sample_rate());
        self.mix.reset(self.targets[param::MIX]);
        self.feedback.reset(self.targets[param::FEEDBACK]);
        self.routing.reset(self.targets[param::ROUTING]);
        self.filters.reset(self.targets[param::FEEDBACK_HP_ENABLED], self.targets[param::FEEDBACK_LP_ENABLED]);
        self.duck.reset(self.targets[param::DUCK]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_dry_only_when_mix_zero() {
        let mut fx = DelayEffect::new(44100.0);
        fx.set_param(2, 0.0); // mix = 0
        let mut buf = vec![1.0, 0.5, 0.0, 0.0];
        let orig = buf.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn delay_produces_echo() {
        let mut fx = DelayEffect::new(1000.0); // 1000 Hz sample rate for easy math
        fx.set_param(0, 10.0); // 10ms = 10 samples at 1kHz
        fx.set_param(1, 0.0); // no feedback
        fx.set_param(2, 1.0); // wet only

        // Send impulse then silence
        let mut buf = vec![0.0; 100];
        buf[0] = 1.0; // impulse on L
        buf[1] = 1.0; // impulse on R
        fx.process(&mut buf, 2);

        // At frame 10 (sample indices 20,21) we should see the echo
        assert!((buf[20] - 1.0).abs() < 1e-6);
        assert!((buf[21] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn delay_feedback_produces_repeats() {
        let mut fx = DelayEffect::new(1000.0);
        fx.set_param(0, 5.0); // 5ms = 5 samples
        fx.set_param(1, 0.5); // feedback
        fx.set_param(2, 1.0); // wet only

        let mut buf = vec![0.0; 60]; // 30 frames stereo
        buf[0] = 1.0;
        buf[1] = 1.0;
        fx.process(&mut buf, 2);

        // First echo at frame 5
        assert!((buf[10] - 1.0).abs() < 1e-6);
        // Second echo at frame 10 (with 0.5 feedback)
        assert!((buf[20] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn delay_param_access() {
        let mut fx = DelayEffect::new(44100.0);
        assert!((fx.get_param(0) - 500.0).abs() < 1e-3);
        assert!((fx.get_param(1) - 0.5).abs() < 1e-3);
        assert!((fx.get_param(2) - 0.3).abs() < 1e-3);
        fx.set_param(0, 100.0);
        assert!((fx.get_param(0) - 100.0).abs() < 1e-3);
        // Appended indices 3..16 are valid; the original three values stay unchanged.
        assert!(!fx.set_param(17, 1.0));
    }

    #[test]
    fn delay_reset() {
        let mut fx = DelayEffect::new(44100.0);
        let mut buf = vec![1.0; 100];
        fx.process(&mut buf, 2);
        fx.reset();
        assert_eq!(fx.history.write_position(), 0);
        assert!(fx.history.left_samples().iter().all(|&s| s == 0.0));
    }
}
