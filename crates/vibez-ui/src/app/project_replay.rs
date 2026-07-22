//! Undo snapshots and project-to-engine replay.
//! Split from app/project_io.rs; inherent methods on [`super::App`].

use std::sync::Arc;

use vibez_core::id::{EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_engine::commands::EngineCommand;

use crate::state::ProjectTrack;

use super::*;

impl App {
    pub(super) fn take_snapshot(&self) -> crate::state::ProjectSnapshot {
        crate::state::ProjectSnapshot {
            project_tracks: Arc::clone(&self.state.project_tracks),
            arrange_timeline: Arc::clone(&self.state.arrangement.timeline),
            sections: Arc::clone(&self.state.perform.sections),
            bpm: self.state.transport.bpm,
            bpm_text: self.state.transport.bpm_text.clone(),
            project_swing: self.state.perform.project_swing(),
            loop_enabled: self.state.transport.loop_enabled,
            loop_start_beats: self.state.transport.loop_start_beats,
            loop_end_beats: self.state.transport.loop_end_beats,
            selected_track: self.state.arrangement.selected_track,
            selected_clips: self.state.arrangement.selected_clips.clone(),
            selected_note_clip: self.state.arrangement.selected_note_clip,
            selected_section: self.state.perform.selected_section,
        }
    }

    pub(super) fn push_undo_snapshot(&mut self, gesture: Option<crate::state::UndoGestureId>) {
        let snapshot = self.take_snapshot();
        self.state.project.history.push_edit(snapshot, gesture);
    }

    pub(super) fn apply_snapshot(&mut self, mut snapshot: crate::state::ProjectSnapshot) {
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

        self.state.project_tracks = snapshot.project_tracks;
        self.state
            .perform
            .sync_project_tracks(&self.state.project_tracks.tracks);
        self.state.arrangement.timeline = snapshot.arrange_timeline;
        self.state.perform.sections = snapshot.sections;
        self.state.transport.bpm = snapshot.bpm;
        self.state.transport.bpm_text = snapshot.bpm_text;
        self.state.perform.set_project_swing(snapshot.project_swing);
        self.state.transport.loop_enabled = snapshot.loop_enabled;
        self.state.transport.loop_start_beats = snapshot.loop_start_beats;
        self.state.transport.loop_end_beats = snapshot.loop_end_beats;
        self.state.arrangement.selected_track = snapshot.selected_track;
        self.state.arrangement.selected_clips = snapshot.selected_clips;
        self.state.arrangement.selected_note_clip = snapshot.selected_note_clip;
        self.state.perform.selected_section = snapshot.selected_section;
        self.state
            .perform
            .sync_selected_section_editor(self.state.arrangement.selected_track);
        self.state.perform.section_name_edit = self
            .state
            .perform
            .selected_section
            .and_then(|id| self.state.perform.sections.by_id(id))
            .map(|section| section.name.clone())
            .unwrap_or_default();
        self.state.perform.editing_section_name = None;
        self.state.perform.duplicate_source = None;

        self.send_command(EngineCommand::SetBpm(self.state.transport.bpm));
        self.send_command(EngineCommand::SetProjectSwing(snapshot.project_swing));
        self.send_command(EngineCommand::SetArrangementLoop(
            self.state.transport.loop_enabled,
        ));
        if self.state.transport.loop_enabled {
            let start = self
                .state
                .beats_to_samples(self.state.transport.loop_start_beats);
            let end = self
                .state
                .beats_to_samples(self.state.transport.loop_end_beats);
            self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
        }

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
                    lane: lane.clone(),
                });
            }
            return;
        }
        match track.kind {
            TrackKind::Audio => {
                self.send_command(EngineCommand::AddTrack(track.id, track.name.clone()));
            }
            TrackKind::Midi | TrackKind::Instrument(_) => {
                self.send_command(EngineCommand::AddMidiTrack(track.id, track.name.clone()));
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
            self.send_command(EngineCommand::SetTrackInstrument(track.id, kind));
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
                        self.send_command(EngineCommand::SetDrumRackPadState {
                            track_id: track.id,
                            pad_index,
                            state: pad.to_state(),
                        });
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
                InstrumentKind::SubtractiveSynth => {}
            }
        }

        for lane in &content.automation {
            self.send_command(EngineCommand::SetAutomationLane {
                track_id: track.id,
                lane: lane.clone(),
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
            self.send_command(EngineCommand::AddClip {
                track_id: track.id,
                clip_id: clip.id,
                audio: Arc::clone(&clip.audio),
                position: clip.position,
                source_offset: clip.source_offset,
                duration: clip.duration,
                loop_enabled: clip.loop_enabled,
                loop_start: clip.loop_start,
                loop_end: clip.loop_end,
            });
        }

        for clip in &content.note_clips {
            self.send_command(EngineCommand::AddNoteClip {
                track_id: track.id,
                clip_id: clip.id,
                position_beats: clip.position_beats,
                duration_beats: clip.duration_beats,
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
                groove_grid: clip.groove_grid,
            });
            for note in &clip.notes {
                self.send_command(EngineCommand::AddNote {
                    track_id: track.id,
                    clip_id: clip.id,
                    note: *note,
                });
            }
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
        self.send_command(EngineCommand::AddBus(bus.id, bus.name.clone()));
        self.send_command(EngineCommand::SetTrackGain(bus.id, bus.gain));
        self.send_command(EngineCommand::SetTrackPan(bus.id, bus.pan));
        self.send_command(EngineCommand::SetTrackMute(bus.id, bus.mute));
        self.send_command(EngineCommand::SetTrackSolo(bus.id, bus.solo));
        self.replay_effects_to_engine(bus);
        for lane in &automation {
            self.send_command(EngineCommand::SetAutomationLane {
                track_id: bus.id,
                lane: lane.clone(),
            });
        }
    }

    /// Replay a channel's built-in effect chain to the engine
    /// (add, params, bypass). Plugin devices reload through the
    /// async pipeline instead.
    fn replay_effects_to_engine(&mut self, track: &ProjectTrack) {
        for effect in &track.effects {
            self.send_command(EngineCommand::AddEffect {
                track_id: track.id,
                effect_id: effect.id,
                effect_type: effect.effect_type,
                position: None,
            });
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
