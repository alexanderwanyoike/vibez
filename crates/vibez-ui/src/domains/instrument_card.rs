//! What a native instrument card remembers besides its parameters: which
//! of its sections a card too narrow for all of them shows, and whether its
//! envelope shows the level's or the filter's.

use vibez_core::id::TrackId;

#[derive(Debug, Clone)]
pub enum InstrumentCardMsg {
    /// Shows the section at this place in the card's own list of tabs.
    ShowPage(TrackId, usize),
    /// Shows the filter's envelope instead of the level's, or back.
    ShowFilterEnvelope(TrackId, bool),
}

impl InstrumentCardMsg {
    pub fn track_id(&self) -> TrackId {
        match self {
            Self::ShowPage(id, _) | Self::ShowFilterEnvelope(id, _) => *id,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstrumentPanel {
    pub page: usize,
    pub filter_envelope: bool,
}

impl InstrumentPanel {
    pub fn update(&mut self, message: InstrumentCardMsg) {
        match message {
            InstrumentCardMsg::ShowPage(_, page) => self.page = page,
            InstrumentCardMsg::ShowFilterEnvelope(_, filter) => self.filter_envelope = filter,
        }
    }
}

/// Whether the track's instrument is a built-in one with its own card.
pub fn has_native_instrument(track: &crate::state::UiTrack) -> bool {
    track.has_instrument
        && track.instrument_kind.is_some()
        && track.plugin_instrument_name.is_none()
        && track.plugin_instrument_ref.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_remembers_its_page_and_envelope() {
        let mut panel = InstrumentPanel::default();
        let track = TrackId::new();
        panel.update(InstrumentCardMsg::ShowPage(track, 2));
        panel.update(InstrumentCardMsg::ShowFilterEnvelope(track, true));
        assert_eq!(panel.page, 2);
        assert!(panel.filter_envelope);
    }
}
