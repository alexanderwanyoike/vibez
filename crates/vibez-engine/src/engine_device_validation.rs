//! Boundary-scoped device validation facts preserve the applied-plan failure precedence.

use super::{AudioEngine, EngineTrack};
use vibez_core::routing::{NodeStage, RoutingNode};

#[derive(Clone, Copy)]
pub(super) enum DeviceValidationFacts {
    ActivationMismatch,
    LocalRecovery,
    Unavailable,
    Latency { actual: Option<u32>, applied: u32 },
    NonDevice,
}

#[derive(Clone, Copy)]
pub(super) enum DeviceValidationDecision {
    Current,
    LocalRecovery,
    Invalid(&'static str),
}

impl DeviceValidationFacts {
    pub(super) fn decision(self) -> DeviceValidationDecision {
        match self {
            Self::ActivationMismatch => DeviceValidationDecision::Invalid(
                "Device was activated at a different sample rate; recreate the device or reopen the project",
            ),
            Self::LocalRecovery => DeviceValidationDecision::LocalRecovery,
            Self::Unavailable => DeviceValidationDecision::Invalid(
                "Device processing configuration is unavailable or failed",
            ),
            Self::Latency { actual, applied } if actual != Some(applied) => {
                DeviceValidationDecision::Invalid(
                    "Device latency no longer matches the applied compensation plan",
                )
            }
            Self::Latency { .. } | Self::NonDevice => DeviceValidationDecision::Current,
        }
    }
}

impl AudioEngine {
    pub(super) fn observe_device_validation(
        &self,
        track: &EngineTrack,
        node: RoutingNode,
        index: usize,
    ) -> DeviceValidationFacts {
        let activation_rate = match node.stage {
            NodeStage::Source => track
                .instrument
                .as_ref()
                .and_then(|instrument| instrument.activation_sample_rate()),
            NodeStage::Effect(id) => track
                .effects
                .iter()
                .find(|slot| slot.id == id)
                .and_then(|slot| slot.effect.activation_sample_rate()),
            _ => None,
        };
        if activation_rate.is_some_and(|rate| rate != self.sample_rate) {
            return DeviceValidationFacts::ActivationMismatch;
        }
        let valid = match node.stage {
            NodeStage::Source => track
                .instrument
                .as_ref()
                .is_none_or(|instrument| instrument.processing_configuration_valid()),
            NodeStage::Effect(id) => track
                .effects
                .iter()
                .find(|slot| slot.id == id)
                .is_some_and(|slot| slot.effect.processing_configuration_valid()),
            _ => true,
        };
        let recovering = match node.stage {
            NodeStage::Source => {
                self.is_device_recovering(node.channel, None)
                    || track
                        .instrument
                        .as_ref()
                        .is_some_and(|instrument| instrument.processing_failure_is_local())
            }
            NodeStage::Effect(id) => {
                self.is_device_recovering(node.channel, Some(id))
                    || track
                        .effects
                        .iter()
                        .find(|slot| slot.id == id)
                        .is_some_and(|slot| slot.effect.processing_failure_is_local())
            }
            _ => false,
        };
        // Local recovery suppresses latency sampling even if configuration
        // validity was false; the unrelated graph keeps its applied alignment.
        if recovering {
            return DeviceValidationFacts::LocalRecovery;
        }
        if !valid {
            return DeviceValidationFacts::Unavailable;
        }
        let actual = match node.stage {
            NodeStage::Source => Some(
                track
                    .instrument
                    .as_ref()
                    .map_or(0, |instrument| instrument.latency_samples()),
            ),
            NodeStage::Effect(id) => track
                .effects
                .iter()
                .find(|slot| slot.id == id)
                .map(|slot| slot.effect.latency_samples()),
            _ => return DeviceValidationFacts::NonDevice,
        };
        DeviceValidationFacts::Latency {
            actual,
            applied: self.routing.device_latencies[index],
        }
    }
}
