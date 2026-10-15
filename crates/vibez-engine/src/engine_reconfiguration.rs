//! Exclusive device handoff and applied-plan validation.

use super::device_validation::DeviceValidationDecision;
use super::handoff_observation::{
    observe_handoff_request, observe_transferred_recovery, HandoffRequestFacts,
};
use super::*;
use vibez_core::id::EffectId;

pub struct RetiredEffectStorage(pub(super) Vec<EffectSlot>);
impl std::fmt::Debug for RetiredEffectStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetiredEffectStorage")
            .field("capacity", &self.0.capacity())
            .finish()
    }
}

pub enum DeviceReconfiguration {
    Effect {
        handoff_id: u64,
        reserved_effects: Vec<EffectSlot>,
        track_id: TrackId,
        position: usize,
        slot: EffectSlot,
    },
    Instrument {
        handoff_id: u64,
        track_id: TrackId,
        instrument: Box<dyn vibez_instruments::Instrument>,
    },
}

impl std::fmt::Debug for DeviceReconfiguration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceReconfiguration")
            .field("track_id", &self.track_id())
            .field("effect_id", &self.effect_id())
            .finish()
    }
}

impl DeviceReconfiguration {
    pub fn prepare_effect_storage(&mut self, slots: usize) {
        if let Self::Effect {
            reserved_effects, ..
        } = self
        {
            reserved_effects.reserve(slots);
        }
    }

    pub fn handoff_id(&self) -> u64 {
        match self {
            Self::Effect { handoff_id, .. } | Self::Instrument { handoff_id, .. } => *handoff_id,
        }
    }

    /// Equality key for the live hosted Box, never a pointer to dereference.
    /// The held non-zero-sized native owner excludes reuse during this handoff.
    pub fn owner_identity(&self) -> usize {
        match self {
            Self::Effect { slot, .. } => effect_owner_identity(&*slot.effect),
            Self::Instrument { instrument, .. } => instrument_owner_identity(&**instrument),
        }
    }

    pub fn processing_recovery_requested(&self) -> bool {
        match self {
            Self::Effect { slot, .. } => slot.effect.processing_recovery_requested(),
            Self::Instrument { instrument, .. } => instrument.processing_recovery_requested(),
        }
    }

    pub fn track_id(&self) -> TrackId {
        match self {
            Self::Effect { track_id, .. } | Self::Instrument { track_id, .. } => *track_id,
        }
    }

    pub fn effect_id(&self) -> Option<EffectId> {
        match self {
            Self::Effect { slot, .. } => Some(slot.id),
            _ => None,
        }
    }

    pub fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        match self {
            Self::Effect { slot, .. } => slot.effect.reconfigure_on_main_thread(),
            Self::Instrument { instrument, .. } => instrument.reconfigure_on_main_thread(),
        }
    }

    pub fn external_inputs(&self) -> &[vibez_core::routing::ExternalInputDescriptor] {
        match self {
            Self::Effect { slot, .. } => slot.effect.external_inputs(),
            Self::Instrument { .. } => &[],
        }
    }

    pub fn latency_samples(&self) -> u32 {
        match self {
            Self::Effect { slot, .. } => slot.effect.latency_samples(),
            Self::Instrument { instrument, .. } => instrument.latency_samples(),
        }
    }
}

pub fn effect_owner_identity(effect: &dyn vibez_dsp::effect::AudioEffect) -> usize {
    effect as *const dyn vibez_dsp::effect::AudioEffect as *const () as usize
}

pub fn instrument_owner_identity(instrument: &dyn vibez_instruments::Instrument) -> usize {
    instrument as *const dyn vibez_instruments::Instrument as *const () as usize
}

impl AudioEngine {
    pub(super) fn is_device_recovering(&self, track: TrackId, effect: Option<EffectId>) -> bool {
        self.device_handoff.pending().is_some_and(|pending| {
            pending.is_local_recovery() && pending.track_id == track && pending.effect_id == effect
        })
    }

