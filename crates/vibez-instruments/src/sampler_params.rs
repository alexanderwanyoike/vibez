//! The sampler's controls. The first ten are the original sampler's, at
//! their original indices; everything after them defaults to off or to the
//! original sound, so older projects play the same.

use vibez_core::effect::ParamDescriptor;

pub mod param {
    pub const ROOT_NOTE: usize = 0;
    pub const ATTACK: usize = 1;
    pub const DECAY: usize = 2;
    pub const SUSTAIN: usize = 3;
    pub const RELEASE: usize = 4;
    pub const START: usize = 5;
    pub const END: usize = 6;
    /// Loop mode; with One-Shot off and this off, the sample gates.
    pub const LOOP: usize = 7;
    pub const ONE_SHOT: usize = 8;
    pub const VOLUME: usize = 9;
    pub const LOOP_START: usize = 10;
    pub const LOOP_END: usize = 11;
    pub const REVERSE: usize = 12;
    pub const CROSSFADE: usize = 13;
    pub const TUNE: usize = 14;
    pub const FINE: usize = 15;
    pub const GLIDE: usize = 16;
    pub const KEY_LOW: usize = 17;
    pub const KEY_HIGH: usize = 18;
    pub const VELOCITY_LOW: usize = 19;
    pub const VELOCITY_HIGH: usize = 20;
    /// How much the played velocity sets the volume.
    pub const VELOCITY_AMOUNT: usize = 21;
    pub const FILTER_TYPE: usize = 22;
    pub const CUTOFF: usize = 23;
    pub const RESONANCE: usize = 24;
    pub const FILTER_ENV: usize = 25;
    pub const FILTER_ATTACK: usize = 26;
    pub const FILTER_DECAY: usize = 27;
    pub const FILTER_SUSTAIN: usize = 28;
    pub const FILTER_RELEASE: usize = 29;
    pub const LFO_SHAPE: usize = 30;
    pub const LFO_TARGET: usize = 31;
    pub const LFO_RATE: usize = 32;
    pub const LFO_AMOUNT: usize = 33;
    pub const COUNT: usize = 34;
}

/// The sampler's filter choices: off, then the synth's four types.
pub mod filter_type {
    pub const OFF: usize = 0;
    pub const LP12: usize = 1;
    pub const LP24: usize = 2;
    pub const HP12: usize = 3;
    pub const BP12: usize = 4;
}

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

pub static SAMPLER_PARAMS: &[ParamDescriptor] = &[
    descriptor("Root Note", 0.0, 127.0, 60.0, "note"),
    descriptor("Attack", 0.001, 2.0, 0.001, "s"),
    descriptor("Decay", 0.001, 2.0, 0.1, "s"),
    descriptor("Sustain", 0.0, 1.0, 1.0, ""),
    descriptor("Release", 0.001, 5.0, 0.01, "s"),
    descriptor("Start", 0.0, 1.0, 0.0, ""),
    descriptor("End", 0.0, 1.0, 1.0, ""),
    descriptor("Loop", 0.0, 1.0, 0.0, ""),
    descriptor("One-Shot", 0.0, 1.0, 0.0, ""),
    descriptor("Volume", 0.0, 1.0, 0.8, ""),
    descriptor("Loop Start", 0.0, 1.0, 0.0, ""),
    descriptor("Loop End", 0.0, 1.0, 1.0, ""),
    descriptor("Reverse", 0.0, 1.0, 0.0, ""),
    descriptor("Loop Crossfade", 0.0, 500.0, 0.0, "ms"),
    descriptor("Tune", -24.0, 24.0, 0.0, "st"),
    descriptor("Fine", -100.0, 100.0, 0.0, "ct"),
    descriptor("Glide", 0.0, 2000.0, 0.0, "ms"),
    descriptor("Key Low", 0.0, 127.0, 0.0, "note"),
    descriptor("Key High", 0.0, 127.0, 127.0, "note"),
    descriptor("Velocity Low", 1.0, 127.0, 1.0, ""),
    descriptor("Velocity High", 1.0, 127.0, 127.0, ""),
    descriptor("Velocity", 0.0, 1.0, 1.0, ""),
    descriptor("Filter Type", 0.0, 4.0, 0.0, ""),
    descriptor("Filter Cutoff", 20.0, 20000.0, 20000.0, "Hz"),
    descriptor("Filter Res", 0.1, 10.0, 0.707, "Q"),
    descriptor("Filter Env", -4.0, 4.0, 0.0, "oct"),
    descriptor("Filter Attack", 0.001, 2.0, 0.005, "s"),
    descriptor("Filter Decay", 0.001, 2.0, 0.3, "s"),
    descriptor("Filter Sustain", 0.0, 1.0, 0.0, ""),
    descriptor("Filter Release", 0.001, 5.0, 0.3, "s"),
    descriptor("LFO Shape", 0.0, 3.0, 0.0, ""),
    descriptor("LFO Target", 0.0, 2.0, 0.0, ""),
    descriptor("LFO Rate", 0.05, 20.0, 1.2, "Hz"),
    descriptor("LFO Amount", 0.0, 1.0, 0.0, ""),
];

/// Choices, switches, notes and velocities, stored as whole numbers.
pub fn is_whole(index: usize) -> bool {
    matches!(
        index,
        param::ROOT_NOTE
            | param::LOOP
            | param::ONE_SHOT
            | param::REVERSE
            | param::KEY_LOW
            | param::KEY_HIGH
            | param::VELOCITY_LOW
            | param::VELOCITY_HIGH
            | param::FILTER_TYPE
            | param::LFO_SHAPE
            | param::LFO_TARGET
    )
}
