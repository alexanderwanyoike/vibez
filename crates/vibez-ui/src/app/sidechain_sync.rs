//! Canonical input identities that invalidate routing preparation.
use crate::{
    domains::perform::{ClipStore, SectionStore},
    state::{AppState, ProjectTracksState, TimelineContent},
};
use std::sync::Arc;

pub(super) struct RoutingInputs {
    tracks: Arc<ProjectTracksState>,
    arrange: Arc<TimelineContent>,
    sections: Arc<SectionStore>,
    clips: Arc<ClipStore>,
    sample_rate: u32,
}

impl RoutingInputs {
    pub fn capture(state: &AppState) -> Self {
        Self {
            sample_rate: state.transport.sample_rate,
            tracks: Arc::clone(&state.project_tracks),
            arrange: Arc::clone(&state.arrangement.timeline),
            sections: Arc::clone(&state.perform.sections),
            clips: Arc::clone(&state.perform.clips),
        }
    }
    pub fn matches(&self, state: &AppState) -> bool {
        self.sample_rate == state.transport.sample_rate && self.matches_canonical(state)
    }
    pub fn matches_canonical(&self, state: &AppState) -> bool {
        Arc::ptr_eq(&self.tracks, &state.project_tracks)
            && Arc::ptr_eq(&self.arrange, &state.arrangement.timeline)
            && Arc::ptr_eq(&self.sections, &state.perform.sections)
            && Arc::ptr_eq(&self.clips, &state.perform.clips)
    }
}
