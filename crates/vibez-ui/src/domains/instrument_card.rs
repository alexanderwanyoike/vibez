//! What a native instrument card remembers besides its parameters: which
//! of its sections a card too narrow for all of them shows, whether its
//! envelope shows the level's or the filter's, and a Drum Rack pad's name
//! while it is typed.

use vibez_core::id::TrackId;

#[derive(Debug, Clone)]
pub enum InstrumentCardMsg {
    /// Shows the section at this place in the card's own list of tabs.
    ShowPage(TrackId, usize),
    /// Shows the filter's envelope instead of the level's, or back.
    ShowFilterEnvelope(TrackId, bool),
    /// Shows a narrow Drum Rack's selected pad instead of its pads, or back.
    ShowPadSampler(TrackId, bool),
    /// A pad's name as typed so far.
    EditPadName(TrackId, usize, String),
    /// Names the pad as typed.
    CommitPadName(TrackId),
}

impl InstrumentCardMsg {
    pub fn track_id(&self) -> TrackId {
        match self {
            Self::ShowPage(id, _)
            | Self::ShowFilterEnvelope(id, _)
            | Self::ShowPadSampler(id, _)
            | Self::EditPadName(id, ..)
            | Self::CommitPadName(id) => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstrumentPanel {
    pub page: usize,
    pub filter_envelope: bool,
    pub pad_sampler: bool,
    /// The pad being named and its name so far.
    pub pad_name: Option<(usize, String)>,
    /// The kit a Drum Rack last loaded or saved.
    pub kit: Option<String>,
}

impl InstrumentPanel {
    /// Handles `message`, giving back the name a committed pad takes.
    pub fn update(&mut self, message: InstrumentCardMsg) -> Option<(usize, String)> {
        match message {
            InstrumentCardMsg::ShowPage(_, page) => self.page = page,
            InstrumentCardMsg::ShowFilterEnvelope(_, filter) => self.filter_envelope = filter,
            InstrumentCardMsg::ShowPadSampler(_, sampler) => self.pad_sampler = sampler,
            InstrumentCardMsg::EditPadName(_, pad, name) => self.pad_name = Some((pad, name)),
            InstrumentCardMsg::CommitPadName(_) => return self.pad_name.take(),
        }
        None
    }

    /// The name typed so far for `pad`.
    pub fn pad_name_draft(&self, pad: usize) -> Option<&str> {
        self.pad_name
            .as_ref()
            .filter(|(typed, _)| *typed == pad)
            .map(|(_, name)| name.as_str())
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

    #[test]
    fn a_typed_pad_name_is_given_back_once_on_commit() {
        let mut panel = InstrumentPanel::default();
        let track = TrackId::new();
        panel.update(InstrumentCardMsg::EditPadName(track, 4, "Sn".into()));
        panel.update(InstrumentCardMsg::EditPadName(track, 4, "Snare".into()));
        assert_eq!(panel.pad_name_draft(4), Some("Snare"));
        assert_eq!(panel.pad_name_draft(5), None);
        assert_eq!(
            panel.update(InstrumentCardMsg::CommitPadName(track)),
            Some((4, "Snare".into()))
        );
        assert_eq!(panel.update(InstrumentCardMsg::CommitPadName(track)), None);
    }
}
