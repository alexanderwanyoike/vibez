//! Project Track lifecycle and channel controls exposed by Arrange.

use super::*;

impl ArrangementState {
    /// The channels an edit on `track_id` reaches: the selected set when the
    /// channel is part of it, otherwise the channel alone.
    fn channel_group(&self, track_id: TrackId) -> Vec<TrackId> {
        if self.selected_channels.contains(&track_id) {
            self.selected_channels.clone()
        } else {
            vec![track_id]
        }
    }

    fn select_channels(
        &mut self,
        project_tracks: &ProjectTracksState,
        keep: impl Fn(TrackId) -> bool,
    ) {
        self.selected_channels = project_tracks
            .channel_ids()
            .filter(|id| keep(*id))
            .collect();
    }

    fn remove_project_track(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        track_id: TrackId,
        engine: &mut impl EngineHandle,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        if track_id.is_master()
            || !project_tracks
                .tracks
                .iter()
                .any(|track| track.id == track_id)
        {
            return action;
        }

        self.pending_project_track_deletion = None;
        let removed_name = project_tracks
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .map(|track| track.name.clone())
            .unwrap_or_else(|| format!("{track_id}"));
        engine.send(EngineCommand::RemoveTrack(track_id));
        project_tracks.tracks.retain(|track| track.id != track_id);
        for track in &mut project_tracks.tracks {
            if track.audio_input_route.resample_source() == Some(track_id) {
                track.audio_input_route = AudioInputRoute::default();
                track.input_monitoring = InputMonitoring::Off;
            }
        }
        Arc::make_mut(&mut self.timeline).remove(track_id);
        self.selected_channels.retain(|id| *id != track_id);
        if self.selected_track == Some(track_id) {
            self.selected_track = project_tracks.tracks.first().map(|track| track.id);
        }
        if self
            .selected_note_clip
            .is_some_and(|(id, _)| id == track_id)
        {
            self.selected_note_clip = None;
        }
        self.selected_clips.retain(|selection| match selection {
            ArrangementSelection::AudioClip { track_id: id, .. }
            | ArrangementSelection::NoteClip { track_id: id, .. } => *id != track_id,
        });
        action.close_track_guis = Some(track_id);
        action.remove_track_from_sections = Some(track_id);
        action.status = Some(format!(
            "Removed {removed_name}. {} track(s) remain.",
            project_tracks.tracks.len()
        ));
        action
    }

