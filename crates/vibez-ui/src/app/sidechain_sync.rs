//! Canonical input identities that invalidate routing preparation.
use crate::{
    domains::perform::{ClipStore, SectionStore},
    state::{AppState, ProjectTracksState, TimelineContent},
};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
struct TimingInputs {
    sample_rate: u32,
    instrument: Option<vibez_core::id::TrackId>,
    selected: Option<vibez_core::id::TrackId>,
    armed: Option<vibez_core::id::TrackId>,
    monitored: Option<vibez_core::id::TrackId>,
    midi_open: bool,
}

pub(super) struct RoutingInputs {
    tracks: Arc<ProjectTracksState>,
    arrange: Arc<TimelineContent>,
    sections: Arc<SectionStore>,
    clips: Arc<ClipStore>,
    timing: TimingInputs,
}

impl RoutingInputs {
    fn timing(state: &AppState, midi_open: bool) -> TimingInputs {
        TimingInputs {
            sample_rate: state.transport.sample_rate,
            instrument: state.perform.instrument_target(),
            selected: state.arrangement.selected_track,
            armed: state.audio_recording.armed_track,
            monitored: state.audio_recording.monitor_track,
            midi_open,
        }
    }
    pub fn capture(state: &AppState, midi_open: bool) -> Self {
        Self {
            timing: Self::timing(state, midi_open),
            tracks: Arc::clone(&state.project_tracks),
            arrange: Arc::clone(&state.arrangement.timeline),
            sections: Arc::clone(&state.perform.sections),
            clips: Arc::clone(&state.perform.clips),
        }
    }
    pub fn matches(&self, state: &AppState, midi_open: bool) -> bool {
        self.timing == Self::timing(state, midi_open) && self.matches_canonical(state)
    }
    pub fn matches_canonical(&self, state: &AppState) -> bool {
        Arc::ptr_eq(&self.tracks, &state.project_tracks)
            && Arc::ptr_eq(&self.arrange, &state.arrangement.timeline)
            && Arc::ptr_eq(&self.sections, &state.perform.sections)
            && Arc::ptr_eq(&self.clips, &state.perform.clips)
    }
}
