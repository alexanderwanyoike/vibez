//! The Wavetable Synth's controls.

use vibez_core::effect::ParamDescriptor;

pub use crate::synth_params::{filter_type, lfo_shape};

/// Each oscillator's controls, in this order from its first index.
pub mod osc {
    pub const TABLE: usize = 0;
    pub const POSITION: usize = 1;
    pub const WARP: usize = 2;
    /// The spread of the unison voices, in cents either side.
    pub const DETUNE: usize = 3;
    pub const UNISON: usize = 4;
    pub const LEVEL: usize = 5;
    pub const OCTAVE: usize = 6;
    pub const COUNT: usize = 7;
}

pub mod param {
    use super::osc;

    pub const OSC_A: usize = 0;
    pub const OSC_B: usize = osc::COUNT;
    pub const FILTER_TYPE: usize = 2 * osc::COUNT;
    pub const CUTOFF: usize = FILTER_TYPE + 1;
    pub const RESONANCE: usize = FILTER_TYPE + 2;
    pub const DRIVE: usize = FILTER_TYPE + 3;
    pub const ATTACK: usize = FILTER_TYPE + 4;
    pub const DECAY: usize = FILTER_TYPE + 5;
    pub const SUSTAIN: usize = FILTER_TYPE + 6;
    pub const RELEASE: usize = FILTER_TYPE + 7;
    pub const LFO_SHAPE: usize = FILTER_TYPE + 8;
    pub const LFO_RATE: usize = FILTER_TYPE + 9;
    pub const LFO_AMOUNT: usize = FILTER_TYPE + 10;
    pub const LFO_DEST: usize = FILTER_TYPE + 11;
    pub const ENV2_ATTACK: usize = FILTER_TYPE + 12;
    pub const ENV2_DECAY: usize = FILTER_TYPE + 13;
    pub const ENV2_SUSTAIN: usize = FILTER_TYPE + 14;
    pub const ENV2_RELEASE: usize = FILTER_TYPE + 15;
    pub const ENV2_AMOUNT: usize = FILTER_TYPE + 16;
    pub const ENV2_DEST: usize = FILTER_TYPE + 17;
    pub const VOICES: usize = FILTER_TYPE + 18;
    pub const OUTPUT_DB: usize = FILTER_TYPE + 19;
    pub const COUNT: usize = FILTER_TYPE + 20;

    /// An oscillator's control, `osc` being 0 for A and 1 for B.
    pub const fn of(osc: usize, control: usize) -> usize {
        osc * super::osc::COUNT + control
    }
}

/// Where the LFO and the second envelope can be sent.
pub mod dest {
    pub const OSC_A_POSITION: usize = 0;
    pub const OSC_B_POSITION: usize = 1;
    pub const OSC_A_WARP: usize = 2;
    pub const OSC_B_WARP: usize = 3;
    pub const CUTOFF: usize = 4;
    pub const PITCH: usize = 5;
    pub const VOLUME: usize = 6;
    pub const COUNT: usize = 7;

    /// Where an oscillator's position or warp is sent.
    pub const fn position(osc: usize) -> usize {
        OSC_A_POSITION + osc
    }

    pub const fn warp(osc: usize) -> usize {
        OSC_A_WARP + osc
    }
}

/// Full modulation moves the cutoff this many octaves, and the pitch this
/// many semitones, either way.
pub const MOD_CUTOFF_OCTAVES: f32 = 4.0;
pub const MOD_PITCH_SEMITONES: f32 = 12.0;
pub const MAX_VOICES: usize = 8;
pub const MAX_UNISON: usize = 8;

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

const TABLES: f32 = (crate::wavetables::table::COUNT - 1) as f32;
const HARMONIC: f32 = crate::wavetables::table::HARMONIC as f32;

pub static WAVETABLE_PARAMS: &[ParamDescriptor] = &[
    descriptor("Osc A Table", 0.0, TABLES, HARMONIC, ""),
    descriptor("Osc A Position", 0.0, 1.0, 0.66, ""),
    descriptor("Osc A Warp", 0.0, 1.0, 0.0, ""),
    descriptor("Osc A Detune", 0.0, 50.0, 0.0, "ct"),
    descriptor("Osc A Unison", 1.0, MAX_UNISON as f32, 1.0, ""),
    descriptor("Osc A Level", 0.0, 1.0, 0.8, ""),
    descriptor("Osc A Octave", -3.0, 3.0, 0.0, "oct"),
    descriptor("Osc B Table", 0.0, TABLES, HARMONIC, ""),
    descriptor("Osc B Position", 0.0, 1.0, 0.3, ""),
    descriptor("Osc B Warp", 0.0, 1.0, 0.0, ""),
    descriptor("Osc B Detune", 0.0, 50.0, 0.0, "ct"),
    descriptor("Osc B Unison", 1.0, MAX_UNISON as f32, 1.0, ""),
    descriptor("Osc B Level", 0.0, 1.0, 0.0, ""),
    descriptor("Osc B Octave", -3.0, 3.0, 0.0, "oct"),
    descriptor("Filter Type", 0.0, 3.0, 1.0, ""),
    descriptor("Cutoff", 20.0, 20_000.0, 20_000.0, "Hz"),
    descriptor("Resonance", 0.0, 1.0, 0.1, ""),
    descriptor("Drive", 0.0, 1.0, 0.0, ""),
    descriptor("Attack", 0.001, 2.0, 0.01, "s"),
    descriptor("Decay", 0.001, 2.0, 0.3, "s"),
    descriptor("Sustain", 0.0, 1.0, 0.8, ""),
    descriptor("Release", 0.001, 5.0, 0.3, "s"),
    descriptor("LFO Shape", 0.0, 3.0, 0.0, ""),
    descriptor("LFO Rate", 0.05, 20.0, 1.0, "Hz"),
    descriptor("LFO Amount", 0.0, 1.0, 0.0, ""),
    descriptor("LFO Destination", 0.0, (dest::COUNT - 1) as f32, 0.0, ""),
    descriptor("Env 2 Attack", 0.001, 2.0, 0.005, "s"),
    descriptor("Env 2 Decay", 0.001, 2.0, 0.4, "s"),
    descriptor("Env 2 Sustain", 0.0, 1.0, 0.0, ""),
    descriptor("Env 2 Release", 0.001, 5.0, 0.3, "s"),
    descriptor("Env 2 Amount", -1.0, 1.0, 0.0, ""),
    descriptor("Env 2 Destination", 0.0, (dest::COUNT - 1) as f32, 4.0, ""),
    descriptor("Voices", 1.0, MAX_VOICES as f32, MAX_VOICES as f32, ""),
    descriptor("Output", -24.0, 6.0, -6.0, "dB"),
];

/// Choices and counts, stored as whole numbers.
pub fn is_whole(index: usize) -> bool {
    if index < param::FILTER_TYPE {
        return matches!(index % osc::COUNT, osc::TABLE | osc::UNISON | osc::OCTAVE);
    }
    matches!(
        index,
        param::FILTER_TYPE | param::LFO_SHAPE | param::LFO_DEST | param::ENV2_DEST | param::VOICES
    )
}

/// The filter's Q for Resonance 0 to 1, from Butterworth to ringing.
pub fn resonance_q(resonance: f32) -> f32 {
    std::f32::consts::FRAC_1_SQRT_2 * 14f32.powf(resonance.clamp(0.0, 1.0))
}
