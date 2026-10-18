//! What a native Wavetable Synth card names its choices and its tabs.

pub use crate::domains::synth::{FILTER_NAMES, LFO_SHAPE_NAMES};

/// Index-aligned with `vibez_instruments::wavetables::table`.
pub const TABLE_NAMES: [&str; 5] = ["Basic Shapes", "Harmonic", "Formant", "Pulse", "Sync"];
/// Index-aligned with `vibez_instruments::wavetable::dest`.
pub const DEST_NAMES: [&str; 7] = [
    "Osc A Position",
    "Osc B Position",
    "Osc A Warp",
    "Osc B Warp",
    "Filter Cutoff",
    "Pitch",
    "Volume",
];

/// The card's tabs, in order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WavetablePage {
    #[default]
    Oscillators,
    Filter,
    Envelopes,
    Modulation,
}

impl WavetablePage {
    pub const ALL: [Self; 4] = [
        Self::Oscillators,
        Self::Filter,
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
            Self::Oscillators => "Oscillators",
            Self::Filter => "Filter",
            Self::Envelopes => "Envelopes",
            Self::Modulation => "Modulation",
        }
    }
}

pub fn is_native_wavetable(track: &crate::state::UiTrack) -> bool {
    track.has_instrument
        && track.instrument_kind == Some(vibez_core::midi::InstrumentKind::WavetableSynth)
        && track.plugin_instrument_name.is_none()
        && track.plugin_instrument_ref.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_instruments::wavetable::dest;
    use vibez_instruments::wavetables::table;

    #[test]
    fn names_line_up_with_the_synth_choices() {
        assert_eq!(TABLE_NAMES.len(), table::COUNT);
        assert_eq!(TABLE_NAMES[table::FORMANT], "Formant");
        assert_eq!(DEST_NAMES.len(), dest::COUNT);
        assert_eq!(DEST_NAMES[dest::CUTOFF], "Filter Cutoff");
    }

    #[test]
    fn pages_round_trip_through_their_place_in_the_tabs() {
        for page in WavetablePage::ALL {
            assert_eq!(WavetablePage::at(page.index()), page);
        }
        assert_eq!(WavetablePage::at(99), WavetablePage::Oscillators);
    }
}
