//! Stereo reverb on Jezar's public-domain Freeverb: eight damped combs into
//! four allpasses per side. Around that tank sit a pre-delay (free or
//! synced), the allpass diffusion, stereo width, low and high cuts on the
//! wet signal, and Freeverb's freeze. At their defaults these leave the
//! original sound untouched, so older projects play the same.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::butterworth_filter::ButterworthFilter;
use crate::effect::AudioEffect;

pub mod param {
    pub const ROOM_SIZE: usize = 0;
    pub const DAMPING: usize = 1;
    pub const MIX: usize = 2;
    pub const PREDELAY_MS: usize = 3;
    pub const PREDELAY_SYNC: usize = 4;
    pub const WIDTH: usize = 5;
    pub const DIFFUSION: usize = 6;
    pub const LOW_CUT_HZ: usize = 7;
    pub const HIGH_CUT_HZ: usize = 8;
    pub const FREEZE: usize = 9;
    pub const COUNT: usize = 10;
}

/// Synced pre-delays in beats, after "Free": a 64th note up to a quarter.
pub const PREDELAY_SYNC_BEATS: [f64; 7] = [1.0 / 16.0, 1.0 / 8.0, 0.25, 0.375, 0.5, 0.75, 1.0];
const MAX_PREDELAY_SECONDS: f64 = 3.0;
const LOW_CUT_OFF_HZ: f32 = 20.0;
const HIGH_CUT_OFF_HZ: f32 = 20_000.0;
pub const RAMP_FRAMES: u32 = 512;

pub static REVERB_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Room Size",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Damping",
        min: 0.0,
        max: 1.0,
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
        name: "Pre-delay",
        min: 0.0,
        max: 250.0,
        default: 0.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Pre-delay Sync",
        min: 0.0,
        max: PREDELAY_SYNC_BEATS.len() as f32,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Width",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Diffusion",
        min: 0.0,
        max: 0.9,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Low Cut",
        min: LOW_CUT_OFF_HZ,
        max: 1000.0,
        default: LOW_CUT_OFF_HZ,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "High Cut",
        min: 1000.0,
        max: HIGH_CUT_OFF_HZ,
        default: HIGH_CUT_OFF_HZ,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Freeze",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
];

// Freeverb-style comb filter delay lengths (in samples at 44100 Hz)
const COMB_LENGTHS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_LENGTHS: [usize; 4] = [556, 441, 341, 225];

// Freeverb tuning constants
const FIXED_GAIN: f32 = 0.015;
const SCALE_WET: f32 = 3.0;
const SCALE_ROOM: f32 = 0.28;
const OFFSET_ROOM: f32 = 0.7;
const SCALE_DAMP: f32 = 0.4;

/// Seconds for a tail to fall 60 dB at this room size: the combs' average
/// loop, repeated until its feedback has taken off 60 dB.
pub fn decay_seconds(room_size: f32) -> f32 {
    let average = COMB_LENGTHS.iter().sum::<usize>() as f32 / COMB_LENGTHS.len() as f32 / 44_100.0;
    let feedback = room_size.clamp(0.0, 1.0) * SCALE_ROOM + OFFSET_ROOM;
    -3.0 * average / feedback.log10()
}

/// The room size whose tail lasts `seconds`; the inverse of
/// [`decay_seconds`], held to the sizes the device has.
pub fn room_for_decay(seconds: f32) -> f32 {
    let average = COMB_LENGTHS.iter().sum::<usize>() as f32 / COMB_LENGTHS.len() as f32 / 44_100.0;
    let feedback = 10.0_f32.powf(-3.0 * average / seconds.max(1e-3));
    ((feedback - OFFSET_ROOM) / SCALE_ROOM).clamp(0.0, 1.0)
}

/// Zero for a value too small to hear, so a fading tail stops instead of
/// crawling through denormal numbers, which are many times slower to
/// compute with. Freeverb's own `undenormalise` does the same.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-15 {
        0.0
    } else {
        value
    }
}

struct CombFilter {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    filter_store: f32,
}

impl CombFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            pos: 0,
            feedback: 0.5,
            damp1: 0.5,
            damp2: 0.5,
            filter_store: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.pos];
        self.filter_store = flush(output * self.damp2 + self.filter_store * self.damp1);
        self.buffer[self.pos] = flush(input + self.filter_store * self.feedback);
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.filter_store = 0.0;
        self.pos = 0;
    }
}

