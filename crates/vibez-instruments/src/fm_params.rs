//! The FM Synth's controls.

use vibez_core::effect::ParamDescriptor;

pub use crate::synth_params::lfo_shape;

pub const OPERATORS: usize = 4;

/// Each operator's controls, in this order from its first index.
pub mod op {
    /// Its frequency against the note's, in steps of a half.
    pub const RATIO: usize = 0;
    /// Added to the ratio, to detune it off the harmonics.
    pub const FINE: usize = 1;
    pub const LEVEL: usize = 2;
    pub const FEEDBACK: usize = 3;
    /// How much quieter it gets above middle C, up to 6 dB an octave.
    pub const KEY_TRACK: usize = 4;
    pub const ATTACK: usize = 5;
    pub const DECAY: usize = 6;
    pub const SUSTAIN: usize = 7;
    pub const RELEASE: usize = 8;
    pub const COUNT: usize = 9;
}

pub mod param {
    use super::{op, OPERATORS};

    pub const ALGORITHM: usize = OPERATORS * op::COUNT;
    /// Scales every modulator's depth.
    pub const BRIGHTNESS: usize = ALGORITHM + 1;
    /// Scales every operator's decay and release.
    pub const DECAY: usize = ALGORITHM + 2;
    /// The LFO's depth.
    pub const MOTION: usize = ALGORITHM + 3;
    pub const LFO_SHAPE: usize = ALGORITHM + 4;
    pub const LFO_RATE: usize = ALGORITHM + 5;
    pub const LFO_DEST: usize = ALGORITHM + 6;
    /// How much a soft note softens the modulators too, and so the tone.
    pub const VELOCITY: usize = ALGORITHM + 7;
    pub const VOICES: usize = ALGORITHM + 8;
    pub const VOLUME_DB: usize = ALGORITHM + 9;
    pub const COUNT: usize = ALGORITHM + 10;

    /// An operator's control, `operator` being 0 for A up to 3 for D.
    pub const fn of(operator: usize, control: usize) -> usize {
        operator * op::COUNT + control
    }
}

/// Where the LFO can be sent.
pub mod dest {
    pub const BRIGHTNESS: usize = 0;
    pub const PITCH: usize = 1;
    pub const VOLUME: usize = 2;
    pub const COUNT: usize = 3;
}

/// The LFO at full Motion moves the pitch this many semitones either way.
pub const MOD_PITCH_SEMITONES: f32 = 1.0;
pub const MAX_VOICES: usize = 8;
/// A level at the bottom of its range is silent.
pub const LEVEL_FLOOR_DB: f32 = -60.0;

/// Which operators modulate which, as the eight algorithms of Yamaha's
/// four-operator FM synths (the DX21, DX27, DX100 and TX81Z). Modulation
/// always runs from a later letter to an earlier one, so operators work
/// from D down to A, and A always sounds.
pub struct Algorithm {
    /// For each operator, the operators modulating it, one bit each.
    pub modulators: [u8; OPERATORS],
    /// The operators heard, one bit each.
    pub carriers: u8,
}

/// Index-aligned with [`ALGORITHMS`].
pub const ALGORITHM_NAMES: [&str; 8] = [
    "D → C → B → A",
    "C + D → B → A",
    "C → B → A ← D",
    "D → C → A ← B",
    "D → C, B → A",
    "D → A, B, C",
    "D → C, A, B",
    "A + B + C + D",
];

const A: u8 = 1;
const B: u8 = 2;
const C: u8 = 4;
const D: u8 = 8;

pub static ALGORITHMS: [Algorithm; ALGORITHM_NAMES.len()] = [
    Algorithm {
        modulators: [B, C, D, 0],
        carriers: A,
    },
    Algorithm {
        modulators: [B, C | D, 0, 0],
        carriers: A,
    },
    Algorithm {
        modulators: [B | D, C, 0, 0],
        carriers: A,
    },
    Algorithm {
        modulators: [B | C, 0, D, 0],
        carriers: A,
    },
    Algorithm {
        modulators: [B, 0, D, 0],
        carriers: A | C,
    },
    Algorithm {
        modulators: [D, D, D, 0],
        carriers: A | B | C,
    },
    Algorithm {
        modulators: [0, 0, D, 0],
        carriers: A | B | C,
    },
    Algorithm {
        modulators: [0; OPERATORS],
        carriers: A | B | C | D,
    },
];

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