    /// Arrange owns Project Track controls and resolves its local timeline
    /// before forwarding editor messages to the shared boundary.
    pub fn update(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        msg: ArrangementMsg,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        if msg.is_timeline_editor_message() {
            return self
                .resolve_timeline_mut()
                .editor
                .update(project_tracks, msg, engine, ctx);
        }

        let mut action = ArrangementAction::default();
        match msg {
            ArrangementMsg::AddTrack => {
                let Some(id) = project_tracks.add_numbered_track("Track", TrackKind::Audio, engine)
                else {
                    action.status = Some(super::PROJECT_TRACKS_FULL.to_string());
                    return action;
                };
                Arc::make_mut(&mut self.timeline).ensure(id);
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", project_tracks.tracks.len()));
            }
            ArrangementMsg::AddMidiTrack | ArrangementMsg::AddInstrumentTrack => {
                let Some(id) = project_tracks.add_numbered_track("MIDI", TrackKind::Midi, engine)
                else {
                    action.status = Some(super::PROJECT_TRACKS_FULL.to_string());
                    return action;
                };
                project_tracks
                    .find_mut(id)
                    .expect("new Project Track")
                    .has_instrument = false;
                Arc::make_mut(&mut self.timeline).ensure(id);
                self.selected_track = Some(id);
                action.status = Some(format!("{} tracks", project_tracks.tracks.len()));
            }
            ArrangementMsg::RequestRemoveTrack(track_id) => {
                if !track_id.is_master()
                    && project_tracks
                        .tracks
                        .iter()
                        .any(|track| track.id == track_id)
                {
                    self.pending_project_track_deletion = Some(track_id);
                }
            }
            ArrangementMsg::CancelRemoveTrack => {
                self.pending_project_track_deletion = None;
            }
            ArrangementMsg::ConfirmRemoveTrack(track_id) => {
                if self.pending_project_track_deletion != Some(track_id) {
                    return action;
                }
                return self.remove_project_track(project_tracks, track_id, engine);
            }
            ArrangementMsg::RemoveTrack(track_id) => {
                return self.remove_project_track(project_tracks, track_id, engine);
            }
            ArrangementMsg::SelectTrack(track_id) => {
                self.selected_track = Some(track_id);
                self.select_channels(project_tracks, |id| id == track_id);
            }
            ArrangementMsg::ToggleChannelSelection(track_id) => {
                if !track_id.is_master() {
                    let selected = std::mem::take(&mut self.selected_channels);
                    let joins = !selected.contains(&track_id);
                    self.select_channels(project_tracks, |id| {
                        if id == track_id {
                            joins
                        } else {
                            selected.contains(&id)
                        }
                    });
                    self.selected_track = Some(track_id);
                }
            }
            ArrangementMsg::SelectChannelRange(track_id) => {
                let order: Vec<_> = project_tracks.channel_ids().collect();
                let at = |id| order.iter().position(|channel| *channel == id);
                match (self.selected_track.and_then(at), at(track_id)) {
                    (Some(anchor), Some(end)) => {
                        let range = anchor.min(end)..=anchor.max(end);
                        self.selected_channels = order[range].to_vec();
                    }
                    _ => {
                        self.selected_track = Some(track_id);
                        self.select_channels(project_tracks, |id| id == track_id);
                    }
                }
            }
            ArrangementMsg::SelectAllChannels => self.select_channels(project_tracks, |_| true),
            ArrangementMsg::RenameTrack(track_id, new_name) => {
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.name = new_name;
                }
            }
            ArrangementMsg::MoveTrackUp(track_id) => {
                project_tracks.move_track(track_id, true, engine)
            }
            ArrangementMsg::MoveTrackDown(track_id) => {
                project_tracks.move_track(track_id, false, engine)
            }
            ArrangementMsg::MoveSelectedTrackUp => {
                if let Some(track_id) = self.selected_track {
                    project_tracks.move_track(track_id, true, engine);
                }
            }
            ArrangementMsg::MoveSelectedTrackDown => {
                if let Some(track_id) = self.selected_track {
                    project_tracks.move_track(track_id, false, engine);
                }
            }
            ArrangementMsg::SetTrackGain(track_id, gain) => {
                let gain = gain.clamp(0.0, 2.0);
                // The rest of the set moves by the same dB. The floor keeps a
                // fader leaving silence from multiplying the others by
                // infinity.
                let floor = vibez_core::level::db_to_amplitude(-60.0);
                let ratio = project_tracks
                    .find(track_id)
                    .map_or(1.0, |track| gain.max(floor) / track.gain.max(floor));
                for id in self.channel_group(track_id) {
                    if let Some(track) = project_tracks.find_mut(id) {
                        track.gain = if id == track_id {
                            gain
                        } else {
                            (track.gain * ratio).clamp(0.0, 2.0)
                        };
                        engine.send(EngineCommand::SetTrackGain(id, track.gain));
                    }
                }
            }
            ArrangementMsg::SetTrackPan(track_id, pan) => {
                let pan = pan.clamp(0.0, 1.0);
                let shift = project_tracks
                    .find(track_id)
                    .map_or(0.0, |track| pan - track.pan);
                for id in self.channel_group(track_id) {
                    if let Some(track) = project_tracks.find_mut(id) {
                        track.pan = if id == track_id {
                            pan
                        } else {
                            (track.pan + shift).clamp(0.0, 1.0)
                        };
                        engine.send(EngineCommand::SetTrackPan(id, track.pan));
                    }
                }
            }
            ArrangementMsg::SetTrackMute(track_id) => {
                if let Some(mute) = project_tracks.find(track_id).map(|track| !track.mute) {
                    for id in self.channel_group(track_id) {
                        if let Some(track) = project_tracks.find_mut(id) {
                            track.mute = mute;
                            engine.send(EngineCommand::SetTrackMute(id, mute));
                        }
                    }
                }
            }
            ArrangementMsg::SetTrackSolo(track_id) => {
                if let Some(solo) = project_tracks.find(track_id).map(|track| !track.solo) {
                    for id in self.channel_group(track_id) {
                        if let Some(track) = project_tracks.find_mut(id) {
                            track.solo = solo;
                            engine.send(EngineCommand::SetTrackSolo(id, solo));
                        }
                    }
                }
            }
            ArrangementMsg::AddBus => {
                if project_tracks.buses.len() >= vibez_engine::mixer::CHANNEL_CAPACITY {
                    action.status = Some(super::PROJECT_BUSES_FULL.to_string());
                    return action;
                }
                let letter = (b'A' + (project_tracks.buses.len() % 26) as u8) as char;
                let id = TrackId::new();
                let name = format!("{letter} Return");
                engine.send(EngineCommand::add_bus(id));
                let color_index = ((project_tracks.buses.len() + 4) % 8) as u8;
                let mut bus = ProjectTrack::new(id, name.clone(), color_index);
                attach_channel_eq(engine, &mut bus);
                project_tracks.buses.push(bus);
                Arc::make_mut(&mut self.timeline).ensure(id);
                self.selected_track = Some(id);
                action.status = Some(format!("Added {name}"));
            }
            ArrangementMsg::RemoveBus(bus_id) => {
                engine.send(EngineCommand::RemoveBus(bus_id));
                project_tracks.buses.retain(|bus| bus.id != bus_id);
                Arc::make_mut(&mut self.timeline).remove(bus_id);
                for track in &mut project_tracks.tracks {
                    track.sends.retain(|(id, _)| *id != bus_id);
                }
                for content in Arc::make_mut(&mut self.timeline).by_track.values_mut() {
                    content.automation.retain(|lane| {
                        lane.target != vibez_core::automation::AutomationTarget::Send { bus_id }
                    });
                }
                self.selected_channels.retain(|id| *id != bus_id);
                if self.selected_track == Some(bus_id) {
                    self.selected_track = project_tracks.tracks.first().map(|track| track.id);
                }
                action.close_track_guis = Some(bus_id);
                action.remove_track_from_sections = Some(bus_id);
                action.status = Some("Removed bus".to_string());
            }
            ArrangementMsg::SetSend {
                track_id,
                bus_id,
                amount,
            } => {
                let amount = amount.clamp(0.0, 1.0);
                let send_of = |track: &ProjectTrack| {
                    track
                        .sends
                        .iter()
                        .find(|(id, _)| *id == bus_id)
                        .map_or(0.0, |(_, amount)| *amount)
                };
                let shift = project_tracks
                    .tracks
                    .iter()
                    .find(|t| t.id == track_id)
                    .map_or(0.0, |track| amount - send_of(track));
                for id in self.channel_group(track_id) {
                    // Only tracks send to buses.
                    let Some(track) = project_tracks.tracks.iter_mut().find(|t| t.id == id) else {
                        continue;
                    };
                    let level = if id == track_id {
                        amount
                    } else {
                        (send_of(track) + shift).clamp(0.0, 1.0)
                    };
                    match track.sends.iter_mut().find(|(bus, _)| *bus == bus_id) {
                        Some(send) => send.1 = level,
                        None => track.sends.push((bus_id, level)),
                    }
                    engine.send(EngineCommand::SetSend {
                        track_id: id,
                        bus_id,
                        amount: level,
                    });
                }
            }
            ArrangementMsg::EngineTrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                if let Some(track) = project_tracks.find_mut(track_id) {
                    track.peak_l = peak_l.max(track.peak_l * 0.85);
                    track.peak_r = peak_r.max(track.peak_r * 0.85);
                }
            }
            _ => unreachable!("editor messages are delegated before Arrange track handling"),
        }
        action
    }
}