struct AllPassFilter {
    buffer: Vec<f32>,
    pos: usize,
}

impl AllPassFilter {
    fn new(size: usize) -> Self {
        Self {
            buffer: vec![0.0; size.max(1)],
            pos: 0,
        }
    }

    fn process(&mut self, input: f32, feedback: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        let output = -input + buffered;
        self.buffer[self.pos] = flush(input + buffered * feedback);
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
    }
}

/// A control moving linearly to its target over [`RAMP_FRAMES`].
#[derive(Clone, Copy)]
struct Ramp {
    value: f32,
    step: f32,
    remaining: u32,
}

impl Ramp {
    fn at(value: f32) -> Self {
        Self {
            value,
            step: 0.0,
            remaining: 0,
        }
    }

    fn toward(&mut self, target: f32, frames: u32) {
        self.step = (target - self.value) / frames as f32;
        self.remaining = frames;
    }

    /// Moves a frame on, saying whether the value changed.
    fn advance(&mut self, target: f32) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        self.value = if self.remaining == 0 {
            target
        } else {
            self.value + self.step
        };
        true
    }
}

/// Freeverb-inspired stereo reverb.
pub struct ReverbEffect {
    sample_rate: f32,
    params: [f32; param::COUNT],
    started: bool,
    bpm: f64,
    combs_l: Vec<CombFilter>,
    combs_r: Vec<CombFilter>,
    allpasses_l: Vec<AllPassFilter>,
    allpasses_r: Vec<AllPassFilter>,
    mix: Ramp,
    width: Ramp,
    freeze: Ramp,
    /// Pre-delay in samples, gliding to a new length rather than jumping.
    predelay: Ramp,
    predelay_line: Vec<f32>,
    predelay_pos: usize,
    low_cut: ButterworthFilter,
    high_cut: ButterworthFilter,
}

impl ReverbEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        let scale = sample_rate / 44100.0;
        let combs = |offset: f32| {
            COMB_LENGTHS
                .iter()
                .map(|&len| CombFilter::new(((len as f32) * scale + offset) as usize))
                .collect()
        };
        let allpasses = |offset: f32| {
            ALLPASS_LENGTHS
                .iter()
                .map(|&len| AllPassFilter::new(((len as f32) * scale + offset) as usize))
                .collect()
        };
        let params = std::array::from_fn(|index| REVERB_PARAMS[index].default);
        let mut reverb = Self {
            sample_rate,
            params,
            started: false,
            bpm: vibez_core::constants::DEFAULT_BPM,
            combs_l: combs(0.0),
            combs_r: combs(23.0),
            allpasses_l: allpasses(0.0),
            allpasses_r: allpasses(23.0),
            mix: Ramp::at(params[param::MIX]),
            width: Ramp::at(params[param::WIDTH]),
            freeze: Ramp::at(0.0),
            predelay: Ramp::at(0.0),
            predelay_line: vec![0.0; (MAX_PREDELAY_SECONDS * f64::from(sample_rate)) as usize + 2],
            predelay_pos: 0,
            low_cut: ButterworthFilter::EMPTY,
            high_cut: ButterworthFilter::EMPTY,
        };
        reverb.update_combs();
        reverb
    }

    fn value(&self, index: usize) -> f32 {
        self.params[index]
    }

    fn update_combs(&mut self) {
        let freeze = self.freeze.value;
        let feedback = self.value(param::ROOM_SIZE) * SCALE_ROOM + OFFSET_ROOM;
        let feedback = feedback + (1.0 - feedback) * freeze;
        let damp1 = self.value(param::DAMPING) * SCALE_DAMP * (1.0 - freeze);
        let damp2 = 1.0 - damp1;
        for comb in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            comb.feedback = feedback;
            comb.damp1 = damp1;
            comb.damp2 = damp2;
        }
    }

    fn predelay_target(&self) -> f32 {
        let sync = self.value(param::PREDELAY_SYNC).round() as usize;
        let seconds = if sync == 0 {
            f64::from(self.value(param::PREDELAY_MS)) / 1000.0
        } else {
            PREDELAY_SYNC_BEATS[sync - 1] * 60.0 / self.bpm.max(1.0)
        };
        (seconds.min(MAX_PREDELAY_SECONDS) * f64::from(self.sample_rate)) as f32
    }

    fn retarget_predelay(&mut self, immediate: bool) {
        let target = self.predelay_target();
        if immediate {
            self.predelay = Ramp::at(target);
        } else if target != self.predelay.value {
            self.predelay.toward(target, RAMP_FRAMES * 8);
        }
    }

    fn configure_filters(&mut self) {
        self.low_cut
            .configure(self.sample_rate, self.value(param::LOW_CUT_HZ), true);
        self.high_cut
            .configure(self.sample_rate, self.value(param::HIGH_CUT_HZ), false);
    }

    fn edit(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = REVERB_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = match index {
            param::PREDELAY_SYNC => value.round(),
            param::FREEZE => f32::from(value >= 0.5),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        let was = self.params[index];
        self.params[index] = value;
        let immediate = automated || !self.started;
        match index {
            param::ROOM_SIZE | param::DAMPING => self.update_combs(),
            param::MIX if immediate => self.mix = Ramp::at(value),
            param::MIX => self.mix.toward(value, RAMP_FRAMES),
            param::WIDTH if immediate => self.width = Ramp::at(value),
            param::WIDTH => self.width.toward(value, RAMP_FRAMES),
            param::FREEZE if immediate => {
                self.freeze = Ramp::at(value);
                self.update_combs();
            }
            param::FREEZE => self.freeze.toward(value, RAMP_FRAMES),
            param::PREDELAY_MS | param::PREDELAY_SYNC => self.retarget_predelay(immediate),
            param::LOW_CUT_HZ => {
                if was <= LOW_CUT_OFF_HZ && value > LOW_CUT_OFF_HZ {
                    self.low_cut.reset();
                }
                self.configure_filters();
            }
            param::HIGH_CUT_HZ => {
                if was >= HIGH_CUT_OFF_HZ && value < HIGH_CUT_OFF_HZ {
                    self.high_cut.reset();
                }
                self.configure_filters();
            }
            _ => {}
        }
        true
    }

    /// The mono tank input after the pre-delay, read between samples while
    /// the length glides.
    fn predelayed(&mut self, input: f32) -> f32 {
        let length = self.predelay_line.len();
        self.predelay_line[self.predelay_pos] = input;
        let delay = self.predelay.value;
        let whole = delay as usize;
        let fraction = delay - whole as f32;
        let at = |back: usize| {
            self.predelay_line[(self.predelay_pos + length - back.min(length - 1)) % length]
        };
        let output = at(whole) * (1.0 - fraction) + at(whole + 1) * fraction;
        self.predelay_pos = (self.predelay_pos + 1) % length;
        output
    }
}

