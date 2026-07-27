//! Global computer-key mapping into stable Pad Positions.

use serde::{Deserialize, Serialize};

use super::PadPosition;

/// A portable physical computer-key position supported by Perform rebinding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComputerKey {
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
}

impl ComputerKey {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }
}

const DEFAULT_COMPUTER_KEYS: [ComputerKey; 16] = [
    ComputerKey::Digit1,
    ComputerKey::Digit2,
    ComputerKey::Digit3,
    ComputerKey::Digit4,
    ComputerKey::Q,
    ComputerKey::W,
    ComputerKey::E,
    ComputerKey::R,
    ComputerKey::A,
    ComputerKey::S,
    ComputerKey::D,
    ComputerKey::F,
    ComputerKey::Z,
    ComputerKey::X,
    ComputerKey::C,
    ComputerKey::V,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformInputMapping {
    #[serde(default = "default_computer_keys")]
    computer_keys: [ComputerKey; 16],
}

const fn default_computer_keys() -> [ComputerKey; 16] {
    DEFAULT_COMPUTER_KEYS
}

impl Default for PerformInputMapping {
    fn default() -> Self {
        Self {
            computer_keys: DEFAULT_COMPUTER_KEYS,
        }
    }
}

impl PerformInputMapping {
    pub fn key_for(&self, position: PadPosition) -> ComputerKey {
        self.computer_keys[position.index()]
    }

    pub fn position_for(&self, key: ComputerKey) -> Option<PadPosition> {
        self.computer_keys
            .iter()
            .position(|candidate| *candidate == key)
            .map(|index| PadPosition::ALL[index])
    }

    pub fn rebind(&mut self, position: PadPosition, key: ComputerKey) {
        let target = position.index();
        if let Some(existing) = self
            .computer_keys
            .iter()
            .position(|candidate| *candidate == key)
        {
            self.computer_keys.swap(target, existing);
        } else {
            self.computer_keys[target] = key;
        }
    }
}
