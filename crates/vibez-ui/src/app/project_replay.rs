//! Undo snapshots and project-to-engine replay.
//! Split from app/project_io.rs; inherent methods on [`super::App`].

use std::sync::Arc;

use vibez_core::id::{EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_engine::commands::EngineCommand;

use crate::state::{ProjectTrack, UndoGestureId};

use super::*;

impl App {
    pub(super) fn take_snapshot(&self) -> crate::state::ProjectSnapshot {
        self.state.project_snapshot()
    }

    /// The current state with every live plugin's state recorded in it.
    pub(super) fn snapshot_with_live_plugin_states(&self) -> crate::state::ProjectSnapshot {
        let mut snapshot = self.take_snapshot();
        crate::domains::project::record_live_plugin_states(&mut snapshot, |key| {
            self.capture_device_state(key)
        });
        snapshot
    }

    pub(super) fn push_undo_snapshot(&mut self, gesture: Option<crate::state::UndoGestureId>) {
        let snapshot = self.take_snapshot();
        self.state.project.history.push_edit(snapshot, gesture);
    }

    /// The undo step before `message`, holding the live state of any plugin
    /// instrument the message takes away.
    pub(super) fn push_undo_snapshot_before(
        &mut self,
        message: &crate::message::Message,
        gesture: Option<crate::state::UndoGestureId>,
    ) {
        let mut snapshot = self.take_snapshot();
        if let Some(track_id) = super::update_policy::departing_instrument(message) {
            crate::domains::project::record_plugin_instrument_state(
                &mut snapshot,
                track_id,
                |key| self.capture_device_state(key),
            );
        }
        self.state.project.history.push_edit(snapshot, gesture);
    }

    /// Open one project transaction at the current canonical editable state.
    /// Returns false when a wider transaction is already active.
    pub(super) fn begin_project_transaction(&mut self) -> bool {
        let snapshot = self.take_snapshot();
        self.state
            .project
            .history
            .begin_transaction(snapshot, self.state.project.dirty)
    }

    pub(super) fn commit_project_transaction(&mut self) -> bool {
        self.state.project.history.commit_transaction()
    }

    /// Project Track deletion is the first multi-store consumer of project
    /// transactions: Arrange removes the channel and local content, then the
    /// application action removes that identity from every Section.
    pub(super) fn begin_project_track_deletion_transaction(
        &mut self,
        message: &crate::message::Message,
    ) -> bool {
        matches!(
            message,
            crate::message::Message::Arrangement(
                crate::domains::arrangement::ArrangementMsg::ConfirmRemoveTrack(_)
                    | crate::domains::arrangement::ArrangementMsg::RemoveTrack(_)
            )
        ) && self.begin_project_transaction()
    }

    pub(super) fn apply_arrangement_action_in_transaction(
        &mut self,
        action: crate::domains::arrangement::ArrangementAction,
        owns_transaction: bool,
    ) -> iced::Task<crate::message::Message> {
        let removed_project_track = action.remove_track_from_sections.is_some();
        let task = self.apply_arrangement_action(action);
        if owns_transaction {
            if removed_project_track {
                self.commit_project_transaction();
            } else {
                // A stale confirmation or missing id is a no-op, so it must
                // not create an empty transaction in history.
                if let Some((_, dirty_before)) = self.state.project.history.abandon_transaction() {
                    self.state.project.dirty = dirty_before;
                }
            }
        }
        task
    }

    pub(super) fn discard_project_transaction(&mut self) {
        if let Some((_, dirty_before)) = self.state.project.history.abandon_transaction() {
            self.state.project.dirty = dirty_before;
        }
    }

    #[allow(dead_code)]
    pub(super) fn abandon_project_transaction(&mut self) -> bool {
        let Some((snapshot, dirty_before)) = self.state.project.history.abandon_transaction()
        else {
            return false;
        };
        self.apply_snapshot(snapshot);
        self.state.project.dirty = dirty_before;
        true
    }

    /// Escape during a lane drag: what the drag `gesture` edited, the
    /// engine and a clip drag's selection go back to how the drag found
    /// them, and no undo step is left.
    pub(super) fn cancel_drag(&mut self, gesture: UndoGestureId) {
        self.state.arrangement.editor.cancel_clip_drag();
        self.state.perform.timeline_editor_mut().cancel_clip_drag();
        if let Some(snapshot) = self.state.project.history.revert_gesture(gesture) {
            self.apply_snapshot(snapshot);
        }
        self.state.arrangement.end_drag_resize();
        self.state.perform.timeline_editor_mut().end_drag_resize();
        self.state.status_text = "Drag cancelled".into();
    }

    pub(super) fn apply_snapshot(&mut self, snapshot: crate::state::ProjectSnapshot) {
        self.restart_device_metering();
        self.cancel_device_preset_load();
        if clip_only_snapshot(&self.state, &snapshot) {
            self.state.perform.clips = snapshot.launcher_clips;
            self.state
                .perform
                .sync_selected_timeline_editor(self.state.arrangement.selected_track);
            return;
        }

        self.state
            .arrangement
            .editor
            .discard_audio_clip_inspector_edits();
        self.state
            .perform
            .section_editor
            .editor_mut()
            .discard_audio_clip_inspector_edits();
        self.send_command(EngineCommand::BeginContentRebuild);
        if self
            .state
            .project_tracks
            .same_channel_layout(&snapshot.project_tracks)
        {
            self.swap_changed_values(snapshot);
        } else {
            self.replay_snapshot(snapshot);
        }
        self.send_command(EngineCommand::EndContentRebuild);
    }

    /// Tears down every channel and replays the snapshot into the engine,
    /// reloading its plugins.
    fn replay_snapshot(&mut self, mut snapshot: crate::state::ProjectSnapshot) {
        self.plugin_load_requests.reset();
        self.sidechain_sync_inputs = None;
        self.track_meter_peaks.clear();
        // Plugin devices cannot live inside snapshots; strip them
        // into reload requests first, capturing the live state of
        // instances that still exist so undo keeps their exact
        // parameters. Must happen before the pointer maps are
        // cleared below.
        let reloads =
            crate::domains::project::collect_plugin_reload_requests(&mut snapshot, |key| {
                self.capture_device_state(key)
            });

        // Plugin instances die with their tracks below. Their GUI
        // windows and raw pointers MUST go first: the window manager
        // pumps VST3 run-loop timers every tick, and pumping a freed
        // plugin is a guaranteed segfault (dogfood crash #22:
        // delete clip + undo while playing).
        if let Some(ref mut mgr) = self.plugin_window_manager {
            mgr.close_all();
        }
        self.plugin_gui_raw_ptrs.clear();
        self.plugin_state_ptrs.clear();

        // Tear down the engine side.
        let existing_track_ids: Vec<TrackId> = self
            .state
            .project_tracks
            .tracks
            .iter()
            .map(|t| t.id)
            .collect();
        for track_id in existing_track_ids {
            self.send_command(EngineCommand::RemoveTrack(track_id));
        }
        // The master bus survives track teardown; clear its chain
        // explicitly so the snapshot replay starts from bare.
        let master_effect_ids: Vec<EffectId> = self
            .state
            .project_tracks
            .master
            .effects
            .iter()
            .map(|e| e.id)
            .collect();
        for effect_id in master_effect_ids {
            self.send_command(EngineCommand::RemoveEffect(TrackId::MASTER, effect_id));
        }

        let bus_ids: Vec<TrackId> = self
            .state
            .project_tracks
            .buses
            .iter()
            .map(|b| b.id)
            .collect();
        for bus_id in bus_ids {
            self.send_command(EngineCommand::RemoveBus(bus_id));
        }

        self.state.project_tracks = Arc::clone(&snapshot.project_tracks);
        self.state
            .perform
            .sync_project_tracks(&self.state.project_tracks.tracks);
        self.restore_snapshot_state(snapshot);

        let tracks = self.state.project_tracks.tracks.clone();
        for track in &tracks {
            self.replay_track_to_engine(track);
        }
        let master = self.state.project_tracks.master.clone();
        self.replay_track_to_engine(&master);
        let buses = self.state.project_tracks.buses.clone();
        for bus in &buses {
            self.replay_bus_to_engine(bus);
        }

        // Reload plugin devices through the project-open pipeline;
        // they re-enter the chains at their recorded positions.
        self.spawn_project_plugin_loads(reloads.effects, reloads.instruments);
        if self
            .state
            .audio_recording
            .armed_track
            .is_some_and(|id| self.state.find_track(id).is_none())
        {
            self.state.audio_recording.armed_track = None;
        }
        self.state.audio_recording.monitor_track = self
            .state
            .audio_recording
            .armed_track
            .or_else(|| self.persisted_monitor_on_track());
        if let Err(error) = self.sync_audio_input_runtime() {
            eprintln!("vibez: Audio Input could not follow project restore: {error}");
        }
    }

    /// Tracks, devices and plugins are unchanged, so they keep running; only
    /// the mixer and device values and the timeline content that differ are
    /// sent to the engine.
    fn swap_changed_values(&mut self, snapshot: crate::state::ProjectSnapshot) {
        let current_tracks = std::mem::replace(
            &mut self.state.project_tracks,
            Arc::clone(&snapshot.project_tracks),
        );
        self.state
            .perform
            .sync_project_tracks(&self.state.project_tracks.tracks);
        let target_tracks = Arc::clone(&self.state.project_tracks);
        for (current, target) in current_tracks.channels().zip(target_tracks.channels()) {
            crate::domains::arrangement::swap_changed_controls(&mut self.cmd_tx, current, target);
        }
        let current = Arc::clone(&self.state.arrangement.timeline);
        self.restore_snapshot_state(snapshot);
        let target = Arc::clone(&self.state.arrangement.timeline);
        let empty = crate::state::TrackTimelineContent::default();
        for channel in self.state.project_tracks.channels() {
            crate::domains::arrangement::swap_changed_content(
                &mut self.cmd_tx,
                channel.id,
                current.get(channel.id).unwrap_or(&empty),
                target.get(channel.id).unwrap_or(&empty),
            );
        }
    }

    /// Everything but the Project Tracks follows the snapshot, and the engine
    /// gets its tempo and loop; channels and their content are the caller's.
    fn restore_snapshot_state(&mut self, snapshot: crate::state::ProjectSnapshot) {
        self.state.arrangement.timeline = snapshot.arrange_timeline;
        self.state.perform.sections = snapshot.sections;
        self.state.perform.clips = snapshot.launcher_clips;
        // Neither may be overtaken, so an unchanged one would hold the next
        // Play behind every content edit still waiting.
        if self.state.transport.bpm != snapshot.bpm {
            self.state.transport.retempo(
                snapshot.bpm,
                &mut crate::domains::EngineTx(&mut self.cmd_tx),
            );
        }
        if self.state.perform.project_swing() != snapshot.project_swing {
            self.send_command(EngineCommand::SetProjectSwing(snapshot.project_swing));
        }
        self.state.perform.set_project_swing(snapshot.project_swing);
        self.state.transport.loop_enabled = snapshot.loop_enabled;
        self.state.transport.loop_start_beats = snapshot.loop_start_beats;
        self.state.transport.loop_end_beats = snapshot.loop_end_beats;
        self.state
            .perform
            .sync_selected_timeline_editor(self.state.arrangement.selected_track);
        self.state.perform.section_name_edit = self
            .state
            .perform
            .selected_section
            .and_then(|id| self.state.perform.sections.by_id(id))
            .map(|section| section.name.clone())
            .unwrap_or_default();
        self.state.perform.editing_section_name = None;
        self.state.perform.duplicate_source = None;

        self.send_command(EngineCommand::SetArrangementLoop(
            self.state.transport.loop_enabled,
        ));
        if self.state.transport.loop_enabled {
            let start = self
                .state
                .transport
                .beats_to_samples(self.state.transport.loop_start_beats);
            let end = self
                .state
                .transport
                .beats_to_samples(self.state.transport.loop_end_beats);
            self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
        }
    }

    pub(super) fn replay_track_to_engine(&mut self, track: &ProjectTrack) {
        let content = self
            .state
            .arrange_content(track.id)
            .cloned()
            .unwrap_or_default();
        if track.id.is_master() {
            // The master bus always exists in the engine: replay only
            // its gain and effect chain.
            self.send_command(EngineCommand::SetTrackGain(track.id, track.gain));
            self.replay_effects_to_engine(track);
            for lane in &content.automation {
                self.send_command(EngineCommand::SetAutomationLane {
                    track_id: track.id,
                    insertion: vibez_engine::clip_insertion::PreparedAutomationLaneInsertion::new(
                        lane.clone(),
                    ),
                });
            }
            return;
        }
        match track.kind {
            TrackKind::Audio => {
                self.send_command(EngineCommand::add_track(track.id));
            }
            TrackKind::Midi | TrackKind::Instrument(_) => {
                self.send_command(EngineCommand::add_track(track.id));
            }
        }
        self.send_command(EngineCommand::SetTrackGain(track.id, track.gain));
        self.send_command(EngineCommand::SetTrackPan(track.id, track.pan));
        self.send_command(EngineCommand::SetTrackMute(track.id, track.mute));
        self.send_command(EngineCommand::SetTrackSolo(track.id, track.solo));
        self.send_command(EngineCommand::SetTrackSwingOffset(
            track.id,
            track.swing_offset,
        ));

        if let Some(kind) = track.instrument_kind {
            self.set_built_in_instrument(track.id, kind);
            for (idx, value) in track.instrument_params.iter().copied().enumerate() {
                self.send_command(EngineCommand::SetInstrumentParam {
                    track_id: track.id,
                    param_index: idx,
                    value,
                });
            }
            match kind {
                InstrumentKind::Sampler => {
                    if let Some(audio) = &track.sample_audio {
                        self.send_command(EngineCommand::LoadSamplerSample {
                            track_id: track.id,
                            sample: Arc::clone(audio),
                            sample_name: track.sample_name.clone().unwrap_or_default(),
                        });
                    }
                }
                InstrumentKind::DrumRack => {
                    for (pad_index, pad) in track.drum_rack_pads.iter().enumerate() {
                        self.send_command(EngineCommand::set_drum_rack_pad_state(
                            track.id,
                            pad_index,
                            pad.to_state(),
                        ));
                        if let Some(audio) = &pad.audio {
                            self.send_command(EngineCommand::LoadDrumRackPadSample {
                                track_id: track.id,
                                pad_index,
                                sample: Arc::clone(audio),
                                sample_name: pad.name.clone().unwrap_or_default(),
                            });
                        }
                    }
                }
                InstrumentKind::SubtractiveSynth
                | InstrumentKind::WavetableSynth
                | InstrumentKind::FmSynth => {}
            }
        }

        for lane in &content.automation {
            self.send_command(EngineCommand::SetAutomationLane {
                track_id: track.id,
                insertion: vibez_engine::clip_insertion::PreparedAutomationLaneInsertion::new(
                    lane.clone(),
                ),
            });
        }

        self.replay_effects_to_engine(track);

        for (bus_id, amount) in &track.sends {
            self.send_command(EngineCommand::SetSend {
                track_id: track.id,
                bus_id: *bus_id,
                amount: *amount,
            });
        }

        for clip in &content.clips {
            self.send_command(clip.add_command(track.id));
        }

        for clip in &content.note_clips {
            self.send_command(clip.add_command(track.id));
        }
    }

    /// Recreate a bus in the engine from its UI model: the channel
    /// itself, then gain/pan/mute and the effect chain.
    fn replay_bus_to_engine(&mut self, bus: &ProjectTrack) {
        let automation = self
            .state
            .arrange_content(bus.id)
            .map(|content| content.automation.clone())
            .unwrap_or_default();
        self.send_command(EngineCommand::add_bus(bus.id));
        self.send_command(EngineCommand::SetTrackGain(bus.id, bus.gain));
        self.send_command(EngineCommand::SetTrackPan(bus.id, bus.pan));
        self.send_command(EngineCommand::SetTrackMute(bus.id, bus.mute));
        self.send_command(EngineCommand::SetTrackSolo(bus.id, bus.solo));
        self.replay_effects_to_engine(bus);
        for lane in &automation {
            self.send_command(EngineCommand::SetAutomationLane {
                track_id: bus.id,
                insertion: vibez_engine::clip_insertion::PreparedAutomationLaneInsertion::new(
                    lane.clone(),
                ),
            });
        }
    }

    /// Replay a channel's built-in effect chain to the engine
    /// (add, params, bypass). Plugin devices reload through the
    /// async pipeline instead.
    fn replay_effects_to_engine(&mut self, track: &ProjectTrack) {
        for effect in &track.effects {
            if effect.plugin_ref.is_some() {
                continue;
            }
            self.insert_built_in_effect(track.id, effect.id, effect.effect_type);
            for (idx, value) in effect.params.iter().copied().enumerate() {
                self.send_command(EngineCommand::SetEffectParam {
                    track_id: track.id,
                    effect_id: effect.id,
                    param_index: idx,
                    value,
                });
            }
            self.send_command(EngineCommand::SetEffectBypass {
                track_id: track.id,
                effect_id: effect.id,
                bypass: effect.bypass,
            });
        }
    }
}

fn clip_only_snapshot(
    state: &crate::state::AppState,
    snapshot: &crate::state::ProjectSnapshot,
) -> bool {
    state.perform.layout == vibez_project::PerformLayout::Clips
        && state.perform.clips.clips.len() == snapshot.launcher_clips.clips.len()
        && state
            .perform
            .clips
            .clips
            .iter()
            .all(|clip| snapshot.launcher_clips.by_id(clip.id).is_some())
        && Arc::ptr_eq(&state.project_tracks, &snapshot.project_tracks)
        && Arc::ptr_eq(&state.arrangement.timeline, &snapshot.arrange_timeline)
        && Arc::ptr_eq(&state.perform.sections, &snapshot.sections)
        && state.transport.bpm == snapshot.bpm
        && state.perform.project_swing() == snapshot.project_swing
        && state.transport.loop_enabled == snapshot.loop_enabled
        && state.transport.loop_start_beats == snapshot.loop_start_beats
        && state.transport.loop_end_beats == snapshot.loop_end_beats
}

#[cfg(test)]
mod clip_snapshot_tests {
    use super::*;

    #[test]
    fn clip_content_undo_preserves_devices_but_track_or_tempo_changes_require_replay() {
        let mut state = crate::state::AppState::default();
        state.perform.layout = vibez_project::PerformLayout::Clips;
        let snapshot = state.project_snapshot();
        assert!(clip_only_snapshot(&state, &snapshot));
        state.transport.bpm += 1.0;
        assert!(!clip_only_snapshot(&state, &snapshot));
        state.transport.bpm = snapshot.bpm;
        Arc::make_mut(&mut state.project_tracks).tracks.clear();
        assert!(!clip_only_snapshot(&state, &snapshot));
        state.project_tracks = Arc::clone(&snapshot.project_tracks);
        state.perform.layout = vibez_project::PerformLayout::Sections;
        assert!(!clip_only_snapshot(&state, &snapshot));
    }
}
