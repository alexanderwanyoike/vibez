use super::*;

impl App {
    pub(super) fn sidechain_model(&self) -> Vec<vibez_core::routing::RoutingChannel> {
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
                    for lane in &content.automation {
                        if let vibez_core::automation::AutomationTarget::Send { bus_id } =
                            lane.target
                        {
                            reserve_send(channel, bus_id);
                        }
                    }
                }
            }
        }
        channels
    }

    pub(super) fn reject_invalid_routing_edit(
        &mut self,
        message: &crate::message::Message,
    ) -> bool {
        use crate::domains::{
            arrangement::ArrangementMsg, automation::AutomationMsg, devices::DevicesMsg,
        };
        use vibez_core::automation::AutomationTarget;
        if !matches!(
            message,
            Message::Devices(
                DevicesMsg::SetSidechainSource { .. } | DevicesMsg::SetSidechainTap { .. }
            ) | Message::Arrangement(ArrangementMsg::SetSend { .. })
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
        if !matches!(message, Message::Devices(_)) {
            return false;
        }
        let mut tracks = self.state.project_tracks.tracks.clone();
        let mut master = self.state.project_tracks.master.clone();
        let mut buses = self.state.project_tracks.buses.clone();
        let changed = match message {
            Message::Devices(DevicesMsg::SetSidechainSource {
                track_id,
                effect_id,
                input_id,
                source,
            }) => Some(crate::domains::sidechain::edit_source_with_model(
                &mut tracks,
                &mut master,
                &mut buses,
                (*track_id, *effect_id, *input_id),
                *source,
                &model,
            )),
            Message::Devices(DevicesMsg::SetSidechainTap {
                track_id,
                effect_id,
                input_id,
                tap,
            }) => Some(crate::domains::sidechain::edit_tap_with_model(
                &mut tracks,
                &mut master,
                &mut buses,
                (*track_id, *effect_id, *input_id),
                *tap,
                &model,
            )),
            _ => None,
        };
        if changed == Some(false) {
            return true;
        }
        if changed.is_some() {
            self.state.devices.last_routing = Some(model);
        }
        false
    }

    pub(super) fn sync_sidechain_routing(&mut self) {
        let mut channels = self.sidechain_model();
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
            return;
        }
        match vibez_engine::routing::PreparedRouting::prepare(&channels, 4096) {
            Ok(prepared) => {
                self.send_command(EngineCommand::SetRouting(prepared));
                self.state.devices.last_routing = Some(channels);
            }
            Err(error) => self.state.status_text = error,
        }
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
