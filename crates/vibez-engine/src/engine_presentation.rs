//! Source-time Capture closure and retained configuration diagnostics.

use super::*;

impl AudioEngine {
    pub(super) fn flush_presentation(&mut self) {
        if let Some(effective_at_samples) = self.pending_capture_stop {
            if self
                .event_tx
                .push(EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                })
                .is_ok()
            {
                self.pending_capture_stop = None;
            } else {
                return;
            }
        }
        if let Some((track_id, effect_id, reason)) = self.pending_compensation_failure {
            if self
                .event_tx
                .push(EngineEvent::CompensationInvalid {
                    track_id,
                    effect_id,
                    reason,
                })
                .is_ok()
            {
                self.pending_compensation_failure = None;
            }
        }
        if self.pending_playback_stop && self.event_tx.push(EngineEvent::PlaybackStopped).is_ok() {
            self.pending_playback_stop = false;
        }
    }

    pub(super) fn report_compensation_failure(
        &mut self,
        track_id: TrackId,
        effect_id: Option<vibez_core::id::EffectId>,
        reason: &'static str,
    ) {
        self.pending_compensation_failure
            .get_or_insert((track_id, effect_id, reason));
        self.flush_presentation();
    }

    pub(super) fn close_capture_for_device_failure(&mut self) {
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.flush_presentation();
    }

    pub(super) fn close_capture_on_failure(&mut self) {
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.flush_presentation();
    }
}