/// An operator's controls, `level` its default: the first sounds, the
/// second colours it a little, and the others wait silent.
macro_rules! operator {
    ($name:literal, $level:expr) => {
        [
            descriptor(concat!("Op ", $name, " Ratio"), 0.5, 16.0, 1.0, ""),
            descriptor(concat!("Op ", $name, " Fine"), -0.5, 0.5, 0.0, ""),
            descriptor(
                concat!("Op ", $name, " Level"),
                LEVEL_FLOOR_DB,
                0.0,
                $level,
                "dB",
            ),
            descriptor(concat!("Op ", $name, " Feedback"), 0.0, 1.0, 0.0, ""),
            descriptor(concat!("Op ", $name, " Key Track"), 0.0, 1.0, 0.0, ""),
            descriptor(concat!("Op ", $name, " Attack"), 0.001, 2.0, 0.005, "s"),
            descriptor(concat!("Op ", $name, " Decay"), 0.001, 5.0, 0.6, "s"),
            descriptor(concat!("Op ", $name, " Sustain"), 0.0, 1.0, 0.7, ""),
            descriptor(concat!("Op ", $name, " Release"), 0.001, 5.0, 0.3, "s"),
        ]
    };
}

const OPERATOR_PARAMS: [[ParamDescriptor; op::COUNT]; OPERATORS] = [
    operator!("A", 0.0),
    operator!("B", -18.0),
    operator!("C", LEVEL_FLOOR_DB),
    operator!("D", LEVEL_FLOOR_DB),
];

pub static FM_PARAMS: &[ParamDescriptor] = &{
    let mut all = [descriptor("", 0.0, 0.0, 0.0, ""); param::COUNT];
    let mut index = 0;
    while index < param::ALGORITHM {
        all[index] = OPERATOR_PARAMS[index / op::COUNT][index % op::COUNT];
        index += 1;
    }
    all[param::ALGORITHM] = descriptor("Algorithm", 0.0, (ALGORITHMS.len() - 1) as f32, 0.0, "");
    all[param::BRIGHTNESS] = descriptor("Brightness", 0.0, 1.0, 0.5, "");
    all[param::DECAY] = descriptor("Decay", 0.0, 1.0, 0.5, "");
    all[param::MOTION] = descriptor("Motion", 0.0, 1.0, 0.0, "");
    all[param::LFO_SHAPE] = descriptor("LFO Shape", 0.0, 3.0, 0.0, "");
    all[param::LFO_RATE] = descriptor("LFO Rate", 0.05, 20.0, 5.0, "Hz");
    all[param::LFO_DEST] = descriptor(
        "LFO Destination",
        0.0,
        (dest::COUNT - 1) as f32,
        dest::BRIGHTNESS as f32,
        "",
    );
    all[param::VELOCITY] = descriptor("Velocity", 0.0, 1.0, 0.5, "");
    all[param::VOICES] = descriptor("Voices", 1.0, MAX_VOICES as f32, MAX_VOICES as f32, "");
    all[param::VOLUME_DB] = descriptor("Volume", -24.0, 6.0, -6.0, "dB");
    all
};

/// Choices and counts, stored as whole numbers.
pub fn is_whole(index: usize) -> bool {
    matches!(
        index,
        param::ALGORITHM | param::LFO_SHAPE | param::LFO_DEST | param::VOICES
    )
}

/// Ratios step by a half, the harmonics and the octave below; Fine moves
/// off them.
pub fn is_ratio(index: usize) -> bool {
    index < param::ALGORITHM && index % op::COUNT == op::RATIO
}

/// An operator's level as a gain, silent at the bottom of its range.
pub fn level_gain(db: f32) -> f32 {
    if db <= LEVEL_FLOOR_DB {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}
