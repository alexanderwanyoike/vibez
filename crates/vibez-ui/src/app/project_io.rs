//! Split out of app.rs; inherent methods on [`super::App`].

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use vibez_core::effect::EffectType;

use iced::Task;

use vibez_core::id::{EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::{ClipInfo, InstrumentStateInfo, MediaSourceRef, TrackInfo};
use vibez_engine::commands::EngineCommand;
use vibez_plugin_host::gui::PluginGuiKey;

use vibez_project::Project;

use crate::message::{Message, ProjectLoadResult};
use crate::state::{UiClip, UiDrumPad, UiEffect, UiNoteClip, UiTrack};
use crate::ui_settings::UiSettings;

use super::*;

impl App {
    pub(super) fn clear_project_runtime(&mut self) {
        // Invalidate any Browser import still preparing (e.g. in its
        // WARP stage) so it cannot add a clip to the reset project.
        self.browser_import_generation = self.browser_import_generation.wrapping_add(1);
        self.stop_browser_audition();
        self.state.transport.playing = false;
        self.state.transport.position_samples = 0;
        self.send_command(EngineCommand::Stop);
        self.send_command(EngineCommand::Seek(0));

        let existing_track_ids: Vec<TrackId> =
            self.state.arrangement.tracks.iter().map(|t| t.id).collect();
        for track_id in existing_track_ids {
            self.send_command(EngineCommand::RemoveTrack(track_id));
        }

        self.state.arrangement.tracks.clear();
        let bus_ids: Vec<TrackId> = self.state.arrangement.buses.iter().map(|b| b.id).collect();
        for bus_id in bus_ids {
            self.send_command(EngineCommand::RemoveBus(bus_id));
        }
        self.state.arrangement.buses.clear();
        self.reset_master_channel();
        // The engine drops all plugin instances with their tracks;
        // their GUI windows and stale raw pointers must go with them
        // (pumping a closed plugin's run-loop timers segfaults).
        if let Some(ref mut mgr) = self.plugin_window_manager {
            mgr.close_all();
        }
        self.plugin_gui_raw_ptrs.clear();
        self.plugin_state_ptrs.clear();
        self.state.arrangement.selected_track = None;
        self.state.arrangement.next_track_number = 1;
        self.state.arrangement.selected_note_clip = None;
        self.state.arrangement.selected_clips.clear();
        self.state.transport.loop_enabled = false;
        self.state.transport.loop_start_beats = 0.0;
        self.state.transport.loop_end_beats = 4.0;
        self.state.arrangement.time_selection_active = false;
        self.state.arrangement.selection_start_beats = 0.0;
        self.state.arrangement.selection_end_beats = 0.0;
        self.state.view.scroll_offset_beats = 0.0;
        self.state.view.context_menu = None;
        self.state.devices.context_menu = None;
        self.state.project.file_menu_open = false;
        self.state.project.unresolved_clips.clear();
        self.state.view.editing_track_name = None;
        self.state.view.editing_clip_name = None;
        self.state.view.edit_name_text.clear();
    }

    /// Tear the master bus back to a bare channel: engine chain
    /// cleared, gain at unity, fresh UI model. Callers re-attach the
    /// channel EQ (or load a saved chain) afterwards.
    pub(super) fn reset_master_channel(&mut self) {
        let effect_ids: Vec<EffectId> = self
            .state
            .arrangement
            .master
            .effects
            .iter()
            .map(|e| e.id)
            .collect();
        for effect_id in effect_ids {
            self.send_command(EngineCommand::RemoveEffect(TrackId::MASTER, effect_id));
        }
        self.state.arrangement.master = crate::state::new_master_track();
        self.send_command(EngineCommand::SetTrackGain(TrackId::MASTER, 1.0));
    }

    /// Console model: the master bus always carries its channel EQ.
    /// Attaches a flat one when missing (fresh session, old project).
    pub(super) fn ensure_master_eq(&mut self) {
        self.ensure_channel_eq(TrackId::MASTER);
    }

    /// Backfill a flat channel EQ on any channel that lacks one
    /// (master, buses, tracks from old project files).
    pub(super) fn ensure_channel_eq(&mut self, chan_id: TrackId) {
        let Some(channel) = self.state.find_track_mut(chan_id) else {
            return;
        };
        let has_eq = channel
            .effects
            .iter()
            .any(|e| e.effect_type == EffectType::Eq && e.plugin_ref.is_none());
        if !has_eq {
            // find_track_mut borrows state; re-resolve inside the
            // engine-handle scope.
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            if let Some(channel) = if chan_id.is_master() {
                Some(&mut self.state.arrangement.master)
            } else {
                self.state
                    .arrangement
                    .tracks
                    .iter_mut()
                    .chain(self.state.arrangement.buses.iter_mut())
                    .find(|t| t.id == chan_id)
            } {
                crate::domains::arrangement::attach_channel_eq(&mut engine, channel);
            }
        }
    }

    /// Rebuild a channel's built-in effect chain from saved
    /// [`EffectInfo`]s, sending the engine commands as it goes.
    /// Plugin-backed slots are queued on `plugin_requests` for the
    /// async reload pipeline instead.
    fn load_saved_effects(
        &mut self,
        effects: &[vibez_core::effect::EffectInfo],
        chan_id: TrackId,
        plugin_requests: &mut Vec<(
            TrackId,
            EffectId,
            usize,
            vibez_core::effect::PluginDeviceInfo,
        )>,
    ) -> Vec<UiEffect> {
        let mut out = Vec::new();
        for (chain_pos, effect_info) in effects.iter().enumerate() {
            if let Some(dev) = &effect_info.plugin {
                plugin_requests.push((chan_id, effect_info.id, chain_pos, dev.clone()));
                continue;
            }
            let fx = vibez_dsp::factory::create_effect_with_params(
                effect_info.effect_type,
                self.state.transport.sample_rate as f32,
                &effect_info.params,
            );
            out.push(UiEffect {
                id: effect_info.id,
                effect_type: effect_info.effect_type,
                bypass: effect_info.bypass,
                params: effect_info.params.clone(),
                descriptors: fx.param_descriptors(),
                plugin_name: None,
                has_plugin_gui: false,
                plugin_ref: None,
            });
            self.send_command(EngineCommand::AddEffect {
                track_id: chan_id,
                effect_id: effect_info.id,
                effect_type: effect_info.effect_type,
                position: None,
            });
            for (idx, value) in effect_info.params.iter().copied().enumerate() {
                self.send_command(EngineCommand::SetEffectParam {
                    track_id: chan_id,
                    effect_id: effect_info.id,
                    param_index: idx,
                    value,
                });
            }
            self.send_command(EngineCommand::SetEffectBypass {
                track_id: chan_id,
                effect_id: effect_info.id,
                bypass: effect_info.bypass,
            });
        }
        out
    }

    pub(super) fn reset_to_new_project(&mut self) {
        if let Some(handle) = self.remote_import_abort.take() {
            handle.abort();
            self.remote_import_request_id = self.remote_import_request_id.saturating_add(1);
            self.remote_import_in_flight = None;
        }
        if let Some(handle) = self.remote_materialization_abort.take() {
            handle.abort();
            self.remote_materialization_request_id =
                self.remote_materialization_request_id.saturating_add(1);
        }
        self.remote_audition_cache_lease = None;
        let _ = self.dropbox_cache.set_policy(self.dropbox_cache.policy());
        self.clear_project_runtime();
        self.ensure_master_eq();
        self.state.transport.bpm = vibez_core::constants::DEFAULT_BPM;
        self.state.transport.bpm_text = format!("{:.0}", self.state.transport.bpm);
        self.send_command(EngineCommand::SetBpm(self.state.transport.bpm));
        self.state.project.current_path = None;
        self.state.project.dirty = false;
        self.state.project.history.clear();
        self.state.status_text = "New project".to_string();
    }

    pub(super) fn persist_ui_settings(&mut self) {
        let settings = UiSettings {
            sample_library_roots: self.state.browser.roots.clone(),
            sample_browser_open: self.state.browser.open,
            sample_browser_width: self.state.browser.dock_width,
            audition_enabled: self.state.browser.audition_enabled,
            audition_gain: self.state.browser.audition_gain,
            audition_loop: self.state.browser.audition_loop,
            auto_warp_on_import: self.state.auto_warp_on_import,
            warp_confidence_threshold: self.state.warp_confidence_threshold,
            preferred_midi_input: self.midi_input.as_ref().map(|h| h.port_name.clone()),
            theme: Some(self.state.current_theme_name.clone()),
            media_cache_budget_bytes: self.state.browser.remote.cache_budget_bytes,
            media_cache_automatic_eviction: self.state.browser.remote.cache_automatic_eviction,
        };
        if let Err(err) = settings.save() {
            self.state.status_text = format!("UI settings save error: {err}");
        }
    }

    pub(super) fn track_info_from_ui(&self, track: &UiTrack) -> TrackInfo {
        let effects = track
            .effects
            .iter()
            .map(|effect| {
                let plugin = effect.plugin_ref.as_ref().map(|dev| {
                    let mut dev = dev.clone();
                    dev.state_b64 = self.capture_device_state(PluginGuiKey::Effect {
                        track_id: track.id,
                        effect_id: effect.id,
                    });
                    dev
                });
                vibez_core::effect::EffectInfo {
                    id: effect.id,
                    effect_type: effect.effect_type,
                    bypass: effect.bypass,
                    params: effect.params.clone(),
                    plugin,
                }
            })
            .collect();

        let plugin_instrument = track.plugin_instrument_ref.as_ref().map(|dev| {
            let mut dev = dev.clone();
            dev.state_b64 =
                self.capture_device_state(PluginGuiKey::Instrument { track_id: track.id });
            dev
        });

        let native_instrument = match track.instrument_kind {
            Some(InstrumentKind::SubtractiveSynth) => Some(InstrumentStateInfo::SubtractiveSynth {
                params: track.instrument_params.clone(),
            }),
            Some(InstrumentKind::Sampler) => Some(InstrumentStateInfo::Sampler {
                params: track.instrument_params.clone(),
                source: track.sample_source.clone(),
            }),
            Some(InstrumentKind::DrumRack) => Some(InstrumentStateInfo::DrumRack {
                pads: track
                    .drum_rack_pads
                    .iter()
                    .map(UiDrumPad::to_state)
                    .collect(),
            }),
            None => None,
        };

        TrackInfo {
            id: track.id,
            name: track.name.clone(),
            gain: track.gain,
            pan: track.pan,
            mute: track.mute,
            solo: track.solo,
            effects,
            kind: track.kind,
            color_index: track.color_index,
            instrument: track.instrument_kind,
            native_instrument,
            plugin_instrument,
            automation: track.automation.clone(),
            sends: track.sends.clone(),
        }
    }

    /// Base64-encoded live state of a plugin device, captured on the
    /// UI thread via the pointer stashed at load time.
    pub(super) fn capture_device_state(&self, key: PluginGuiKey) -> Option<String> {
        use base64::Engine;
        let ptr = self.plugin_state_ptrs.get(&key)?;
        let data = unsafe { vibez_plugin_host::capture_plugin_state(ptr) }?;
        Some(base64::engine::general_purpose::STANDARD.encode(data))
    }

    pub(super) fn project_from_state(&self) -> Project {
        let tracks = self
            .state
            .arrangement
            .tracks
            .iter()
            .map(|track| self.track_info_from_ui(track))
            .collect();

        let clips = self
            .state
            .arrangement
            .tracks
            .iter()
            .flat_map(|track| {
                track.clips.iter().map(|clip| ClipInfo {
                    id: clip.id,
                    track_id: track.id,
                    name: clip.name.clone(),
                    position: clip.position,
                    source_offset: clip.source_offset,
                    duration: clip.duration,
                    source: clip.source.clone(),
                    file_path: clip.source.as_ref().and_then(|source| match source {
                        MediaSourceRef::LocalFile { path } => Some(path.clone()),
                        MediaSourceRef::StagedProjectMedia { .. }
                        | MediaSourceRef::StagedRemoteProjectMedia { .. }
                        | MediaSourceRef::ProjectMedia { .. }
                        | MediaSourceRef::DropboxFile { .. } => None,
                    }),
                    loop_enabled: clip.loop_enabled,
                    loop_start: clip.loop_start,
                    loop_end: clip.loop_end,
                    original_bpm: clip.original_bpm,
                    warped: clip.warped,
                    warped_to_bpm: clip.warped_to_bpm,
                })
            })
            .collect();

        let note_clips = self
            .state
            .arrangement
            .tracks
            .iter()
            .flat_map(|track| {
                track
                    .note_clips
                    .iter()
                    .map(|clip| vibez_core::midi::NoteClipInfo {
                        id: clip.id,
                        track_id: track.id,
                        name: clip.name.clone(),
                        position_beats: clip.position_beats,
                        duration_beats: clip.duration_beats,
                        notes: clip.notes.clone(),
                        loop_enabled: clip.loop_enabled,
                        loop_start_beats: clip.loop_start_beats,
                        loop_end_beats: clip.loop_end_beats,
                    })
            })
            .collect();

        Project {
            name: self
                .state
                .project
                .current_path
                .as_ref()
                .and_then(|path| path.file_stem())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string()),
            bpm: self.state.transport.bpm,
            sample_rate: self.state.transport.sample_rate,
            tracks,
            clips,
            note_clips,
            master: Some(self.track_info_from_ui(&self.state.arrangement.master)),
            buses: self
                .state
                .arrangement
                .buses
                .iter()
                .map(|bus| self.track_info_from_ui(bus))
                .collect(),
        }
    }

    /// The full document a save must persist: the editable arrangement
    /// plus clips whose media was unavailable at load time. Bounce and
    /// export use [`Self::project_from_state`] directly, since unresolved
    /// clips have no audio to render.
    pub(super) fn project_for_save(&self) -> Project {
        let mut project = self.project_from_state();
        project
            .clips
            .extend(self.state.project.unresolved_clips.iter().cloned());
        project
    }

    pub(super) fn apply_saved_project_sources(&mut self, project: &Project) {
        for saved_clip in &project.clips {
            if let Some(track) = self.state.find_track_mut(saved_clip.track_id) {
                if let Some(clip) = track.clips.iter_mut().find(|clip| clip.id == saved_clip.id) {
                    clip.source = saved_clip.source.clone();
                }
            }
        }
        for saved_track in &project.tracks {
            let Some(track) = self.state.find_track_mut(saved_track.id) else {
                continue;
            };
            match &saved_track.native_instrument {
                Some(InstrumentStateInfo::Sampler { source, .. }) => {
                    track.sample_source = source.clone();
                }
                Some(InstrumentStateInfo::DrumRack { pads }) => {
                    for (slot, saved_pad) in track.drum_rack_pads.iter_mut().zip(pads) {
                        slot.source = saved_pad.source.clone();
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn rebuild_from_loaded_project(&mut self, loaded: ProjectLoadResult) {
        let remote_provenance = first_remote_provenance_label(&loaded.project);
        self.clear_project_runtime();
        self.state.project.unresolved_clips = loaded.unresolved_clips;

        // Seed the global id counter past every persisted id BEFORE
        // anything new is created: loaded ids come from a previous
        // session's counter, and a collision makes two objects
        // answer to the same id (double selection, engine commands
        // hitting both).
        let max_loaded_id = loaded
            .project
            .tracks
            .iter()
            .flat_map(|t| std::iter::once(t.id.raw()).chain(t.effects.iter().map(|e| e.id.raw())))
            .chain(loaded.project.clips.iter().map(|c| c.id.raw()))
            .chain(loaded.project.note_clips.iter().map(|c| c.id.raw()))
            .chain(
                loaded
                    .project
                    .master
                    .iter()
                    .chain(loaded.project.buses.iter())
                    .flat_map(|m| {
                        std::iter::once(m.id.raw()).chain(m.effects.iter().map(|e| e.id.raw()))
                    }),
            )
            .max()
            .unwrap_or(0);
        vibez_core::id::ensure_ids_above(max_loaded_id);
        // Third-party plugin devices load asynchronously after the
        // built-in rebuild; collected here, spawned at the end.
        let mut plugin_effect_requests: Vec<(
            TrackId,
            EffectId,
            usize,
            vibez_core::effect::PluginDeviceInfo,
        )> = Vec::new();
        let mut plugin_instrument_requests: Vec<(TrackId, vibez_core::effect::PluginDeviceInfo)> =
            Vec::new();
        self.state.project.history.clear();
        self.state.transport.bpm = loaded.project.bpm;
        self.state.transport.bpm_text = format!("{:.0}", loaded.project.bpm);
        self.send_command(EngineCommand::SetBpm(loaded.project.bpm));

        for track_info in &loaded.project.tracks {
            let mut track = UiTrack::new_instrument(
                track_info.id,
                track_info.name.clone(),
                track_info.kind,
                track_info.color_index,
            );
            track.gain = track_info.gain;
            track.pan = track_info.pan;
            track.mute = track_info.mute;
            track.solo = track_info.solo;
            track.automation = track_info.automation.clone();
            track.instrument_kind = track_info.instrument;
            track.has_instrument = track_info.instrument.is_some();
            if let Some(dev) = &track_info.plugin_instrument {
                plugin_instrument_requests.push((track_info.id, dev.clone()));
            }

            match track.kind {
                TrackKind::Audio => {
                    self.send_command(EngineCommand::AddTrack(
                        track_info.id,
                        track_info.name.clone(),
                    ));
                }
                TrackKind::Midi | TrackKind::Instrument(_) => {
                    self.send_command(EngineCommand::AddMidiTrack(
                        track_info.id,
                        track_info.name.clone(),
                    ));
                }
            }

            self.send_command(EngineCommand::SetTrackGain(track_info.id, track_info.gain));
            self.send_command(EngineCommand::SetTrackPan(track_info.id, track_info.pan));
            self.send_command(EngineCommand::SetTrackMute(track_info.id, track_info.mute));
            self.send_command(EngineCommand::SetTrackSolo(track_info.id, track_info.solo));

            if let Some(kind) = track_info.instrument {
                self.send_command(EngineCommand::SetTrackInstrument(track_info.id, kind));
            }

            if let Some(native) = &track_info.native_instrument {
                match native {
                    InstrumentStateInfo::SubtractiveSynth { params } => {
                        track.instrument_params = params.clone();
                        for (idx, value) in params.iter().copied().enumerate() {
                            self.send_command(EngineCommand::SetInstrumentParam {
                                track_id: track_info.id,
                                param_index: idx,
                                value,
                            });
                        }
                    }
                    InstrumentStateInfo::Sampler { params, source } => {
                        track.instrument_params = params.clone();
                        track.sample_source = source.clone();
                        track.sample_name = source.as_ref().map(MediaSourceRef::display_name);
                        for (idx, value) in params.iter().copied().enumerate() {
                            self.send_command(EngineCommand::SetInstrumentParam {
                                track_id: track_info.id,
                                param_index: idx,
                                value,
                            });
                        }
                    }
                    InstrumentStateInfo::DrumRack { pads } => {
                        track.drum_rack_pads = pads.iter().map(UiDrumPad::from_state).collect();
                        track.selected_drum_pad = 0;
                        for (pad_index, pad) in pads.iter().cloned().enumerate() {
                            self.send_command(EngineCommand::SetDrumRackPadState {
                                track_id: track_info.id,
                                pad_index,
                                state: pad,
                            });
                        }
                    }
                }
            }

            for lane in &track_info.automation {
                self.send_command(EngineCommand::SetAutomationLane {
                    track_id: track_info.id,
                    lane: lane.clone(),
                });
            }

            track.sends = track_info.sends.clone();
            for (bus_id, amount) in &track_info.sends {
                self.send_command(EngineCommand::SetSend {
                    track_id: track_info.id,
                    bus_id: *bus_id,
                    amount: *amount,
                });
            }

            for (chain_pos, effect_info) in track_info.effects.iter().enumerate() {
                if let Some(dev) = &effect_info.plugin {
                    plugin_effect_requests.push((
                        track_info.id,
                        effect_info.id,
                        chain_pos,
                        dev.clone(),
                    ));
                    continue;
                }
                let fx = vibez_dsp::factory::create_effect_with_params(
                    effect_info.effect_type,
                    self.state.transport.sample_rate as f32,
                    &effect_info.params,
                );
                let descriptors = fx.param_descriptors();
                track.effects.push(UiEffect {
                    id: effect_info.id,
                    effect_type: effect_info.effect_type,
                    bypass: effect_info.bypass,
                    params: effect_info.params.clone(),
                    descriptors,
                    plugin_name: None,
                    has_plugin_gui: false,
                    plugin_ref: None,
                });
                self.send_command(EngineCommand::AddEffect {
                    track_id: track_info.id,
                    effect_id: effect_info.id,
                    effect_type: effect_info.effect_type,
                    position: None,
                });
                for (idx, value) in effect_info.params.iter().copied().enumerate() {
                    self.send_command(EngineCommand::SetEffectParam {
                        track_id: track_info.id,
                        effect_id: effect_info.id,
                        param_index: idx,
                        value,
                    });
                }
                self.send_command(EngineCommand::SetEffectBypass {
                    track_id: track_info.id,
                    effect_id: effect_info.id,
                    bypass: effect_info.bypass,
                });
            }

            // Console model: every channel has its EQ. Backfill for
            // projects saved before the channel EQ existed.
            if !track
                .effects
                .iter()
                .any(|e| e.effect_type == EffectType::Eq && e.plugin_ref.is_none())
            {
                let effect_id = EffectId::new();
                let fx = vibez_dsp::factory::create_effect(
                    EffectType::Eq,
                    self.state.transport.sample_rate as f32,
                );
                let descriptors = fx.param_descriptors();
                track.effects.push(UiEffect {
                    id: effect_id,
                    effect_type: EffectType::Eq,
                    bypass: false,
                    params: descriptors.iter().map(|d| d.default).collect(),
                    descriptors,
                    plugin_name: None,
                    has_plugin_gui: false,
                    plugin_ref: None,
                });
                self.send_command(EngineCommand::AddEffect {
                    track_id: track_info.id,
                    effect_id,
                    effect_type: EffectType::Eq,
                    position: None,
                });
            }

            self.state.arrangement.next_track_number = self
                .state
                .arrangement
                .next_track_number
                .max(self.state.arrangement.tracks.len() as u32 + 1);
            self.state.arrangement.tracks.push(track);
        }

        // Master bus: gain + effect chain from the file, then the
        // channel-EQ backfill for projects saved before the master
        // was a real channel. clear_project_runtime left it bare.
        if let Some(master_info) = &loaded.project.master {
            self.state.arrangement.master.gain = master_info.gain;
            self.send_command(EngineCommand::SetTrackGain(
                TrackId::MASTER,
                master_info.gain,
            ));
            let effects = self.load_saved_effects(
                &master_info.effects,
                TrackId::MASTER,
                &mut plugin_effect_requests,
            );
            self.state.arrangement.master.effects = effects;
            self.state.arrangement.master.automation = master_info.automation.clone();
            for lane in &master_info.automation {
                self.send_command(EngineCommand::SetAutomationLane {
                    track_id: TrackId::MASTER,
                    lane: lane.clone(),
                });
            }
        }
        self.ensure_master_eq();

        // Return buses: recreate each channel, then its chain; sends
        // were restored with their tracks above.
        for bus_info in &loaded.project.buses {
            self.send_command(EngineCommand::AddBus(bus_info.id, bus_info.name.clone()));
            let mut bus = UiTrack::new(bus_info.id, bus_info.name.clone(), bus_info.color_index);
            bus.gain = bus_info.gain;
            bus.pan = bus_info.pan;
            bus.mute = bus_info.mute;
            bus.solo = bus_info.solo;
            self.send_command(EngineCommand::SetTrackGain(bus_info.id, bus_info.gain));
            self.send_command(EngineCommand::SetTrackPan(bus_info.id, bus_info.pan));
            self.send_command(EngineCommand::SetTrackMute(bus_info.id, bus_info.mute));
            self.send_command(EngineCommand::SetTrackSolo(bus_info.id, bus_info.solo));
            bus.effects = self.load_saved_effects(
                &bus_info.effects,
                bus_info.id,
                &mut plugin_effect_requests,
            );
            bus.automation = bus_info.automation.clone();
            for lane in &bus_info.automation {
                self.send_command(EngineCommand::SetAutomationLane {
                    track_id: bus_info.id,
                    lane: lane.clone(),
                });
            }
            self.state.arrangement.buses.push(bus);
            self.ensure_channel_eq(bus_info.id);
        }

        for loaded_clip in loaded.clips {
            self.send_command(EngineCommand::AddClip {
                track_id: loaded_clip.info.track_id,
                clip_id: loaded_clip.info.id,
                audio: Arc::clone(&loaded_clip.audio),
                position: loaded_clip.info.position,
                source_offset: loaded_clip.info.source_offset,
                duration: loaded_clip.info.duration,
                loop_enabled: loaded_clip.info.loop_enabled,
                loop_start: loaded_clip.info.loop_start,
                loop_end: loaded_clip.info.loop_end,
            });

            if let Some(track) = self.state.find_track_mut(loaded_clip.info.track_id) {
                track.clips.push(UiClip {
                    id: loaded_clip.info.id,
                    name: loaded_clip.info.name,
                    audio: loaded_clip.audio,
                    source: loaded_clip.info.source.clone(),
                    position: loaded_clip.info.position,
                    source_offset: loaded_clip.info.source_offset,
                    duration: loaded_clip.info.duration,
                    loop_enabled: loaded_clip.info.loop_enabled,
                    loop_start: loaded_clip.info.loop_start,
                    loop_end: loaded_clip.info.loop_end,
                    original_bpm: loaded_clip.info.original_bpm,
                    warped: loaded_clip.info.warped,
                    warped_to_bpm: loaded_clip.info.warped_to_bpm,
                    original_audio: loaded_clip.original_audio,
                });
            }
        }

        for note_clip in &loaded.project.note_clips {
            self.send_command(EngineCommand::AddNoteClip {
                track_id: note_clip.track_id,
                clip_id: note_clip.id,
                position_beats: note_clip.position_beats,
                duration_beats: note_clip.duration_beats,
                loop_enabled: note_clip.loop_enabled,
                loop_start_beats: note_clip.loop_start_beats,
                loop_end_beats: note_clip.loop_end_beats,
            });
            for note in &note_clip.notes {
                self.send_command(EngineCommand::AddNote {
                    track_id: note_clip.track_id,
                    clip_id: note_clip.id,
                    note: *note,
                });
            }
            if let Some(track) = self.state.find_track_mut(note_clip.track_id) {
                track.note_clips.push(UiNoteClip {
                    id: note_clip.id,
                    name: note_clip.name.clone(),
                    position_beats: note_clip.position_beats,
                    duration_beats: note_clip.duration_beats,
                    notes: note_clip.notes.clone(),
                    selected_notes: HashSet::new(),
                    loop_enabled: note_clip.loop_enabled,
                    loop_start_beats: note_clip.loop_start_beats,
                    loop_end_beats: note_clip.loop_end_beats,
                });
            }
        }

        for sampler in loaded.sampler_samples {
            if let Some(track) = self.state.find_track_mut(sampler.track_id) {
                track.sample_name = Some(sampler.name.clone());
                track.sample_source = Some(sampler.source.clone());
                track.sample_audio = Some(Arc::clone(&sampler.audio));
            }
            self.send_command(EngineCommand::LoadSamplerSample {
                track_id: sampler.track_id,
                sample: sampler.audio,
                sample_name: sampler.name,
            });
        }

        for pad in loaded.drum_rack_pad_samples {
            if let Some(track) = self.state.find_track_mut(pad.track_id) {
                if let Some(slot) = track.drum_rack_pads.get_mut(pad.pad_index) {
                    *slot = UiDrumPad::from_state(&pad.state);
                    slot.name = Some(pad.name.clone());
                    slot.audio = Some(Arc::clone(&pad.audio));
                }
            }
            self.send_command(EngineCommand::SetDrumRackPadState {
                track_id: pad.track_id,
                pad_index: pad.pad_index,
                state: pad.state,
            });
            self.send_command(EngineCommand::LoadDrumRackPadSample {
                track_id: pad.track_id,
                pad_index: pad.pad_index,
                sample: pad.audio,
                sample_name: pad.name,
            });
        }

        self.state.arrangement.selected_track =
            self.state.arrangement.tracks.first().map(|track| track.id);
        self.state.project.current_path = Some(loaded.path.clone());
        self.state.project.dirty = false;
        let provenance_suffix = remote_provenance
            .map(|label| format!(" · Remote source {label}"))
            .unwrap_or_default();
        self.state.status_text = if loaded.warnings.is_empty() {
            format!("Opened {}{provenance_suffix}", loaded.path.display())
        } else {
            format!(
                "Opened {} with {} warning(s){provenance_suffix}",
                loaded.path.display(),
                loaded.warnings.len()
            )
        };

        self.spawn_project_plugin_loads(plugin_effect_requests, plugin_instrument_requests);
    }

    /// Reload persisted plugin devices through the background loader
    /// service. Results flow through the same channels as interactive
    /// plugin loads.
    pub(super) fn spawn_project_plugin_loads(
        &mut self,
        effect_requests: Vec<(
            TrackId,
            EffectId,
            usize,
            vibez_core::effect::PluginDeviceInfo,
        )>,
        instrument_requests: Vec<(TrackId, vibez_core::effect::PluginDeviceInfo)>,
    ) {
        if effect_requests.is_empty() && instrument_requests.is_empty() {
            return;
        }
        let n = effect_requests.len() + instrument_requests.len();
        self.state.status_text = format!("Loading {n} plugin(s)...");
        crate::services::plugin_loader::spawn_device_reloads(
            effect_requests,
            instrument_requests,
            self.plugin_effect_tx.clone(),
            self.plugin_instrument_tx.clone(),
            self.state.transport.sample_rate as f64,
        );
    }

    pub(super) fn handle_export_path_selected(&mut self, path: Option<PathBuf>) -> Task<Message> {
        let Some(mut path) = path else {
            return Task::none();
        };
        if path.extension().is_none() {
            path.set_extension("wav");
        }
        let total = self.state.total_duration_samples();
        if total == 0 {
            self.state.status_text = "Nothing to export: project is empty".to_string();
            return Task::none();
        }
        let assets = self.collect_bounce_assets();
        let project = self.project_from_state();
        let sample_rate = self.state.transport.sample_rate;
        let bpm = self.state.transport.bpm;
        let request = vibez_engine::render::BounceRequest {
            tracks: project.tracks,
            master: project.master,
            buses: project.buses,
            audio_clips: project.clips,
            note_clips: project.note_clips,
            clip_audio: assets.clips,
            sampler_audio: assets.samplers,
            drum_pad_audio: assets.pads,
            mode: vibez_engine::render::BounceMode::Master,
            range_samples: (0, total),
            bpm,
            sample_rate,
        };
        self.state.status_text = format!("Exporting to {}...", path.display());
        Task::perform(export_async(request, path), Message::ExportComplete)
    }
}

fn first_remote_provenance_label(project: &Project) -> Option<String> {
    let track_sources = project.tracks.iter().flat_map(|track| {
        let sampler = match &track.native_instrument {
            Some(InstrumentStateInfo::Sampler { source, .. }) => source.iter().collect::<Vec<_>>(),
            Some(InstrumentStateInfo::DrumRack { pads }) => {
                pads.iter().filter_map(|pad| pad.source.as_ref()).collect()
            }
            _ => Vec::new(),
        };
        sampler
    });
    project
        .clips
        .iter()
        .filter_map(|clip| clip.source.as_ref())
        .chain(track_sources)
        .filter_map(MediaSourceRef::provenance)
        .find_map(|provenance| match provenance {
            vibez_core::track::MediaProvenance::Remote { .. } => Some(provenance.display_label()),
            vibez_core::track::MediaProvenance::Local { .. } => None,
        })
}