impl AudioEffect for ReverbEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn set_audio_context(&mut self, context: DeviceAudioContext) {
        if context.bpm.is_finite() && context.bpm > 0.0 && context.bpm != self.bpm {
            self.bpm = context.bpm;
            let immediate = !self.started;
            self.retarget_predelay(immediate);
        }
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Reverb
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        REVERB_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.edit(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.edit(index, value, true)
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let ch = channels.min(2);
        let diffusion = self.value(param::DIFFUSION);
        let low_cut = self.value(param::LOW_CUT_HZ) > LOW_CUT_OFF_HZ;
        let high_cut = self.value(param::HIGH_CUT_HZ) < HIGH_CUT_OFF_HZ;
        for frame in buffer.chunks_exact_mut(channels) {
            self.started = true;
            self.mix.advance(self.value(param::MIX));
            self.width.advance(self.value(param::WIDTH));
            if self.freeze.advance(self.value(param::FREEZE)) {
                self.update_combs();
            }
            let predelay_target = self.predelay_target();
            self.predelay.advance(predelay_target);

            let finite = |sample: f32| if sample.is_finite() { sample } else { 0.0 };
            let dry_l = finite(frame[0]);
            let dry_r = if ch >= 2 { finite(frame[1]) } else { dry_l };

            // Attenuate input before comb filters (critical Freeverb tuning)
            let mut input = (dry_l + dry_r) * FIXED_GAIN;
            if self.freeze.value > 0.0 {
                input *= 1.0 - self.freeze.value;
            }
            if self.predelay.value > 0.0 || self.predelay.remaining > 0 {
                input = self.predelayed(input);
            } else {
                self.predelayed(input);
            }

            let mut wet_l = 0.0_f32;
            let mut wet_r = 0.0_f32;
            for comb in &mut self.combs_l {
                wet_l += comb.process(input);
            }
            for comb in &mut self.combs_r {
                wet_r += comb.process(input);
            }
            for ap in &mut self.allpasses_l {
                wet_l = ap.process(wet_l, diffusion);
            }
            for ap in &mut self.allpasses_r {
                wet_r = ap.process(wet_r, diffusion);
            }
            if low_cut {
                wet_l = flush(self.low_cut.process(wet_l, 0));
                wet_r = flush(self.low_cut.process(wet_r, 1));
            }
            if high_cut {
                wet_l = flush(self.high_cut.process(wet_l, 0));
                wet_r = flush(self.high_cut.process(wet_r, 1));
            }

            // Freeverb wet gain with stereo width cross-feed
            let mix = self.mix.value;
            let width = self.width.value;
            let wet1 = mix * SCALE_WET * (width * 0.5 + 0.5);
            let wet2 = mix * SCALE_WET * ((1.0 - width) * 0.5);
            let dry = 1.0 - mix;
            frame[0] = dry_l * dry + wet_l * wet1 + wet_r * wet2;
            if ch >= 2 {
                frame[1] = dry_r * dry + wet_r * wet1 + wet_l * wet2;
            }
        }
    }

