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
    midi_target: Option<vibez_core::id::TrackId>,
    armed: Option<vibez_core::id::TrackId>,
    monitored: Option<vibez_core::id::TrackId>,
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
        let reduced = state.project_tracks.reduced_latency_monitoring;
        TimingInputs {
            sample_rate: state.transport.sample_rate,
            instrument: reduced
                .then(|| {
                    state.perform.live_instrument_target(
                        state.view.workspace == crate::state::Workspace::Perform,
                        &state.project_tracks.tracks,
                    )
                })
                .flatten(),
            midi_target: (reduced && midi_open)
                .then(|| super::midi_input::configured_target(state))
                .flatten(),
            armed: reduced
                .then_some(state.audio_recording.armed_track)
                .flatten(),
            monitored: reduced
                .then_some(state.audio_recording.monitor_track)
                .flatten(),
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
