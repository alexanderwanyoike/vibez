//! What a native Subtractive Synth card names its choices, and which of
//! its sections a card too narrow for all of them shows.

use vibez_core::id::TrackId;

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

    pub fn name(self) -> &'static str {
        match self {
            Self::Oscillator => "Oscillator",
            Self::Filter => "Filter",
            Self::Envelope => "Envelope",
            Self::Modulation => "Modulation",
        }
    }
}

#[derive(Debug, Clone)]
pub enum SynthMsg {
    ShowPage(TrackId, SynthPage),
    /// Shows the filter's envelope instead of the level's, or back.
    ShowFilterEnvelope(TrackId, bool),
}

impl SynthMsg {
    pub fn track_id(&self) -> TrackId {
        match self {
            Self::ShowPage(id, _) | Self::ShowFilterEnvelope(id, _) => *id,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SynthPanel {
    pub page: SynthPage,
    pub filter_envelope: bool,
}

impl SynthPanel {
    pub fn update(&mut self, message: SynthMsg) {
        match message {
            SynthMsg::ShowPage(_, page) => self.page = page,
            SynthMsg::ShowFilterEnvelope(_, filter) => self.filter_envelope = filter,
        }
    }
}

/// The concept's Soft Analog sound: a saw over a triangle an octave down
/// through a gently resonant 24 dB low-pass, breathing with the LFO.
pub const SOFT_ANALOG: &[(usize, f32)] = {
    use vibez_instruments::synth::{filter_type, param, Waveform};
    &[
        (param::WAVEFORM, Waveform::Saw as usize as f32),
        (param::WAVEFORM_2, Waveform::Triangle as usize as f32),
        (param::OCTAVE_2, -1.0),
        (param::LEVEL_2, 0.6),
        (param::SUB, 0.25),
        (param::NOISE, 0.063),
        (param::FILTER_TYPE, filter_type::LP24 as f32),
        (param::CUTOFF, 2_400.0),
        (param::RESONANCE, 1.5),
        (param::ATTACK, 0.012),
        (param::DECAY, 0.24),
        (param::SUSTAIN, 0.68),
        (param::RELEASE, 0.42),
        (param::LFO_RATE, 1.2),
        (param::LFO_AMOUNT, 0.35),
        (param::VOLUME, 0.5),
    ]
};

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
    fn a_panel_remembers_its_page_and_envelope() {
        let mut panel = SynthPanel::default();
        let track = TrackId::new();
        panel.update(SynthMsg::ShowPage(track, SynthPage::Filter));
        panel.update(SynthMsg::ShowFilterEnvelope(track, true));
        assert_eq!(panel.page, SynthPage::Filter);
        assert!(panel.filter_envelope);
    }
}
