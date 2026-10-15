//! Connects selected native Delay observations to persistent device panel state.

use super::delay_response_worker::{
    DelayResponseWorker, Owner, ReferenceKey, Request, ResponseResult,
};
use super::*;
use crate::domains::delay_observation::is_native_delay;
use crate::domains::delay_response::DelayResponse;
use vibez_core::{
    delay_state::DelayState,
    id::{EffectId, TrackId},
};

struct PendingEdit {
    after_order: u64,
    token: u64,
    values: std::collections::BTreeMap<usize, f32>,
}

struct CachedResponse {
    owner: Owner,
    key: ReferenceKey,
    response: Option<Arc<DelayResponse>>,
}

#[derive(Default)]
pub(super) struct DelayDevices {
    pub panels:
        std::collections::HashMap<EffectId, crate::domains::delay_observation::DelayObservation>,
    pub empty_panel: crate::domains::delay_observation::DelayObservation,
    worker: DelayResponseWorker,
    responses: std::collections::HashMap<EffectId, CachedResponse>,
    pending_edits: std::collections::HashMap<EffectId, PendingEdit>,
    next_token: u64,
}

impl App {
    pub(super) fn accept_delay_state(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        generation: u64,
        observation_token: u64,
        snapshot: Option<DelayState>,
    ) {
        if self.device_meters.channel != Some(track_id)
            || self.device_meters.generation != generation
        {
            return;
        }
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
            .filter(|effect| is_native_delay(effect))
        else {
            return;
        };
        let snapshot = if effect.bypass || effect.reconfiguration_failed {
            None
        } else {
            snapshot
        };
        let panel = self.delay_devices.panels.entry(effect_id).or_default();
        panel.accept(
            snapshot,
            self.state.transport.sample_rate,
            std::time::Instant::now(),
        );
        if let Some(state) = panel.snapshot {
            let acknowledged = self
                .delay_devices
                .pending_edits
                .get(&effect_id)
                .is_some_and(|pending| {
                    observation_token == pending.token
                        && state.observation_order > pending.after_order
                        && pending.values.iter().all(|(&index, &value)| {
                            state.targets[index].to_bits() == value.to_bits()
                        })
                });
            if acknowledged {
                self.delay_devices.pending_edits.remove(&effect_id);
            }
        }
    }

    pub(super) fn poll_delay_devices(&mut self) {
        let project = &self.state.project_tracks;
        self.delay_devices.panels.retain(|id, _| {
            project
                .tracks
                .iter()
                .chain(&project.buses)
                .chain(std::iter::once(&project.master))
                .any(|track| {
                    track
                        .effects
                        .iter()
                        .any(|effect| effect.id == *id && is_native_delay(effect))
                })
        });
        let now = std::time::Instant::now();
        for panel in self.delay_devices.panels.values_mut() {
            panel.expire(now);
        }
    }
    pub(super) fn restart_delay_responses(&mut self) {
        // Cancellation keeps the worker's occupied slot until its closure acknowledges destruction.
        self.delay_devices.worker.reconcile(&[]);
        self.delay_devices.responses.clear();
        self.delay_devices.pending_edits.clear();
        for panel in self.delay_devices.panels.values_mut() {
            panel.restart();
        }
    }

    pub(super) fn invalidate_delay_reference(
        &mut self,
        message: &crate::domains::devices::DevicesMsg,
    ) -> Option<EngineCommand> {
        use vibez_dsp::delay::param;
        let edit = message.param_edit()?;
        let Some(effect) = self
            .state
            .find_track(edit.track_id)
            .and_then(|track| {
                track
                    .effects
                    .iter()
                    .find(|effect| effect.id == edit.effect_id)
            })
            .filter(|effect| is_native_delay(effect))
        else {
            return None;
        };
        let values: std::collections::BTreeMap<_, _> = edit
            .updates
            .into_iter()
            .filter_map(|(index, value)| {
                if matches!(index, param::MIX | param::DUCK | param::TIME_CHANGE)
                    || !value.is_finite()
                {
                    return None;
                }
                let descriptor = effect.descriptors.get(index)?;
                let value = value.clamp(descriptor.min, descriptor.max);
                let value = match index {
                    param::SYNC
                    | param::LINK
                    | param::FEEDBACK_HP_ENABLED
                    | param::FEEDBACK_LP_ENABLED => f32::from(u8::from(value >= 0.5)),
                    param::LEFT_DIVISION
                    | param::LEFT_MODIFIER
                    | param::ROUTING
                    | param::RIGHT_DIVISION
                    | param::RIGHT_MODIFIER => {
                        // Echo matching needs the DSP's single ordinal zero representation.
                        value.round() as u32 as f32
                    }
                    _ => value,
                };
                Some((index, value))
            })
            .collect();
        if values.is_empty() {
            return None;
        }
        let after_order = self
            .delay_devices
            .panels
            .get(&edit.effect_id)
            .and_then(|panel| panel.snapshot)
            .map_or(0, |state| state.observation_order);
        self.delay_devices.next_token = self.delay_devices.next_token.wrapping_add(1).max(1);
        let token = self.delay_devices.next_token;
        let pending = self
            .delay_devices
            .pending_edits
            .entry(edit.effect_id)
            .or_insert(PendingEdit {
                after_order,
                token,
                values: Default::default(),
            });
        pending.token = token;
        pending.after_order = pending.after_order.max(after_order);
        pending.values.extend(values);
        self.delay_devices.worker.cancel(Owner {
            track_id: edit.track_id,
            effect_id: edit.effect_id,
            generation: self.device_meters.generation,
        });
        Some(EngineCommand::SetEffectObservationToken {
            track_id: edit.track_id,
            effect_id: edit.effect_id,
            token,
        })
    }

