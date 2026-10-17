//! Separate resident handoff discovery from the transferred owner's post-stop recovery observation.

use super::device_handoff::RecoveryScope;
use super::reconfiguration::DeviceReconfiguration;
use super::EngineTrack;

#[derive(Clone, Copy)]
pub(super) enum HandoffRequestFacts {
    Effect { position: usize },
    Instrument,
    None,
}

#[derive(Clone, Copy)]
pub(super) struct TransferredRecoveryFacts {
    local_recovery: bool,
}

impl TransferredRecoveryFacts {
    pub(super) fn scope(self) -> RecoveryScope {
        RecoveryScope::from_recovery_request(self.local_recovery)
    }
}

pub(super) fn observe_handoff_request(track: &EngineTrack) -> HandoffRequestFacts {
    if let Some(position) = track
        .effects
        .iter()
        .position(|slot| slot.effect.reconfiguration_requested())
    {
        HandoffRequestFacts::Effect { position }
    } else if track
        .instrument
        .as_ref()
        .is_some_and(|instrument| instrument.reconfiguration_requested())
    {
        HandoffRequestFacts::Instrument
    } else {
        HandoffRequestFacts::None
    }
}

pub(super) fn observe_transferred_recovery(
    device: &DeviceReconfiguration,
) -> TransferredRecoveryFacts {
    // Stopping can change the owner's recovery answer. Discovery cannot supply
    // this later classification, including when publication will be Full.
    TransferredRecoveryFacts {
        local_recovery: device.processing_recovery_requested(),
    }
}
