//! The synth's controls. The first eight are the original synth's, at their
//! original indices; everything after them defaults to off or to the
//! original sound, so older projects play the same.

use vibez_core::effect::ParamDescriptor;

pub mod param {
    pub const WAVEFORM: usize = 0;
    pub const ATTACK: usize = 1;
    pub const DECAY: usize = 2;
    pub const SUSTAIN: usize = 3;
    pub const RELEASE: usize = 4;
    pub const CUTOFF: usize = 5;
    pub const RESONANCE: usize = 6;
    pub const VOLUME: usize = 7;
    pub const OCTAVE: usize = 8;
    pub const DETUNE: usize = 9;
    pub const WAVEFORM_2: usize = 10;
    pub const OCTAVE_2: usize = 11;
    pub const DETUNE_2: usize = 12;
    pub const LEVEL_2: usize = 13;
    pub const SUB: usize = 14;
    pub const NOISE: usize = 15;
    pub const FILTER_TYPE: usize = 16;
    pub const VOICE_MODE: usize = 17;
    pub const VOICES: usize = 18;
    pub const GLIDE: usize = 19;
    pub const LFO_SHAPE: usize = 20;
    pub const LFO_TARGET: usize = 21;
    pub const LFO_RATE: usize = 22;
    pub const LFO_AMOUNT: usize = 23;
    pub const FILTER_ENV: usize = 24;
    pub const FILTER_ATTACK: usize = 25;
    pub const FILTER_DECAY: usize = 26;
    pub const FILTER_SUSTAIN: usize = 27;
    pub const FILTER_RELEASE: usize = 28;
    pub const COUNT: usize = 29;
}

pub mod filter_type {
    pub const LP12: usize = 0;
    pub const LP24: usize = 1;
    pub const HP12: usize = 2;
    pub const BP12: usize = 3;
}

pub mod voice_mode {
    pub const POLY: usize = 0;
    pub const MONO: usize = 1;
    /// One voice; a note played while another is held changes pitch
    /// without starting the envelopes again.
    pub const LEGATO: usize = 2;
}

pub mod lfo_shape {
    pub const SINE: usize = 0;
    pub const TRIANGLE: usize = 1;
    pub const SQUARE: usize = 2;
    pub const SAW: usize = 3;
}

pub mod lfo_target {
    pub const CUTOFF: usize = 0;
    pub const PITCH: usize = 1;
    pub const VOLUME: usize = 2;
}

/// The LFO at full amount moves the cutoff this many octaves either way.
pub const LFO_CUTOFF_OCTAVES: f32 = 4.0;
/// The LFO at full amount moves the pitch this many semitones either way.
pub const LFO_PITCH_SEMITONES: f32 = 12.0;
pub const MAX_VOICES: usize = 8;

const fn descriptor(
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
) -> ParamDescriptor {
    ParamDescriptor {
        name,
        min,
        max,
        default,
        unit,
    }
}

pub static SYNTH_PARAMS: &[ParamDescriptor] = &[
    descriptor("Waveform", 0.0, 3.0, 1.0, ""),
    descriptor("Attack", 0.001, 2.0, 0.01, "s"),
    descriptor("Decay", 0.001, 2.0, 0.1, "s"),
    descriptor("Sustain", 0.0, 1.0, 0.7, ""),
    descriptor("Release", 0.001, 5.0, 0.3, "s"),
    descriptor("Filter Cutoff", 20.0, 20000.0, 5000.0, "Hz"),
    descriptor("Filter Res", 0.1, 10.0, 1.0, "Q"),
    descriptor("Volume", 0.0, 1.0, 0.8, ""),
    descriptor("Octave", -3.0, 3.0, 0.0, "oct"),
    descriptor("Detune", -12.0, 12.0, 0.0, "st"),
    descriptor("Osc 2 Wave", 0.0, 3.0, 3.0, ""),
    descriptor("Osc 2 Octave", -3.0, 3.0, 0.0, "oct"),
    descriptor("Osc 2 Detune", -12.0, 12.0, 0.0, "st"),
    descriptor("Osc 2 Level", 0.0, 1.0, 0.0, ""),
    descriptor("Sub", 0.0, 1.0, 0.0, ""),
    descriptor("Noise", 0.0, 1.0, 0.0, ""),
    descriptor("Filter Type", 0.0, 3.0, 0.0, ""),
    descriptor("Voice Mode", 0.0, 2.0, 0.0, ""),
    descriptor("Voices", 1.0, MAX_VOICES as f32, MAX_VOICES as f32, ""),
    descriptor("Glide", 0.0, 2000.0, 0.0, "ms"),
    descriptor("LFO Shape", 0.0, 3.0, 0.0, ""),
    descriptor("LFO Target", 0.0, 2.0, 0.0, ""),
    descriptor("LFO Rate", 0.05, 20.0, 1.2, "Hz"),
    descriptor("LFO Amount", 0.0, 1.0, 0.0, ""),
    descriptor("Filter Env", -4.0, 4.0, 0.0, "oct"),
    descriptor("Filter Attack", 0.001, 2.0, 0.005, "s"),
    descriptor("Filter Decay", 0.001, 2.0, 0.3, "s"),
    descriptor("Filter Sustain", 0.0, 1.0, 0.0, ""),
    descriptor("Filter Release", 0.001, 5.0, 0.3, "s"),
];

/// Choices and counts, stored as whole numbers.
pub fn is_whole(index: usize) -> bool {
    matches!(
        index,
        param::WAVEFORM
            | param::OCTAVE
            | param::WAVEFORM_2
            | param::OCTAVE_2
            | param::FILTER_TYPE
            | param::VOICE_MODE
            | param::VOICES
            | param::LFO_SHAPE
            | param::LFO_TARGET
    )
}
