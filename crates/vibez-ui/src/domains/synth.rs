//! What a native Subtractive Synth card names its choices and its tabs.

/// Index-aligned with `vibez_instruments::synth::Waveform`.
pub const WAVE_NAMES: [&str; 4] = ["Sine", "Saw", "Square", "Triangle"];
/// Index-aligned with `vibez_instruments::synth::filter_type`.
pub const FILTER_NAMES: [&str; 4] = ["LP12", "LP24", "HP12", "BP12"];
/// Index-aligned with `vibez_instruments::synth::voice_mode`.
pub const VOICE_MODE_NAMES: [&str; 3] = ["Poly", "Mono", "Legato"];
/// Index-aligned with `vibez_instruments::synth::lfo_shape`.
pub const LFO_SHAPE_NAMES: [&str; 4] = ["Sine", "Triangle", "Square", "Saw"];
/// Index-aligned with `vibez_instruments::synth::lfo_target`.
pub const LFO_TARGET_NAMES: [&str; 3] = ["Cutoff", "Pitch", "Volume"];

/// The synth's sections, in tab order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SynthPage {
    #[default]
    Oscillator,
    Filter,
    Envelope,
    Modulation,
}

impl SynthPage {
    pub const ALL: [Self; 4] = [
        Self::Oscillator,
        Self::Filter,
        Self::Envelope,
        Self::Modulation,
    ];

    /// The page at this place in the tabs, as a panel remembers it.
    pub fn at(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or_default()
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Oscillator => "Oscillator",
            Self::Filter => "Filter",
            Self::Envelope => "Envelope",
            Self::Modulation => "Modulation",
        }
    }
}

pub fn is_native_synth(track: &crate::state::UiTrack) -> bool {
    track.has_instrument
        && track.instrument_kind == Some(vibez_core::midi::InstrumentKind::SubtractiveSynth)
        && track.plugin_instrument_name.is_none()
        && track.plugin_instrument_ref.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_instruments::synth::{filter_type, lfo_shape, lfo_target, voice_mode, Waveform};

    #[test]
    fn names_line_up_with_the_synth_choices() {
        assert_eq!(WAVE_NAMES[Waveform::Triangle as usize], "Triangle");
        assert_eq!(FILTER_NAMES[filter_type::BP12], "BP12");
        assert_eq!(VOICE_MODE_NAMES[voice_mode::LEGATO], "Legato");
        assert_eq!(LFO_SHAPE_NAMES[lfo_shape::SAW], "Saw");
        assert_eq!(LFO_TARGET_NAMES[lfo_target::VOLUME], "Volume");
    }

    #[test]
    fn pages_round_trip_through_their_place_in_the_tabs() {
        for page in SynthPage::ALL {
            assert_eq!(SynthPage::at(page.index()), page);
        }
        assert_eq!(SynthPage::at(99), SynthPage::Oscillator);
    }
}
