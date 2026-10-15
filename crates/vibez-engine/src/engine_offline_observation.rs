//! Boundary-scoped offline facts preserve strict Bounce failure order and diagnostics.

use super::AudioEngine;
use vibez_core::id::{EffectId, TrackId};

#[derive(Clone, Copy)]
pub(super) enum OfflineDeviceTarget {
    Instrument(TrackId),
    Effect { track: TrackId, effect: EffectId },
}

#[derive(Clone, Copy)]
pub(super) enum OfflineConfigurationFacts {
    ActivationMismatch {
        target: OfflineDeviceTarget,
        active: u32,
        requested: u32,
    },
    Unavailable(OfflineDeviceTarget),
    InvalidAppliedPlan,
    Current,
}

impl OfflineConfigurationFacts {
    pub(super) fn failure(self) -> Option<String> {
        match self {
            Self::ActivationMismatch { target, active, requested } => Some(match target {
                OfflineDeviceTarget::Instrument(track) => format!(
                    "Instrument on channel {} was activated at {active} Hz but Bounce requests {requested} Hz",
                    track.raw()
                ),
                OfflineDeviceTarget::Effect { track, effect } => format!(
                    "Effect {} on channel {} was activated at {active} Hz but Bounce requests {requested} Hz",
                    effect.raw(), track.raw()
                ),
            }),
            Self::Unavailable(OfflineDeviceTarget::Instrument(track)) => Some(format!(
                "Instrument on channel {} requires a valid processing configuration during Bounce",
                track.raw()
            )),
            Self::Unavailable(OfflineDeviceTarget::Effect { track, effect }) => Some(format!(
                "Effect {} on channel {} requires a valid processing configuration during Bounce",
                effect.raw(), track.raw()
            )),
            Self::InvalidAppliedPlan => {
                Some("Applied audio configuration became invalid during Bounce".into())
            }
            Self::Current => None,
        }
    }
}

impl AudioEngine {
    pub(super) fn observe_offline_configuration(&self) -> OfflineConfigurationFacts {
        for track in self
            .tracks
            .iter()
            .chain(&self.buses)
            .chain(std::iter::once(&self.master))
        {
            if let Some(rate) = track
                .instrument
                .as_ref()
                .and_then(|instrument| instrument.activation_sample_rate())
                .filter(|rate| *rate != self.sample_rate)
            {
                return OfflineConfigurationFacts::ActivationMismatch {
                    target: OfflineDeviceTarget::Instrument(track.id),
                    active: rate,
                    requested: self.sample_rate,
                };
            }
            if track.instrument.as_ref().is_some_and(|instrument| {
                instrument.reconfiguration_requested()
                    || !instrument.processing_configuration_valid()
            }) {
                return OfflineConfigurationFacts::Unavailable(OfflineDeviceTarget::Instrument(
                    track.id,
                ));
            }
            for slot in &track.effects {
                if let Some(rate) = slot
                    .effect
                    .activation_sample_rate()
                    .filter(|rate| *rate != self.sample_rate)
                {
                    return OfflineConfigurationFacts::ActivationMismatch {
                        target: OfflineDeviceTarget::Effect {
                            track: track.id,
                            effect: slot.id,
                        },
                        active: rate,
                        requested: self.sample_rate,
                    };
                }
                if slot.effect.reconfiguration_requested()
                    || !slot.effect.processing_configuration_valid()
                {
                    return OfflineConfigurationFacts::Unavailable(OfflineDeviceTarget::Effect {
                        track: track.id,
                        effect: slot.id,
                    });
                }
            }
        }
        // Bounce keeps its stricter policy and original last-resort plan check;
        // a realtime local-recovery exemption cannot authorize an offline export.
        if !self.compensation_plan.applied_plan_is_valid() {
            OfflineConfigurationFacts::InvalidAppliedPlan
        } else {
            OfflineConfigurationFacts::Current
        }
    }
}