    fn delay_request(&self, owner: Owner) -> Option<Request> {
        if self.device_meters.channel != Some(owner.track_id)
            || self.device_meters.generation != owner.generation
            || self
                .delay_devices
                .pending_edits
                .contains_key(&owner.effect_id)
        {
            return None;
        }
        let effect = self
            .state
            .find_track(owner.track_id)?
            .effects
            .iter()
            .find(|effect| effect.id == owner.effect_id && is_native_delay(effect))?;
        if effect.bypass || effect.reconfiguration_failed {
            return None;
        }
        let state = self.delay_devices.panels.get(&owner.effect_id)?.snapshot?;
        (!state.reference_moving && state.sample_rate == self.state.transport.sample_rate)
            .then(|| Request::new(owner, state))
    }

    pub(super) fn poll_delay_responses(&mut self) -> Task<Message> {
        let owners: Vec<_> = self
            .device_meters
            .channel
            .and_then(|track| self.state.find_track(track))
            .into_iter()
            .flat_map(|track| {
                track
                    .effects
                    .iter()
                    .filter(|effect| {
                        is_native_delay(effect) && !effect.bypass && !effect.reconfiguration_failed
                    })
                    .map(|effect| Owner {
                        track_id: track.id,
                        effect_id: effect.id,
                        generation: self.device_meters.generation,
                    })
            })
            .collect();
        if !self.delay_devices.worker.reconcile(&owners) {
            self.delay_devices.worker.reconcile(&[]);
            return Task::none();
        }
        self.delay_devices
            .responses
            .retain(|id, _| self.delay_devices.panels.contains_key(id));
        self.delay_devices
            .pending_edits
            .retain(|id, _| self.delay_devices.panels.contains_key(id));
        for owner in owners {
            let Some(request) = self.delay_request(owner) else {
                self.delay_devices.worker.cancel(owner);
                continue;
            };
            if !self
                .delay_devices
                .responses
                .get(&owner.effect_id)
                .is_some_and(|cached| cached.owner == owner && cached.key == request.key)
            {
                self.delay_devices.worker.offer(request);
            }
        }
        let Some(work) = self.delay_devices.worker.admit() else {
            return Task::none();
        };
        let token = work.token;
        let task = Task::perform(work.run(), |(token, result)| {
            Message::DelayResponseReady(token, result)
        });
        self.delay_devices.worker.attach(token, task)
    }

    pub(super) fn finish_delay_response(&mut self, token: u64, result: ResponseResult) {
        let Some((request, result)) = self.delay_devices.worker.complete(token, result) else {
            return;
        };
        if self
            .delay_request(request.owner)
            .is_some_and(|current| current.key == request.key)
        {
            self.delay_devices.responses.insert(
                request.owner.effect_id,
                CachedResponse {
                    owner: request.owner,
                    key: request.key,
                    response: result.ok().flatten(),
                },
            );
        }
    }

    pub(super) fn delay_reference(
        &self,
        track_id: TrackId,
        effect_id: EffectId,
    ) -> (Option<&DelayResponse>, &'static str) {
        let owner = Owner {
            track_id,
            effect_id,
            generation: self.device_meters.generation,
        };
        if self.delay_devices.pending_edits.contains_key(&effect_id) {
            return (None, "Awaiting applied state");
        }
        let Some(request) = self.delay_request(owner) else {
            return (None, "Moving or unavailable");
        };
        let Some(cached) = self
            .delay_devices
            .responses
            .get(&effect_id)
            .filter(|cached| cached.owner == owner && cached.key == request.key)
        else {
            return (None, "Calculating");
        };
        (
            cached.response.as_deref(),
            if cached.response.is_some() {
                "Reference wet response"
            } else {
                "Reference unavailable"
            },
        )
    }
}

#[cfg(test)]
#[path = "delay_reference_currency_tests.rs"]
mod currency_tests;
