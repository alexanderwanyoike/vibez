use super::*;
use vibez_core::id::EffectId;

pub enum DeviceReconfiguration {
    Effect {
        track_id: TrackId,
        position: usize,
        slot: EffectSlot,
    },
    Instrument {
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

impl AudioEngine {
    pub(super) fn reconfiguration_pending(&self) -> bool {
        self.tracks
            .iter()
            .chain(&self.buses)
            .chain(std::iter::once(&self.master))
            .any(|track| {
                track
                    .effects
                    .iter()
                    .any(|slot| slot.effect.reconfiguration_requested())
                    || track
                        .instrument
                        .as_ref()
                        .is_some_and(|instrument| instrument.reconfiguration_requested())
            })
    }

    pub(super) fn validate_compensation(&mut self) {
        if self.compensation_suspended || self.graph_edit_pending || !self.compensation_valid {
            return;
        }
        let Some(routing) = self.routing.as_ref() else {
            return;
        };
        let mut invalid = None;
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
            let activation_rate = match node.stage {
                vibez_core::routing::NodeStage::Source => track
                    .instrument
                    .as_ref()
                    .and_then(|instrument| instrument.activation_sample_rate()),
                vibez_core::routing::NodeStage::Effect(id) => track
                    .effects
                    .iter()
                    .find(|slot| slot.id == id)
                    .and_then(|slot| slot.effect.activation_sample_rate()),
                _ => None,
            };
            if activation_rate.is_some_and(|rate| rate != self.sample_rate) {
                invalid = Some((node.channel, match node.stage {vibez_core::routing::NodeStage::Effect(id) => Some(id), _ => None}, "Device was activated at a different sample rate; recreate the device or reopen the project"));
                break;
            }
            let valid = match node.stage {
                vibez_core::routing::NodeStage::Source => track
                    .instrument
                    .as_ref()
                    .is_none_or(|instrument| instrument.processing_configuration_valid()),
                vibez_core::routing::NodeStage::Effect(id) => track
                    .effects
                    .iter()
                    .find(|slot| slot.id == id)
                    .is_some_and(|slot| slot.effect.processing_configuration_valid()),
                _ => true,
            };
            if !valid {
                invalid = Some((
                    node.channel,
                    match node.stage {
                        vibez_core::routing::NodeStage::Effect(id) => Some(id),
                        _ => None,
                    },
                    "Device processing configuration is unavailable or failed",
                ));
                break;
            }
            let (effect_id, actual) = match node.stage {
                vibez_core::routing::NodeStage::Source => (
                    None,
                    Some(
                        track
                            .instrument
                            .as_ref()
                            .map_or(0, |instrument| instrument.latency_samples()),
                    ),
                ),
                vibez_core::routing::NodeStage::Effect(id) => (
                    Some(id),
                    track
                        .effects
                        .iter()
                        .find(|slot| slot.id == id)
                        .map(|slot| slot.effect.latency_samples()),
                ),
                _ => continue,
            };
            if actual != Some(routing.device_latencies[index]) {
                invalid = Some((
                    node.channel,
                    effect_id,
                    "Device latency no longer matches the applied compensation plan",
                ));
                break;
            }
        }
        if let Some((track_id, effect_id, reason)) = invalid {
            self.close_capture_on_failure();
            self.compensation_valid = false;
            self.transport.stop();
            self.report_compensation_failure(track_id, effect_id, reason);
            let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
        }
    }

    pub(super) fn begin_device_reconfiguration(&mut self) {
        if self.compensation_suspended || self.graph_edit_pending {
            return;
        }
        let mut requested = None;
        for track in self
            .tracks
            .iter_mut()
            .chain(self.buses.iter_mut())
            .chain(std::iter::once(&mut self.master))
        {
            if let Some(position) = track
                .effects
                .iter()
                .position(|slot| slot.effect.reconfiguration_requested())
            {
                let mut slot = track.effects.remove(position);
                slot.effect.stop_for_reconfiguration();
                requested = Some(DeviceReconfiguration::Effect {
                    track_id: track.id,
                    position,
                    slot,
                });
                break;
            }
            if track
                .instrument
                .as_ref()
                .is_some_and(|instrument| instrument.reconfiguration_requested())
            {
                let mut instrument = track.instrument.take().unwrap();
                instrument.stop_for_reconfiguration();
                requested = Some(DeviceReconfiguration::Instrument {
                    track_id: track.id,
                    instrument,
                });
                break;
            }
        }
        if let Some(device) = requested {
            match self
                .event_tx
                .push(EngineEvent::DeviceReconfiguration(device))
            {
                Ok(()) => self.compensation_suspended = true,
                Err(rtrb::PushError::Full(EngineEvent::DeviceReconfiguration(device))) => {
                    self.restore_reconfigured_device(device)
                }
                Err(_) => unreachable!(),
            }
        }
    }

    pub(super) fn restore_reconfigured_device(&mut self, device: DeviceReconfiguration) {
        match device {
            DeviceReconfiguration::Effect {
                track_id,
                position,
                slot,
            } => {
                if let Some(track) = self.channel_mut(track_id) {
                    // Removal retained the vector capacity while the main
                    // thread exclusively owned the stopped native instance.
                    track
                        .effects
                        .insert(position.min(track.effects.len()), slot);
                } else {
                    self.dispose_effect(slot.effect);
                }
            }
            DeviceReconfiguration::Instrument {
                track_id,
                instrument,
            } => {
                if let Some(track) = self.channel_mut(track_id) {
                    track.instrument = Some(instrument);
                } else {
                    self.dispose_instrument(instrument);
                }
            }
        }
    }
}