    fn reset(&mut self) {
        for comb in self.combs_l.iter_mut().chain(self.combs_r.iter_mut()) {
            comb.reset();
        }
        for ap in self
            .allpasses_l
            .iter_mut()
            .chain(self.allpasses_r.iter_mut())
        {
            ap.reset();
        }
        self.predelay_line.fill(0.0);
        self.predelay_pos = 0;
        self.low_cut.reset();
        self.high_cut.reset();
        self.started = false;
        self.mix = Ramp::at(self.value(param::MIX));
        self.width = Ramp::at(self.value(param::WIDTH));
        self.freeze = Ramp::at(self.value(param::FREEZE));
        self.update_combs();
        self.retarget_predelay(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_silence_in_silence_out() {
        let mut fx = ReverbEffect::new(44100.0);
        let mut buf = vec![0.0_f32; 200];
        fx.process(&mut buf, 2);
        for s in &buf {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn reverb_produces_tail() {
        let mut fx = ReverbEffect::new(44100.0);
        fx.set_param(2, 1.0); // wet only

        // Impulse
        let mut buf = vec![0.0_f32; 4];
        buf[0] = 1.0;
        buf[1] = 1.0;
        fx.process(&mut buf, 2);

        // Now process more silence - should have reverb tail
        let mut tail = vec![0.0_f32; 4000];
        fx.process(&mut tail, 2);

        let energy: f32 = tail.iter().map(|s| s * s).sum();
        assert!(energy > 0.01, "Reverb should produce a tail");
    }

    #[test]
    fn reverb_dry_when_mix_zero() {
        let mut fx = ReverbEffect::new(44100.0);
        fx.set_param(2, 0.0);
        let mut buf = vec![0.5, -0.3, 0.8, -0.1];
        let orig = buf.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn reverb_reset_clears() {
        let mut fx = ReverbEffect::new(44100.0);
        let mut buf = vec![1.0; 100];
        fx.process(&mut buf, 2);
        fx.reset();

        let mut silence = vec![0.0_f32; 200];
        fx.process(&mut silence, 2);
        for s in &silence {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn a_fading_tail_reaches_silence_without_slow_denormal_numbers() {
        let mut fx = ReverbEffect::new(48_000.0);
        fx.set_param(2, 1.0);
        let mut buf = vec![0.0_f32; 1024];
        buf[0] = 1.0;
        buf[1] = 1.0;
        fx.process(&mut buf, 2);
        let mut denormals = 0;
        for _ in 0..(20 * 48_000 / 512) {
            let mut silence = vec![0.0_f32; 1024];
            fx.process(&mut silence, 2);
            denormals += silence.iter().filter(|s| s.is_subnormal()).count();
        }
        assert_eq!(denormals, 0);
    }

    #[test]
    fn reverb_param_access() {
        let mut fx = ReverbEffect::new(44100.0);
        assert!((fx.get_param(0) - 0.5).abs() < 1e-3);
        assert!((fx.get_param(1) - 0.5).abs() < 1e-3);
        assert!((fx.get_param(2) - 0.3).abs() < 1e-3);
        fx.set_param(0, 0.8);
        assert!((fx.get_param(0) - 0.8).abs() < 1e-3);
        assert!(!fx.set_param(param::COUNT, 1.0));
    }
}

#[cfg(test)]
#[path = "reverb_legacy_tests.rs"]
mod legacy_tests;

#[cfg(test)]
#[path = "reverb_controls_tests.rs"]
mod controls_tests;
