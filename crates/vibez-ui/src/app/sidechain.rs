use super::*;

#[cfg(test)]
thread_local! { pub(super) static MODEL_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

impl App {
    pub(super) fn sidechain_model(&self) -> Vec<vibez_core::routing::RoutingChannel> {
        #[cfg(test)]
        MODEL_BUILDS.with(|count| count.set(count.get() + 1));
        let mut channels = crate::domains::sidechain::routing_channels(
            &self.state.project_tracks.tracks,
            &self.state.project_tracks.master,
            &self.state.project_tracks.buses,
        );
        let timelines = std::iter::once(self.state.arrangement.timeline.as_ref())
            .chain(
                self.state
                    .perform
                    .sections
                    .sections
                    .iter()
                    .map(|section| section.timeline.as_ref()),
            )
            .chain(
                self.state
                    .perform
                    .clips
                    .clips
                    .iter()
                    .map(|clip| clip.timeline.as_ref()),
            );
        for timeline in timelines {
            for channel in &mut channels {
                if let Some(content) = timeline.get(channel.id) {
                    vibez_core::routing::reserve_automated_sends(
                        &mut channel.sends,
                        &content.automation,
                    );
                }
            }
        }
        for channel in &mut channels {
            channel
                .sends
                .retain(|(_, amount)| *amount > vibez_core::routing::SEND_SILENCE_THRESHOLD);
            for (_, amount) in &mut channel.sends {
                *amount = 1.0;
            }
        }
        channels
    }

    pub(super) fn reject_invalid_send_edit(&mut self, message: &crate::message::Message) -> bool {
        use crate::domains::{arrangement::ArrangementMsg, automation::AutomationMsg};
        use vibez_core::automation::AutomationTarget;
        if !matches!(
            message,
            Message::Arrangement(ArrangementMsg::SetSend { .. })
                | Message::Automation(AutomationMsg::AddLane {
                    target: AutomationTarget::Send { .. },
                    ..
                })
        ) {
            return false;
        }
        let mut model = self.sidechain_model();
        let send = match message {
            Message::Arrangement(ArrangementMsg::SetSend {
                track_id,
                bus_id,
                amount,
            }) if *amount > 0.0 => Some((*track_id, *bus_id)),
            Message::Automation(AutomationMsg::AddLane {
                track_id,
                target: AutomationTarget::Send { bus_id },
            }) => Some((*track_id, *bus_id)),
            _ => None,
        };
        if let Some((track_id, bus_id)) = send {
            if let Some(channel) = model.iter_mut().find(|channel| channel.id == track_id) {
                reserve_send(channel, bus_id);
            }
            if let Err(error) = vibez_core::routing::RoutingGraph::prepare(&model) {
                self.state.status_text = format!("Routing change rejected: {error:?}");
                return true;
            }
        }
        false
    }

    pub(super) fn retain_routing_activation(
        &mut self,
        channels: &[vibez_core::routing::RoutingChannel],
        model: &[vibez_core::routing::RoutingChannel],
    ) {
        let changed = channels.iter().zip(model).any(|(active, original)| {
            active
                .effects
                .iter()
                .zip(&original.effects)
                .any(|(a, b)| a.inactive_inputs != b.inactive_inputs)
        });
        if changed {
            let tracks = Arc::make_mut(&mut self.state.project_tracks);
            for channel in channels {
                if let Some(track) = tracks.find_mut(channel.id) {
                    for effect in &channel.effects {
                        if let Some(slot) =
                            track.effects.iter_mut().find(|slot| slot.id == effect.id)
                        {
                            slot.inactive_sidechains.clone_from(&effect.inactive_inputs);
                        }
                    }
                }
            }
            self.mark_project_dirty();
        }
    }

    pub(super) fn sync_sidechain_routing(&mut self) {
        if self
            .sidechain_sync_inputs
            .as_ref()
            .is_some_and(|inputs| inputs.matches(&self.state))
        {
            return;
        }
        let model = self.sidechain_model();
        let mut channels = match vibez_core::routing::resolve_restored(
            &model,
            self.state.devices.last_routing.as_deref(),
        ) {
            Ok(channels) => channels,
            Err(error) => {
                self.state.status_text = format!("Routing unavailable: {error:?}");
                self.send_command(EngineCommand::RejectRoutingUpdate {
                    reason: self.state.status_text.clone(),
                });
                self.sidechain_sync_inputs =
                    Some(super::sidechain_sync::RoutingInputs::capture(&self.state));
                return;
            }
        };
        self.retain_routing_activation(&channels, &model);
        let names: std::collections::HashMap<_, _> = self
            .state
            .project_tracks
            .tracks
            .iter()
            .chain(self.state.project_tracks.buses.iter())
            .map(|track| (track.id, track.name.clone()))
            .collect();
        let mut changed_names = false;
        for channel in &mut channels {
            for effect in &mut channel.effects {
                for route in &mut effect.assignments {
                    if let Some(name) = names.get(&route.source) {
                        if route.source_name != *name {
                            route.source_name.clone_from(name);
                            changed_names = true;
                        }
                    }
                }
            }
        }
        if changed_names {
            let project = Arc::make_mut(&mut self.state.project_tracks);
            for track in project
                .tracks
                .iter_mut()
                .chain(project.buses.iter_mut())
                .chain(std::iter::once(&mut project.master))
            {
                for effect in &mut track.effects {
                    for route in &mut effect.sidechains {
                        if let Some(name) = names.get(&route.source) {
                            route.source_name.clone_from(name);
                        }
                    }
                }
            }
        }
        for channel in &mut channels {
            for effect in &mut channel.effects {
                for route in &mut effect.assignments {
                    route.source_name.clear();
                }
            }
        }
        if self.state.devices.last_routing.as_ref() == Some(&channels) {
            self.sidechain_sync_inputs =
                Some(super::sidechain_sync::RoutingInputs::capture(&self.state));
            return;
        }
        match vibez_engine::routing::PreparedRouting::prepare(&channels, 4096) {
            Ok(prepared) => {
                self.state.devices.sidechain_choices =
                    crate::domains::sidechain::input_source_choices(&channels);
                self.send_command(EngineCommand::SetRouting(prepared));
                self.state.devices.last_routing = Some(channels);
            }
            Err(error) => {
                self.send_command(EngineCommand::RejectRoutingUpdate {
                    reason: error.clone(),
                });
                self.state.status_text = error;
            }
        }
        self.sidechain_sync_inputs =
            Some(super::sidechain_sync::RoutingInputs::capture(&self.state));
    }
}

fn reserve_send(
    channel: &mut vibez_core::routing::RoutingChannel,
    bus_id: vibez_core::id::TrackId,
) {
    if let Some((_, amount)) = channel.sends.iter_mut().find(|(id, _)| *id == bus_id) {
        *amount = amount.max(1.0);
    } else {
        channel.sends.push((bus_id, 1.0));
    }
}