    pub(super) fn recovering_effect_bypass(
        &self,
        track: TrackId,
        effect: EffectId,
    ) -> Option<bool> {
        self.device_handoff
            .pending()
            .filter(|pending| {
                pending.is_local_recovery()
                    && pending.track_id == track
                    && pending.effect_id == Some(effect)
            })
            .map(|pending| pending.bypass)
    }

    pub(super) fn local_device_failure_pending(&self) -> bool {
        self.device_handoff
            .pending()
            .is_some_and(|pending| pending.is_local_recovery())
            || self
                .tracks
                .iter()
                .chain(&self.buses)
                .chain(std::iter::once(&self.master))
                .any(|track| {
                    track
                        .instrument
                        .as_ref()
                        .is_some_and(|instrument| instrument.processing_failure_is_local())
                        || track
                            .effects
                            .iter()
                            .any(|slot| slot.effect.processing_failure_is_local())
                })
    }

    pub(super) fn validate_compensation(&mut self) {
        if self.device_handoff.is_suspended() || self.compensation_plan.blocks_processing() {
            return;
        }
        let routing = &self.routing;
        let mut invalid = None;
        let mut local_failure = false;
        for (index, node) in routing.graph.nodes.iter().enumerate() {
            let track = if node.channel.is_master() {
                Some(&self.master)
            } else {
                self.tracks
                    .iter()
                    .chain(&self.buses)
                    .find(|track| track.id == node.channel)
            };
            let Some(track) = track else {
                // Retained Missing Source stages produce silence while the
                // receiving paths keep their valid prepared alignment.
                continue;
            };
            match self
                .observe_device_validation(track, *node, index)
                .decision()
            {
                DeviceValidationDecision::Current => {}
                DeviceValidationDecision::LocalRecovery => {
                    local_failure = true;
                }
                DeviceValidationDecision::Invalid(reason) => {
                    invalid = Some((
                        node.channel,
                        match node.stage {
                            vibez_core::routing::NodeStage::Effect(id) => Some(id),
                            _ => None,
                        },
                        reason,
                    ));
                    break;
                }
            }
        }
        if local_failure && !self.device_failure_capture_closed {
            self.close_capture_for_device_failure();
        }
        self.device_failure_capture_closed = local_failure;
        if let Some((track_id, effect_id, reason)) = invalid {
            self.close_capture_on_failure();
            self.compensation_plan.invalidate_applied_plan();
            self.transport.stop();
            self.report_compensation_failure(track_id, effect_id, reason);
            self.presentation_state.terminals.request_playback_stop();
            self.flush_presentation();
        }
    }

    pub(super) fn begin_device_reconfiguration(&mut self) {
        if !self.compensation_plan.applied_plan_is_valid()
            || self.device_handoff.pending().is_some()
            || self.device_handoff.is_suspended()
            || self.compensation_plan.awaiting_structural_plan()
        {
            return;
        }
        let Some(handoff_id) = self.device_handoff.candidate_identity() else {
            self.close_capture_on_failure();
            self.transport.stop();
            self.presentation_state.terminals.request_playback_stop();
            self.flush_presentation();
            self.report_compensation_failure(
                TrackId::MASTER,
                None,
                "Device handoff identity exhausted",
            );
            self.compensation_plan.invalidate_applied_plan();
            return;
        };
        let mut requested = None;
        for track in self
            .tracks
            .iter_mut()
            .chain(self.buses.iter_mut())
            .chain(std::iter::once(&mut self.master))
        {
            match observe_handoff_request(track) {
                HandoffRequestFacts::Effect { position } => {
                    let mut slot = track.effects.remove(position);
                    slot.effect.stop_for_reconfiguration();
                    requested = Some(DeviceReconfiguration::Effect {
                        handoff_id,
                        reserved_effects: Vec::new(),
                        track_id: track.id,
                        position,
                        slot,
                    });
                    break;
                }
                HandoffRequestFacts::Instrument => {
                    let mut instrument = track.instrument.take().unwrap();
                    instrument.stop_for_reconfiguration();
                    requested = Some(DeviceReconfiguration::Instrument {
                        handoff_id,
                        track_id: track.id,
                        instrument,
                    });
                    break;
                }
                HandoffRequestFacts::None => {}
            }
        }
        if let Some(device) = requested {
            self.device_handoff.record_issued_identity(handoff_id);
            let (position, bypass) = match &device {
                DeviceReconfiguration::Effect { position, slot, .. } => (*position, slot.bypass),
                _ => (0, false),
            };
            let scope = observe_transferred_recovery(&device).scope();
            self.device_handoff
                .track_transfer(device_handoff::PendingDeviceReconfiguration {
                    handoff_id,
                    track_id: device.track_id(),
                    effect_id: device.effect_id(),
                    position,
                    bypass,
                    scope,
                });
            match self
                .event_tx
                .push(EngineEvent::DeviceReconfiguration(device))
            {
                Ok(()) => self.device_handoff.publication_succeeded(scope),
                Err(rtrb::PushError::Full(EngineEvent::DeviceReconfiguration(device))) => {
                    let _ = self.restore_reconfigured_device(device);
                }
                Err(rtrb::PushError::Full(event)) => self.retire_event(event),
            }
        }
    }

