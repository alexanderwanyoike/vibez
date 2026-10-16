//! What a native Sampler card names its choices and tabs, and how its
//! One-shot, Gate and Loop modes map onto the sampler's two switches.

use vibez_instruments::sampler::param;

/// Index-aligned with `vibez_instruments::sampler::filter_type`.
pub const FILTER_NAMES: [&str; 5] = ["Off", "LP12", "LP24", "HP12", "BP12"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Plays through, whatever the note does.
    OneShot,
    /// Plays while the note is held.
    Gate,
    Loop,
}

impl Mode {
    pub const ALL: [(Self, &'static str); 3] = [
        (Self::OneShot, "One-shot"),
        (Self::Gate, "Gate"),
        (Self::Loop, "Loop"),
    ];

    pub fn of(params: &[f32]) -> Self {
        let on = |index: usize| params.get(index).is_some_and(|value| *value >= 0.5);
        if on(param::LOOP) {
            Self::Loop
        } else if on(param::ONE_SHOT) {
            Self::OneShot
        } else {
            Self::Gate
        }
    }

    /// The two switches this mode sets, as one edit.
    pub fn updates(self) -> Vec<(usize, f32)> {
        let (looping, one_shot) = match self {
            Self::OneShot => (0.0, 1.0),
            Self::Gate => (0.0, 0.0),
            Self::Loop => (1.0, 0.0),
        };
        vec![(param::LOOP, looping), (param::ONE_SHOT, one_shot)]
    }
}

/// The sampler's sections, in tab order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SamplerPage {
    #[default]
    Sample,
    Zones,
    Pitch,
    Filter,
    Modulation,
}

impl SamplerPage {
    pub const ALL: [Self; 5] = [
        Self::Sample,
        Self::Zones,
        Self::Pitch,
        Self::Filter,
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
            Self::Sample => "Sample",
            Self::Zones => "Zones",
            Self::Pitch => "Pitch",
            Self::Filter => "Filter",
            Self::Modulation => "Modulation",
        }
    }
}

/// The tabs a card shows.
pub fn pages(_pad: bool) -> &'static [SamplerPage] {
    &SamplerPage::ALL
}

/// The values typed beside the sample's name on a wide card.
pub fn header_controls(_pad: bool) -> &'static [usize] {
    &[param::ROOT_NOTE, param::TUNE, param::VOLUME]
}

/// The Pitch tab's controls.
pub fn pitch_controls(_pad: bool) -> &'static [usize] {
    &[param::ROOT_NOTE, param::TUNE, param::FINE, param::GLIDE]
}

pub fn is_native_sampler(track: &crate::state::UiTrack) -> bool {
    track.has_instrument
        && track.instrument_kind == Some(vibez_core::midi::InstrumentKind::Sampler)
        && track.plugin_instrument_name.is_none()
        && track.plugin_instrument_ref.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_instruments::sampler::{filter_type, SAMPLER_PARAMS};

    #[test]
    fn each_mode_sets_the_switches_that_read_back_as_it() {
        let mut params: Vec<f32> = SAMPLER_PARAMS.iter().map(|d| d.default).collect();
        assert_eq!(Mode::of(&params), Mode::Gate, "the original sampler gated");
        for (mode, _) in Mode::ALL {
            for (index, value) in mode.updates() {
                params[index] = value;
            }
            assert_eq!(Mode::of(&params), mode);
        }
    }

    #[test]
    fn a_pad_shows_no_zone_root_or_glide_and_both_show_pan() {
        assert!(!pages(true).contains(&SamplerPage::Zones));
        assert_eq!(pages(false), &SamplerPage::ALL);
        for index in [param::ROOT_NOTE, param::GLIDE] {
            assert!(!pitch_controls(true).contains(&index));
            assert!(!header_controls(true).contains(&index));
        }
        assert!(pitch_controls(true).contains(&param::VELOCITY_AMOUNT));
        assert!(header_controls(true).contains(&param::PAN));
        assert!(header_controls(false).contains(&param::PAN));
    }

    #[test]
    fn names_and_pages_line_up() {
        assert_eq!(FILTER_NAMES[filter_type::OFF], "Off");
        assert_eq!(FILTER_NAMES[filter_type::BP12], "BP12");
        for page in SamplerPage::ALL {
            assert_eq!(SamplerPage::at(page.index()), page);
        }
    }
}
