//! How a launched Clip takes over a Track's launcher slot.

use super::*;
use crate::playback_source::{ActiveClipPlayback, PreparedClipPlayback};

impl EngineTrack {
    pub(crate) fn start_launcher_clip(&mut self, prepared: &mut PreparedClipPlayback) {
        self.flush_notes();
        std::mem::swap(&mut self.launcher_source, &mut prepared.source);
        self.active_clip = prepared.clip_id.map(|clip_id| ActiveClipPlayback {
            clip_id,
            request_id: prepared.request_id,
            position: 0,
            length: prepared.length_samples.max(1),
            looping: prepared.looping,
        });
    }
}