    pub(super) fn handoff_is_current(&self, device: &DeviceReconfiguration) -> bool {
        self.device_handoff.pending().is_some_and(|pending| {
            pending.handoff_id == device.handoff_id()
                && pending.track_id == device.track_id()
                && pending.effect_id == device.effect_id()
        })
    }

    pub(super) fn restore_reconfigured_device(&mut self, device: DeviceReconfiguration) -> bool {
        let pending = self.device_handoff.take_for_restoration();
        match device {
            DeviceReconfiguration::Effect {
                track_id,
                mut slot,
                mut reserved_effects,
                ..
            } => {
                self.apply_pending_device_parameters(track_id, Some(slot.id), |index, value| {
                    slot.effect.set_param(index, value)
                });
                if let Some(track) = self.channel_mut(track_id) {
                    if track.effects.len() == track.effects.capacity()
                        && reserved_effects.capacity() > track.effects.len()
                    {
                        // A concurrent valid addition can consume the removed
                        // slot's spare capacity; main supplies the replacement.
                        reserved_effects.append(&mut track.effects);
                        std::mem::swap(&mut track.effects, &mut reserved_effects);
                    }
                    if track.effects.len() < track.effects.capacity() {
                        slot.bypass = pending.bypass;
                        track
                            .effects
                            .insert(pending.position.min(track.effects.len()), slot);
                        if reserved_effects.capacity() > 0 {
                            self.retire_event(EngineEvent::RetiredEffectStorage(
                                RetiredEffectStorage(reserved_effects),
                            ));
                        }
                        return true;
                    }
                }
                if reserved_effects.capacity() > 0 {
                    self.retire_event(EngineEvent::RetiredEffectStorage(RetiredEffectStorage(
                        reserved_effects,
                    )));
                }
                self.dispose_effect(slot.effect);
                self.compensation_plan.invalidate_applied_plan();
                self.report_compensation_failure(
                    track_id,
                    pending.effect_id,
                    "Device restoration has no reserved effect slot",
                );
            }
            DeviceReconfiguration::Instrument {
                track_id,
                mut instrument,
                ..
            } => {
                self.apply_pending_device_parameters(track_id, None, |index, value| {
                    instrument.set_param(index, value)
                });
                if let Some(track) = self.channel_mut(track_id) {
                    if track.instrument.is_none() {
                        track.instrument = Some(instrument);
                        return true;
                    }
                }
                self.dispose_instrument(instrument);
                self.compensation_plan.invalidate_applied_plan();
                self.report_compensation_failure(
                    track_id,
                    None,
                    "Device restoration target is unavailable",
                );
            }
        }
        false
    }
}
