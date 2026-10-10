//! Source-time Capture closure and retained configuration diagnostics.

use super::*;

impl AudioEngine {

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
        self.cancel_capture_presentation();
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.flush_presentation();
    }

    pub(super) fn close_capture_on_failure(&mut self) {
        self.cancel_presentation();
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.flush_presentation();
    }
}
