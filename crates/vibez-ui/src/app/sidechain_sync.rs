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
}

impl RoutingInputs {
    pub fn capture(state: &AppState) -> Self {
        Self {
            tracks: Arc::clone(&state.project_tracks),
            arrange: Arc::clone(&state.arrangement.timeline),
            sections: Arc::clone(&state.perform.sections),
            clips: Arc::clone(&state.perform.clips),
        }
    }
    pub fn matches(&self, state: &AppState) -> bool {
        Arc::ptr_eq(&self.tracks, &state.project_tracks)
            && Arc::ptr_eq(&self.arrange, &state.arrangement.timeline)
            && Arc::ptr_eq(&self.sections, &state.perform.sections)
            && Arc::ptr_eq(&self.clips, &state.perform.clips)
    }
}
