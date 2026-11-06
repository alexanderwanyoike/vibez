//! How a launched Clip takes over a Track's launcher slot.

use super::*;
use crate::playback_source::{ActiveClipPlayback, PreparedClipPlayback, QueuedClipPlayback};
use vibez_core::perform::ClipLaunchMode;

impl EngineTrack {
    /// Swaps `prepared` into the launcher slot and records where it started.
    pub(crate) fn start_launcher_clip(&mut self, prepared: &mut PreparedClipPlayback) {
        let length = prepared.length_samples.max(1);
        let start = match (prepared.launch_mode, self.active_clip) {
            (ClipLaunchMode::Legato, Some(outgoing)) => {
                (outgoing.position % outgoing.length.max(1)) % length
            }
            _ => 0,
        };
        prepared.start_position = start;
        self.flush_notes();
        std::mem::swap(&mut self.launcher_source, &mut prepared.source);
        self.active_clip = prepared.clip_id.map(|clip_id| ActiveClipPlayback {
            clip_id,
            request_id: prepared.request_id,
            position: start,
            length,
            looping: prepared.looping,
        });
        self.pickup_launch_notes = start > 0;
    }

    /// The queued Clip once its swap time is final. A batch's time can still
    /// move until the whole batch is staged.
    pub(crate) fn scheduled_clip(&self, batch_waiting: bool) -> Option<&QueuedClipPlayback> {
        self.queued_clip
            .as_ref()
            .filter(|queued| !queued.pending_batch || batch_waiting)
    }

    /// Where a `mode` launch takes over instead of the shared launch
    /// boundary, if anywhere.
    pub(crate) fn own_launch_boundary(&self, mode: ClipLaunchMode, now: u64) -> Option<u64> {
        let playing = self.active_clip.filter(|_| mode == ClipLaunchMode::AtEnd)?;
        Some(now + playing.length.saturating_sub(playing.position))
    }

    /// Frames until a queued Legato Clip replaces the one playing.
    pub(crate) fn legato_swap_in(&self, now: u64, batch_waiting: bool) -> Option<u64> {
        self.active_clip?;
        self.scheduled_clip(batch_waiting)
            .filter(|queued| queued.prepared.launch_mode == ClipLaunchMode::Legato)
            .map(|queued| queued.effective_at.saturating_sub(now))
    }

    /// A Legato swap joins two Clips mid-waveform. Dipping the source to
    /// silence across the boundary hides the step without rendering both.
    pub(crate) fn update_launch_declick(&mut self, swap_in: Option<u64>, declick_frames: u64) {
        if declick_frames == 0 {
            return;
        }
        let dipping = self.launch_declick.target == 0.0;
        match swap_in.filter(|frames| *frames <= declick_frames) {
            Some(frames) if !dipping => self.launch_declick.ramp_to(0.0, frames as u32),
            None if dipping => self.launch_declick.ramp_to(1.0, declick_frames as u32),
            _ => {}
        }
    }

    pub(crate) fn reset_launch_declick(&mut self) {
        self.launch_declick.ramp_to(1.0, 0);
    }

    pub(super) fn apply_launch_declick(&mut self, frames: usize, channels: usize) {
        let ramp = &mut self.launch_declick;
        if ramp.remaining == 0 && ramp.gain == 1.0 {
            return;
        }
        for frame in self.mix_buffer[..frames * channels].chunks_exact_mut(channels) {
            let gain = ramp.next_gain();
            for sample in frame {
                *sample *= gain;
            }
        }
    }
}
