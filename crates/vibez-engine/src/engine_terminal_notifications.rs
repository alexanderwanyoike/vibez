//! Retain terminal event ownership and publication order while UI delivery is full.

use vibez_core::id::{EffectId, TrackId};

use crate::events::EngineEvent;

#[derive(Default)]
pub(super) struct TerminalNotifications {
    capture_stop: Option<u64>,
    compensation_failure: Option<(TrackId, Option<EffectId>, &'static str)>,
    playback_stop: bool,
    overflow_owner: Option<EngineEvent>,
}

impl TerminalNotifications {
    pub(super) fn capture_stop(&self) -> Option<u64> {
        self.capture_stop
    }

    pub(super) fn request_capture_stop(&mut self, effective_at_samples: u64) {
        // A later clock coordinate may move backwards after Seek. The first
        // accepted closure still owns this Capture lifetime's timestamp.
        self.capture_stop.get_or_insert(effective_at_samples);
    }

    pub(super) fn request_failure(
        &mut self,
        track_id: TrackId,
        effect_id: Option<EffectId>,
        reason: &'static str,
    ) {
        self.compensation_failure
            .get_or_insert((track_id, effect_id, reason));
    }

    pub(super) fn request_playback_stop(&mut self) {
        self.playback_stop = true;
    }

    pub(super) fn cancel_playback_stop(&mut self) {
        // Play can resume while the first Capture stop, cause and owner still
        // await delivery. Those lifetimes do not belong to this acknowledgement.
        self.playback_stop = false;
    }

    pub(super) fn retain_overflow_owner(&mut self, event: EngineEvent) -> Option<EngineEvent> {
        if self.overflow_owner.is_some() {
            return Some(event);
        }
        self.overflow_owner = Some(event);
        None
    }

    pub(super) fn occupied_slots(&self) -> usize {
        usize::from(self.capture_stop.is_some())
            + usize::from(self.compensation_failure.is_some())
            + usize::from(self.playback_stop)
            + usize::from(self.overflow_owner.is_some())
    }

    pub(super) fn publish(&mut self, events: &mut rtrb::Producer<EngineEvent>) -> bool {
        // The engine first publishes all due accepted Capture feed. A Full
        // response must leave this slot and every later owner intact.
        if let Some(effective_at_samples) = self.capture_stop {
            if events
                .push(EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                })
                .is_err()
            {
                return false;
            }
            self.capture_stop = None;
        }
        if let Some((track_id, effect_id, reason)) = self.compensation_failure {
            if events
                .push(EngineEvent::CompensationInvalid {
                    track_id,
                    effect_id,
                    reason,
                })
                .is_err()
            {
                return false;
            }
            self.compensation_failure = None;
        }
        if self.playback_stop {
            if events.push(EngineEvent::PlaybackStopped).is_err() {
                return false;
            }
            self.playback_stop = false;
        }
        if let Some(event) = self.overflow_owner.take() {
            if let Err(rtrb::PushError::Full(event)) = events.push(event) {
                self.overflow_owner = Some(event);
                return false;
            }
        }
        true
    }

    #[cfg(test)]
    pub(super) fn failure(&self) -> Option<(TrackId, Option<EffectId>, &'static str)> {
        self.compensation_failure
    }

    #[cfg(test)]
    pub(super) fn playback_stop_pending(&self) -> bool {
        self.playback_stop
    }

    #[cfg(test)]
    pub(super) fn overflow_owner(&self) -> Option<&EngineEvent> {
        self.overflow_owner.as_ref()
    }
}
