//! A Clip launches with its own launch options, or its Project Track's where it has none.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::{ClipLaunchMode, MusicalBoundary};
use vibez_project::ClipLaunchOverrides;

use super::{ClipStore, LauncherClip, PerformState};

/// The boundary a Project Track launches on until its launch quantization is set.
const DEFAULT_LAUNCH_QUANTIZATION: MusicalBoundary = MusicalBoundary::OneBar;

/// A Project Track's Clip launch options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrackLaunchOptions {
    pub(crate) quantization: MusicalBoundary,
    pub(crate) mode: ClipLaunchMode,
}

impl Default for TrackLaunchOptions {
    fn default() -> Self {
        Self {
            quantization: DEFAULT_LAUNCH_QUANTIZATION,
            mode: ClipLaunchMode::default(),
        }
    }
}

impl ClipStore {
    fn track_options(&self, track_id: TrackId) -> TrackLaunchOptions {
        self.track_options
            .get(&track_id)
            .copied()
            .unwrap_or_default()
    }

    fn update_track_options(
        &mut self,
        track_id: TrackId,
        update: impl FnOnce(&mut TrackLaunchOptions),
    ) {
        let mut options = self.track_options(track_id);
        update(&mut options);
        if options == TrackLaunchOptions::default() {
            self.track_options.remove(&track_id);
        } else {
            self.track_options.insert(track_id, options);
        }
    }

    /// Saved launch options for `tracks` that are off the defaults, in order.
    pub fn launcher_tracks(
        &self,
        tracks: impl IntoIterator<Item = TrackId>,
    ) -> Vec<vibez_project::LauncherTrackInfo> {
        tracks
            .into_iter()
            .filter_map(|track_id| {
                let options = self.track_options.get(&track_id)?;
                Some(vibez_project::LauncherTrackInfo {
                    track_id,
                    launch_quantization: options.quantization,
                    launch_mode: options.mode,
                })
            })
            .collect()
    }

    pub fn restore_launcher_tracks(&mut self, tracks: &[vibez_project::LauncherTrackInfo]) {
        for track in tracks {
            self.update_track_options(track.track_id, |options| {
                options.quantization = track.launch_quantization;
                options.mode = track.launch_mode;
            });
        }
    }

    pub fn launch_quantization(&self, track_id: TrackId) -> MusicalBoundary {
        self.track_options(track_id).quantization
    }

    pub fn clip_launch_quantization(&self, clip: &LauncherClip) -> MusicalBoundary {
        clip.launch
            .quantization
            .unwrap_or_else(|| self.launch_quantization(clip.track_id))
    }

    pub fn clip_launch_mode(&self, clip: &LauncherClip) -> ClipLaunchMode {
        clip.launch
            .mode
            .unwrap_or_else(|| self.launch_mode(clip.track_id))
    }

    pub fn launch_mode(&self, track_id: TrackId) -> ClipLaunchMode {
        self.track_options(track_id).mode
    }

    /// Prepares `clip` to launch with its own or its track's launch mode.
    pub fn prepare_launch(
        &self,
        clip: &LauncherClip,
        request_id: u64,
        samples_per_beat: f64,
    ) -> Box<vibez_engine::playback_source::PreparedClipPlayback> {
        let mut prepared = clip.prepare(request_id, samples_per_beat);
        prepared.launch_mode = self.clip_launch_mode(clip);
        prepared
    }
}

impl PerformState {
    /// Leaves the shared store untouched when `edit` changes nothing, so a
    /// picker choosing its current value is not a Project edit.
    pub(crate) fn edit_track_launch(
        &mut self,
        track_id: TrackId,
        edit: impl FnOnce(&mut TrackLaunchOptions),
    ) {
        let current = self.clips.track_options(track_id);
        let mut options = current;
        edit(&mut options);
        if options != current {
            Arc::make_mut(&mut self.clips)
                .update_track_options(track_id, |stored| *stored = options);
        }
    }

    /// The Clip counterpart of `edit_track_launch`.
    pub(crate) fn edit_clip_launch(
        &mut self,
        clip_id: ClipId,
        edit: impl FnOnce(&mut ClipLaunchOverrides),
    ) {
        let Some(current) = self.clips.by_id(clip_id).map(|clip| clip.launch) else {
            return;
        };
        let mut launch = current;
        edit(&mut launch);
        if launch != current {
            if let Some(clip) = Arc::make_mut(&mut self.clips).by_id_mut(clip_id) {
                clip.launch = launch;
            }
        }
    }

    /// A stop lands where the Clip it stops would launch.
    pub fn stop_quantization(&self, track_id: TrackId) -> MusicalBoundary {
        self.clip_editor.playing.get(&track_id).map_or_else(
            || self.clips.launch_quantization(track_id),
            |playing| {
                self.clips
                    .clip_launch_quantization(self.clips.by_id(playing.id).unwrap_or(playing))
            },
        )
    }
}
