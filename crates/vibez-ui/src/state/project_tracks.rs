//! Project-owned track lookup and its unchanged defaults.

use super::{new_master_track, ProjectTrack, TrackId};

/// Project-owned tracks and channels shared by every musical timeline.
#[derive(Debug, Clone)]
pub struct ProjectTracksState {
    pub tracks: Vec<ProjectTrack>,
    /// The master bus channel (see [`new_master_track`]).
    pub master: ProjectTrack,
    /// Return channels: mixer-only tracks fed by per-track sends.
    pub buses: Vec<ProjectTrack>,
    pub next_track_number: u32,
}

impl Default for ProjectTracksState {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            master: new_master_track(),
            buses: Vec::new(),
            next_track_number: 0,
        }
    }
}

impl ProjectTracksState {
    pub fn find(&self, id: TrackId) -> Option<&ProjectTrack> {
        if id.is_master() {
            return Some(&self.master);
        }
        self.tracks
            .iter()
            .chain(self.buses.iter())
            .find(|t| t.id == id)
    }

    pub fn find_mut(&mut self, id: TrackId) -> Option<&mut ProjectTrack> {
        if id.is_master() {
            return Some(&mut self.master);
        }
        self.tracks
            .iter_mut()
            .chain(self.buses.iter_mut())
            .find(|t| t.id == id)
    }
}
