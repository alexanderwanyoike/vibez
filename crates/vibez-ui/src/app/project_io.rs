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
        let has_eq = self
            .state
            .arrangement
            .master
            .effects
            .iter()
            .any(|e| e.effect_type == EffectType::Eq && e.plugin_ref.is_none());
        if !has_eq {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            crate::domains::arrangement::attach_channel_eq(
                &mut engine,
                &mut self.state.arrangement.master,
            );
        }
    }

    pub(super) fn reset_to_new_project(&mut self) {
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
            auto_warp_on_import: self.state.auto_warp_on_import,
            warp_confidence_threshold: self.state.warp_confidence_threshold,
            preferred_midi_input: self.midi_input.as_ref().map(|h| h.port_name.clone()),
            theme: Some(self.state.current_theme_name.clone()),
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
                        MediaSourceRef::DropboxFile { .. } => None,
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
        }
    }

    pub(super) fn rebuild_from_loaded_project(&mut self, loaded: ProjectLoadResult) {
        self.clear_project_runtime();

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
                    .flat_map(|m| m.effects.iter().map(|e| e.id.raw())),
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
            for (chain_pos, effect_info) in master_info.effects.iter().enumerate() {
                if let Some(dev) = &effect_info.plugin {
                    plugin_effect_requests.push((
                        TrackId::MASTER,
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
                self.state.arrangement.master.effects.push(UiEffect {
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
                    track_id: TrackId::MASTER,
                    effect_id: effect_info.id,
                    effect_type: effect_info.effect_type,
                    position: None,
                });
                for (idx, value) in effect_info.params.iter().copied().enumerate() {
                    self.send_command(EngineCommand::SetEffectParam {
                        track_id: TrackId::MASTER,
                        effect_id: effect_info.id,
                        param_index: idx,
                        value,
                    });
                }
                self.send_command(EngineCommand::SetEffectBypass {
                    track_id: TrackId::MASTER,
                    effect_id: effect_info.id,
                    bypass: effect_info.bypass,
                });
            }
        }
        self.ensure_master_eq();

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
        self.state.status_text = if loaded.warnings.is_empty() {
            format!("Opened {}", loaded.path.display())
        } else {
            format!(
                "Opened {} with {} warning(s)",
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

    pub(super) fn take_snapshot(&self) -> crate::state::ProjectSnapshot {
        crate::state::ProjectSnapshot {
            tracks: self.state.arrangement.tracks.clone(),
            master: self.state.arrangement.master.clone(),
            bpm: self.state.transport.bpm,
            bpm_text: self.state.transport.bpm_text.clone(),
            loop_enabled: self.state.transport.loop_enabled,
            loop_start_beats: self.state.transport.loop_start_beats,
            loop_end_beats: self.state.transport.loop_end_beats,
            selected_track: self.state.arrangement.selected_track,
            selected_clips: self.state.arrangement.selected_clips.clone(),
            selected_note_clip: self.state.arrangement.selected_note_clip,
            next_track_number: self.state.arrangement.next_track_number,
        }
    }

    pub(super) fn push_undo_snapshot(&mut self) {
        let snapshot = self.take_snapshot();
        self.state.project.history.push_undo(snapshot);
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
        let existing_track_ids: Vec<TrackId> =
            self.state.arrangement.tracks.iter().map(|t| t.id).collect();
        for track_id in existing_track_ids {
            self.send_command(EngineCommand::RemoveTrack(track_id));
        }
        // The master bus survives track teardown; clear its chain
        // explicitly so the snapshot replay starts from bare.
        let master_effect_ids: Vec<EffectId> = self
            .state
            .arrangement
            .master
            .effects
            .iter()
            .map(|e| e.id)
            .collect();
        for effect_id in master_effect_ids {
            self.send_command(EngineCommand::RemoveEffect(TrackId::MASTER, effect_id));
        }

        self.state.arrangement.master = snapshot.master;
        self.state.arrangement.tracks = snapshot.tracks;
        self.state.transport.bpm = snapshot.bpm;
        self.state.transport.bpm_text = snapshot.bpm_text;
        self.state.transport.loop_enabled = snapshot.loop_enabled;
        self.state.transport.loop_start_beats = snapshot.loop_start_beats;
        self.state.transport.loop_end_beats = snapshot.loop_end_beats;
        self.state.arrangement.selected_track = snapshot.selected_track;
        self.state.arrangement.selected_clips = snapshot.selected_clips;
        self.state.arrangement.selected_note_clip = snapshot.selected_note_clip;
        self.state.arrangement.next_track_number = snapshot.next_track_number;

        self.send_command(EngineCommand::SetBpm(self.state.transport.bpm));
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

        let tracks = self.state.arrangement.tracks.clone();
        for track in &tracks {
            self.replay_track_to_engine(track);
        }
        let master = self.state.arrangement.master.clone();
        self.replay_track_to_engine(&master);

        // Reload plugin devices through the project-open pipeline;
        // they re-enter the chains at their recorded positions.
        self.spawn_project_plugin_loads(reloads.effects, reloads.instruments);
    }

    pub(super) fn replay_track_to_engine(&mut self, track: &UiTrack) {
        if track.id.is_master() {
            // The master bus always exists in the engine: replay only
            // its gain and effect chain.
            self.send_command(EngineCommand::SetTrackGain(track.id, track.gain));
            self.replay_effects_to_engine(track);
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

        for lane in &track.automation {
            self.send_command(EngineCommand::SetAutomationLane {
                track_id: track.id,
                lane: lane.clone(),
            });
        }

        self.replay_effects_to_engine(track);

        for clip in &track.clips {
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

        for clip in &track.note_clips {
            self.send_command(EngineCommand::AddNoteClip {
                track_id: track.id,
                clip_id: clip.id,
                position_beats: clip.position_beats,
                duration_beats: clip.duration_beats,
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
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

    /// Replay a channel's built-in effect chain to the engine
    /// (add, params, bypass). Plugin devices reload through the
    /// async pipeline instead.
    fn replay_effects_to_engine(&mut self, track: &UiTrack) {
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
