//! What a native FM Synth card names its choices and its tabs.

pub use crate::domains::synth::LFO_SHAPE_NAMES;
pub use vibez_instruments::fm::ALGORITHM_NAMES;

/// Index-aligned with `vibez_instruments::fm::dest`.
pub const DEST_NAMES: [&str; 3] = ["Brightness", "Pitch", "Volume"];
pub const OPERATOR_NAMES: [&str; vibez_instruments::fm::OPERATORS] = ["A", "B", "C", "D"];

/// The card's tabs, in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FmPage {
    #[default]
    Overview,
    Operators,
    Envelopes,
    Modulation,
}

impl FmPage {
    pub const ALL: [Self; 4] = [
        Self::Overview,
        Self::Operators,
        Self::Envelopes,
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
            Self::Overview => "Overview",
            Self::Operators => "Operators",
            Self::Envelopes => "Envelopes",
            Self::Modulation => "Modulation",
        }
    }
}

pub fn is_native_fm(track: &crate::state::UiTrack) -> bool {
    track.has_instrument
        && track.instrument_kind == Some(vibez_core::midi::InstrumentKind::FmSynth)
        && track.plugin_instrument_name.is_none()
        && track.plugin_instrument_ref.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_instruments::fm::dest;

    #[test]
    fn names_line_up_with_the_synth_choices() {
        assert_eq!(DEST_NAMES.len(), dest::COUNT);
        assert_eq!(DEST_NAMES[dest::PITCH], "Pitch");
    }

    #[test]
    fn pages_round_trip_through_their_place_in_the_tabs() {
        for page in FmPage::ALL {
            assert_eq!(FmPage::at(page.index()), page);
        }
        assert_eq!(FmPage::at(99), FmPage::Overview);
    }
}
