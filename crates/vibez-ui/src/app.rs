use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, scrollable,
    stack, text, text_input, vertical_space,
};
use iced::{Color, Element, Length, Subscription, Task, Theme};

use rtrb::{Consumer, Producer};
use vibez_audio_io::audio_stream::AudioOutputStream;
use vibez_audio_io::file_io;
use vibez_core::constants::UI_TICK_MS;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::track::{ClipInfo, InstrumentStateInfo, MediaSourceRef, TrackInfo};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;
use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::{PluginCategory, PluginFormat, PluginInfo, PluginInstance};
use vibez_project::Project;

use crate::icons;
use crate::message::{LoadedClipData, LoadedSamplerData, Message, ProjectLoadResult};
use crate::plugin_window::{PluginRawPtr, PluginWindowEvent, PluginWindowManager};
use crate::state::{
    AppState, ArrangementSelection, ContextMenuTarget, DetailPanelTab, SettingsTab, UiClip,
    UiEffect, UiNoteClip, UiTrack, Workspace,
};
use crate::theme as th;
use crate::widgets::audio_clip_detail::AudioClipDetailWidget;
use crate::widgets::effect_slot::view_effect_slot;
use crate::widgets::mixer_strip::view_mixer_strip;
use crate::widgets::piano_roll::PianoRollWidget;
use crate::widgets::timeline::{ArrangementMinimap, MinimapTrack, RulerWidget, TrackClipCanvas};
use crate::widgets::track_header::view_track_header;
use crate::widgets::vu_meter::VuMeterWidget;

/// Result of loading a plugin on a background thread.
/// For CLAP plugins, `clap_partial` carries an un-initialized plugin that
/// must be finished on the UI thread (for JUCE MessageManager compatibility).
struct PluginLoadResult {
    track_id: TrackId,
    effect_id: EffectId,
    plugin_name: String,
    /// Fully-loaded effect (VST3) or None (CLAP — see `clap_partial`).
    effect: Option<Box<dyn vibez_dsp::effect::AudioEffect>>,
    gui_raw_ptr: Option<PluginRawPtr>,
    /// CLAP two-phase: partially loaded plugin to be finished on UI thread.
    clap_partial: Option<vibez_plugin_host::clap_host::instance::PartialClapPlugin>,
    sample_rate: f64,
}

/// Result of loading a plugin instrument on a background thread.
struct PluginInstrumentLoadResult {
    track_id: TrackId,
    plugin_name: String,
    /// Fully-loaded instrument (VST3) or None (CLAP — see `clap_partial`).
    instrument: Option<Box<dyn vibez_instruments::Instrument>>,
    gui_raw_ptr: Option<PluginRawPtr>,
    /// CLAP two-phase: partially loaded plugin to be finished on UI thread.
    clap_partial: Option<vibez_plugin_host::clap_host::instance::PartialClapPlugin>,
    sample_rate: f64,
}

struct App {
    state: AppState,
    cmd_tx: Option<Producer<EngineCommand>>,
    event_rx: Option<Consumer<EngineEvent>>,
    _stream: Option<AudioOutputStream>,
    // Channels for receiving loaded plugins from background threads
    plugin_effect_rx: std::sync::mpsc::Receiver<PluginLoadResult>,
    plugin_effect_tx: std::sync::mpsc::Sender<PluginLoadResult>,
    plugin_instrument_rx: std::sync::mpsc::Receiver<PluginInstrumentLoadResult>,
    plugin_instrument_tx: std::sync::mpsc::Sender<PluginInstrumentLoadResult>,
    // Plugin GUI support
    plugin_window_manager: Option<PluginWindowManager>,
    plugin_gui_raw_ptrs: std::collections::HashMap<PluginGuiKey, PluginRawPtr>,
}

pub fn run() -> iced::Result {
    iced::application("vibez", App::update, App::view)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1400.0, 900.0))
        .font(icons::ICON_FONT_BYTES)
        .run_with(App::new)
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (engine, cmd_tx, event_rx) = AudioEngine::new();

        let (stream, sample_rate) = match AudioOutputStream::open(engine, Some(512)) {
            Ok(s) => {
                let sr = s.sample_rate();
                if let Err(e) = s.play() {
                    eprintln!("vibez: failed to start audio stream: {e}");
                }
                (Some(s), sr)
            }
            Err(e) => {
                eprintln!("vibez: failed to open audio stream: {e}");
                (None, 44_100)
            }
        };

        let state = AppState {
            sample_rate,
            ..Default::default()
        };

        let (plugin_effect_tx, plugin_effect_rx) = std::sync::mpsc::channel();
        let (plugin_instrument_tx, plugin_instrument_rx) = std::sync::mpsc::channel();

        // Register the UI thread (process main thread) as the CLAP "main thread".
        // JUCE-based CLAP plugins require GUI calls on this thread.
        vibez_plugin_host::set_clap_main_thread();

        let plugin_window_manager = PluginWindowManager::new();

        let mut app = Self {
            state,
            cmd_tx: Some(cmd_tx),
            event_rx: Some(event_rx),
            _stream: stream,
            plugin_effect_rx,
            plugin_effect_tx,
            plugin_instrument_rx,
            plugin_instrument_tx,
            plugin_window_manager,
            plugin_gui_raw_ptrs: std::collections::HashMap::new(),
        };

        // Inform the engine of the actual sample rate
        app.send_command(EngineCommand::SetBpm(app.state.bpm));

        (app, Task::none())
    }

    fn send_command(&mut self, cmd: EngineCommand) {
        if let Some(ref mut tx) = self.cmd_tx {
            let _ = tx.push(cmd);
        }
    }

    fn mark_project_dirty(&mut self) {
        self.state.project_dirty = true;
    }

    fn clear_project_runtime(&mut self) {
        self.state.playing = false;
        self.state.position_samples = 0;
        self.send_command(EngineCommand::Stop);
        self.send_command(EngineCommand::Seek(0));

        let existing_track_ids: Vec<TrackId> = self.state.tracks.iter().map(|t| t.id).collect();
        for track_id in existing_track_ids {
            self.send_command(EngineCommand::RemoveTrack(track_id));
        }

        self.state.tracks.clear();
        self.state.selected_track = None;
        self.state.next_track_number = 1;
        self.state.selected_note_clip = None;
        self.state.selected_clips.clear();
        self.state.loop_enabled = false;
        self.state.loop_start_beats = 0.0;
        self.state.loop_end_beats = 4.0;
        self.state.time_selection_active = false;
        self.state.selection_start_beats = 0.0;
        self.state.selection_end_beats = 0.0;
        self.state.scroll_offset_beats = 0.0;
        self.state.context_menu = None;
        self.state.device_context_menu = None;
        self.state.file_menu_open = false;
        self.state.editing_track_name = None;
        self.state.editing_clip_name = None;
        self.state.edit_name_text.clear();
    }

    fn reset_to_new_project(&mut self) {
        self.clear_project_runtime();
        self.state.bpm = vibez_core::constants::DEFAULT_BPM;
        self.state.bpm_text = format!("{:.0}", self.state.bpm);
        self.send_command(EngineCommand::SetBpm(self.state.bpm));
        self.state.current_project_path = None;
        self.state.project_dirty = false;
        self.state.status_text = "New project".to_string();
    }

    fn track_info_from_ui(&self, track: &UiTrack) -> TrackInfo {
        let effects = track
            .effects
            .iter()
            .filter(|effect| effect.plugin_name.is_none())
            .map(|effect| vibez_core::effect::EffectInfo {
                id: effect.id,
                effect_type: effect.effect_type,
                bypass: effect.bypass,
                params: effect.params.clone(),
            })
            .collect();

        let native_instrument = match track.instrument_kind {
            Some(InstrumentKind::SubtractiveSynth) => Some(InstrumentStateInfo::SubtractiveSynth {
                params: track.instrument_params.clone(),
            }),
            Some(InstrumentKind::Sampler) => Some(InstrumentStateInfo::Sampler {
                params: track.instrument_params.clone(),
                source: track.sample_source.clone(),
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
        }
    }

    fn project_from_state(&self) -> Project {
        let tracks = self
            .state
            .tracks
            .iter()
            .map(|track| self.track_info_from_ui(track))
            .collect();

        let clips = self
            .state
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
                })
            })
            .collect();

        let note_clips = self
            .state
            .tracks
            .iter()
            .flat_map(|track| {
                track.note_clips.iter().map(|clip| vibez_core::midi::NoteClipInfo {
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
                .current_project_path
                .as_ref()
                .and_then(|path| path.file_stem())
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string()),
            bpm: self.state.bpm,
            sample_rate: self.state.sample_rate,
            tracks,
            clips,
            note_clips,
        }
    }

    fn rebuild_from_loaded_project(&mut self, loaded: ProjectLoadResult) {
        self.clear_project_runtime();
        self.state.bpm = loaded.project.bpm;
        self.state.bpm_text = format!("{:.0}", loaded.project.bpm);
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
            track.instrument_kind = track_info.instrument;
            track.has_instrument = track_info.instrument.is_some();

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
                    InstrumentStateInfo::DrumRack { .. } => {}
                }
            }

            for effect_info in &track_info.effects {
                let fx = vibez_dsp::factory::create_effect_with_params(
                    effect_info.effect_type,
                    self.state.sample_rate as f32,
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

            self.state.next_track_number = self.state.next_track_number.max(
                self.state.tracks.len() as u32 + 1,
            );
            self.state.tracks.push(track);
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
            }
            self.send_command(EngineCommand::LoadSamplerSample {
                track_id: sampler.track_id,
                sample: sampler.audio,
                sample_name: sampler.name,
            });
        }

        self.state.selected_track = self.state.tracks.first().map(|track| track.id);
        self.state.current_project_path = Some(loaded.path.clone());
        self.state.project_dirty = false;
        self.state.status_text = if loaded.warnings.is_empty() {
            format!("Opened {}", loaded.path.display())
        } else {
            format!(
                "Opened {} with {} warning(s)",
                loaded.path.display(),
                loaded.warnings.len()
            )
        };
    }

    /// Auto-scroll the arrangement when a clip's right edge nears the visible boundary.
    /// Called from resize/move handlers so the view follows the drag.
    fn auto_scroll_to_beat(&mut self, clip_end_beat: f64) {
        let ppb = 20.0 * self.state.zoom_level as f64;
        // Conservative estimate of canvas width (window minus track headers)
        let canvas_width = 1400.0_f64;
        let visible_beats = canvas_width / ppb;
        let visible_end = self.state.scroll_offset_beats + visible_beats;
        let margin = 2.0_f64;

        if clip_end_beat > visible_end - margin {
            let delta = clip_end_beat - visible_end + margin * 2.0;
            let total = self.state.total_beats();
            self.state.scroll_offset_beats =
                (self.state.scroll_offset_beats + delta).clamp(0.0, total);
        }
        // Also scroll left when dragging toward the left edge
        if clip_end_beat < self.state.scroll_offset_beats + margin
            && self.state.scroll_offset_beats > 0.0
        {
            let delta = self.state.scroll_offset_beats + margin - clip_end_beat;
            self.state.scroll_offset_beats = (self.state.scroll_offset_beats - delta).max(0.0);
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        // Auto-dismiss context menu on any action except tick/engine/menu events
        if self.state.context_menu.is_some() {
            let keep_menu = matches!(
                message,
                Message::Tick
                    | Message::EnginePosition(_)
                    | Message::EngineMetering { .. }
                    | Message::EngineStopped
                    | Message::EngineTrackMeter { .. }
                    | Message::ShowContextMenu { .. }
                    | Message::DismissContextMenu
                    | Message::DeleteClipsInRegion { .. }
                    | Message::SetSelectionAsLoop
                    | Message::DeleteSelectedClip
                    | Message::DuplicateSelectedClip
                    | Message::SplitSelectedAtPlayhead
                    | Message::JoinSelectedClips
                    | Message::SplitAudioClip { .. }
                    | Message::SplitNoteClip { .. }
                    | Message::SplitClipsAtRegion { .. }
                    | Message::CreateNoteClipFromSelection(_)
                    | Message::EditNameText(_)
                    | Message::CursorMoved(_, _)
                    | Message::MouseReleased
                    | Message::NewProject
                    | Message::OpenProject
                    | Message::SaveProject
                    | Message::SaveProjectAs
                    | Message::ToggleFileMenu
                    | Message::DismissFileMenu
                    | Message::ProjectOpenPathSelected(_)
                    | Message::ProjectSavePathSelected(_)
                    | Message::ProjectLoaded(_)
                    | Message::ProjectSaved(_)
                    | Message::OpenSettings
                    | Message::CloseSettings
                    | Message::SelectSettingsTab(_)
                    | Message::SetBufferSize(_)
                    | Message::ScanPlugins
                    | Message::ScanPluginsComplete(_)
                    | Message::PluginLoadError(_)
            );
            if !keep_menu {
                self.state.context_menu = None;
            }
        }

        let should_mark_dirty = matches!(
            &message,
            Message::BpmSubmit
                | Message::AddTrack
                | Message::RemoveTrack(_)
                | Message::ClipAudioDecoded(..)
                | Message::RemoveClip(..)
                | Message::SetTrackGain(..)
                | Message::SetTrackPan(..)
                | Message::SetTrackMute(_)
                | Message::SetTrackSolo(_)
                | Message::AddEffect(..)
                | Message::RemoveEffect(..)
                | Message::SetEffectParam(..)
                | Message::ToggleEffectBypass(..)
                | Message::MoveEffectUp(..)
                | Message::MoveEffectDown(..)
                | Message::AddInstrumentTrack
                | Message::SetInstrumentParam(..)
                | Message::SamplerSampleDecoded(..)
                | Message::ToggleClipLoop(..)
                | Message::SetClipLoopRegion { .. }
                | Message::ToggleNoteClipLoop(..)
                | Message::SetNoteClipLoopRegion { .. }
                | Message::AddNoteClipToTrack(_)
                | Message::AddNote { .. }
                | Message::RemoveNote(..)
                | Message::EditNote(..)
                | Message::RemoveSelectedNotes(..)
                | Message::NudgeSelectedNotes { .. }
                | Message::MoveNotesAbsolute { .. }
                | Message::DuplicateNoteClip(..)
                | Message::DoubleNoteClip(..)
                | Message::CropNoteClip(..)
                | Message::MoveAudioClip { .. }
                | Message::MoveNoteClipPosition { .. }
                | Message::ResizeAudioClip { .. }
                | Message::ResizeNoteClipDuration { .. }
                | Message::MoveClipToTrack { .. }
                | Message::SplitAudioClip { .. }
                | Message::SplitNoteClip { .. }
                | Message::DeleteSelectedClip
                | Message::DuplicateSelectedClip
                | Message::SplitSelectedAtPlayhead
                | Message::JoinSelectedClips
                | Message::ToggleArrangementLoop
                | Message::SetArrangementLoopRegion { .. }
                | Message::DeleteClipsInRegion { .. }
                | Message::SplitClipsAtRegion { .. }
                | Message::CreateClipFromSelection
                | Message::CreateNoteClipFromSelection(_)
                | Message::MoveTrackUp(_)
                | Message::MoveTrackDown(_)
                | Message::MoveSelectedTrackUp
                | Message::MoveSelectedTrackDown
                | Message::RenameTrack(..)
                | Message::RenameClip(..)
                | Message::AddMidiTrack
                | Message::SetTrackInstrument(..)
                | Message::RemoveTrackInstrument(_)
                | Message::HalveNoteClip(..)
        );
        if should_mark_dirty {
            self.mark_project_dirty();
        }

        match message {
            Message::Play => {
                self.state.playing = true;
                self.send_command(EngineCommand::Play);
            }
            Message::Stop => {
                self.state.playing = false;
                self.state.position_samples = 0;
                self.send_command(EngineCommand::Stop);
                self.send_command(EngineCommand::Seek(0));
            }
            Message::TogglePlayback => {
                if self.state.playing {
                    return self.update(Message::Stop);
                } else {
                    return self.update(Message::Play);
                }
            }
            Message::Seek(normalized) => {
                let total = self.state.total_duration_samples();
                if total > 0 {
                    let sample_pos = (normalized * total as f64) as u64;
                    self.state.position_samples = sample_pos;
                    self.send_command(EngineCommand::Seek(sample_pos));
                }
                // Simple click clears the time selection
                self.state.time_selection_active = false;
            }
            Message::BpmChanged(val) => {
                self.state.bpm_text = val;
            }
            Message::BpmSubmit => {
                if let Ok(bpm) = self.state.bpm_text.parse::<f64>() {
                    let bpm = bpm.clamp(20.0, 999.0);
                    self.state.bpm = bpm;
                    self.state.bpm_text = format!("{bpm:.0}");
                    self.send_command(EngineCommand::SetBpm(bpm));
                    // Re-send loop region since beat→sample mapping changed
                    if self.state.loop_enabled {
                        let start = self.state.beats_to_samples(self.state.loop_start_beats);
                        let end = self.state.beats_to_samples(self.state.loop_end_beats);
                        self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
                    }
                } else {
                    let bpm = self.state.bpm;
                    self.state.bpm_text = format!("{bpm:.0}");
                }
            }

            // -- Workspace --
            Message::SwitchWorkspace(ws) => {
                self.state.workspace = ws;
            }

            Message::SwitchDetailTab(tab) => {
                self.state.detail_panel_tab = tab;
            }

            // -- Zoom / scroll --
            Message::ZoomIn => {
                self.state.zoom_level = (self.state.zoom_level * 1.25).min(16.0);
            }
            Message::ZoomOut => {
                self.state.zoom_level = (self.state.zoom_level / 1.25).max(0.01);
            }
            Message::SetZoom(level) => {
                self.state.zoom_level = level.clamp(0.01, 16.0);
            }
            Message::ZoomToFit => {
                let content_beats = self.state.total_beats();
                if content_beats > 0.0 {
                    // Conservative estimate of canvas width (window minus track headers)
                    let canvas_width = 1400.0_f32;
                    let target_ppb = canvas_width / content_beats as f32;
                    self.state.zoom_level = (target_ppb / 20.0).clamp(0.01, 16.0);
                    self.state.scroll_offset_beats = 0.0;
                }
            }
            Message::ScrollArrangement(delta) => {
                let total = self.state.total_beats();
                self.state.scroll_offset_beats =
                    (self.state.scroll_offset_beats + delta).clamp(0.0, total);
            }

            // -- Snap grid --
            Message::SetSnapGrid(grid) => {
                self.state.snap_grid = grid;
            }

            // -- Engine events --
            Message::Tick => {
                self.poll_engine_events();
                self.poll_plugin_loads();
                self.poll_plugin_windows();
                // Pump CLAP plugin timers and FDs (needed for JUCE event loop)
                vibez_plugin_host::poll_clap_events();

                // Tick-driven auto-scroll: when dragging a clip and cursor is near the
                // window edge, continuously scroll the arrangement. The canvas can't
                // generate new events when the cursor is stationary at the screen edge,
                // so this tick loop drives the scrolling at 60fps.
                if self.state.drag_resize_active {
                    let edge_zone = 60.0_f32;
                    // Right edge: estimate window right ~= track header + canvas
                    // Use cursor_x relative to a conservative right boundary
                    let right_boundary = 1600.0_f32; // reasonable default
                    if self.state.cursor_x > right_boundary - edge_zone {
                        let overshoot = ((self.state.cursor_x
                            - (right_boundary - edge_zone))
                            / edge_zone)
                            .clamp(0.0, 3.0) as f64;
                        let delta = overshoot * 2.0;
                        let total = self.state.total_beats();
                        self.state.scroll_offset_beats =
                            (self.state.scroll_offset_beats + delta).clamp(0.0, total);
                    }
                    // Left edge
                    let left_boundary = 230.0_f32; // ~track header width
                    if self.state.cursor_x < left_boundary + edge_zone
                        && self.state.scroll_offset_beats > 0.0
                    {
                        let overshoot = ((left_boundary + edge_zone - self.state.cursor_x)
                            / edge_zone)
                            .clamp(0.0, 3.0) as f64;
                        let delta = overshoot * 2.0;
                        self.state.scroll_offset_beats =
                            (self.state.scroll_offset_beats - delta).max(0.0);
                    }
                }
            }
            Message::EnginePosition(pos) => {
                self.state.position_samples = pos;
            }
            Message::EngineMetering { peak_l, peak_r } => {
                self.state.peak_l = peak_l;
                self.state.peak_r = peak_r;
            }
            Message::EngineStopped => {
                self.state.playing = false;
            }

            // -- Multi-track messages --
            Message::AddTrack => {
                let track_num = self.state.next_track_number;
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");

                self.send_command(EngineCommand::AddTrack(id, name.clone()));
                self.state.tracks.push(UiTrack::new(id, name, color_index));
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::RemoveTrack(track_id) => {
                // Close all plugin GUI windows for this track
                if let Some(ref mut mgr) = self.plugin_window_manager {
                    mgr.close_track_effects(track_id);
                }
                self.plugin_gui_raw_ptrs
                    .retain(|k, _| match k {
                        PluginGuiKey::Effect { track_id: tid, .. } => *tid != track_id,
                        PluginGuiKey::Instrument { track_id: tid } => *tid != track_id,
                    });

                self.send_command(EngineCommand::RemoveTrack(track_id));
                self.state.tracks.retain(|t| t.id != track_id);
                if self.state.selected_track == Some(track_id) {
                    self.state.selected_track = self.state.tracks.first().map(|t| t.id);
                }
                // Clear note clip selection if track removed
                if let Some((tid, _)) = self.state.selected_note_clip {
                    if tid == track_id {
                        self.state.selected_note_clip = None;
                    }
                }
                // Clear arrangement selections for removed track
                self.state.selected_clips.retain(|sel| {
                    let sel_track = match sel {
                        ArrangementSelection::AudioClip { track_id: t, .. } => *t,
                        ArrangementSelection::NoteClip { track_id: t, .. } => *t,
                    };
                    sel_track != track_id
                });
                if self.state.tracks.is_empty() {
                    self.state.status_text = "Ready — Add a track to get started".to_string();
                } else {
                    self.state.status_text = format!("{} tracks", self.state.tracks.len());
                }
            }
            Message::SelectTrack(track_id) => {
                self.state.selected_track = Some(track_id);
            }
            Message::AddClipToTrack(track_id) => {
                // Guard: only audio tracks can have audio clips
                if let Some(track) = self.state.find_track(track_id) {
                    if track.kind.is_midi() {
                        self.state.status_text =
                            "MIDI tracks use note clips, not audio".to_string();
                        return Task::none();
                    }
                }
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Add Audio Clip")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    move |path| Message::ClipFileSelected(track_id, path),
                );
            }
            Message::ClipFileSelected(track_id, path) => {
                if let Some(path) = path {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.state.status_text = format!("Loading {file_name}...");
                    let clip_id = ClipId::new();
                    let source = MediaSourceRef::LocalFile { path: path.clone() };

                    return Task::perform(decode_file_async(path), move |result| match result {
                        Ok(audio) => Message::ClipAudioDecoded(
                            track_id,
                            clip_id,
                            Arc::new(audio),
                            file_name.clone(),
                            source.clone(),
                        ),
                        Err(e) => Message::ClipDecodeError(track_id, e),
                    });
                }
            }
            Message::ClipAudioDecoded(track_id, clip_id, audio, name, source) => {
                let existing_end = self
                    .state
                    .find_track(track_id)
                    .map(|t| {
                        t.clips
                            .iter()
                            .map(|c| c.position.saturating_add(c.duration))
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);

                let duration = audio.num_frames() as u64;

                self.send_command(EngineCommand::AddClip {
                    track_id,
                    clip_id,
                    audio: Arc::clone(&audio),
                    position: existing_end,
                    source_offset: 0,
                    duration,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                });

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.clips.push(UiClip {
                        id: clip_id,
                        name: name.clone(),
                        audio,
                        source: Some(source),
                        position: existing_end,
                        source_offset: 0,
                        duration,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                }

                self.state.status_text = format!("Added clip: {name}");
            }
            Message::ClipDecodeError(_, err) => {
                self.state.status_text = format!("Error: {err}");
            }
            Message::RemoveClip(track_id, clip_id) => {
                self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.clips.retain(|c| c.id != clip_id);
                }
                // Clear from multi-selection if this clip was selected
                self.state
                    .selected_clips
                    .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
            }
            Message::SetTrackGain(track_id, gain) => {
                let gain = gain.clamp(0.0, 2.0);
                self.send_command(EngineCommand::SetTrackGain(track_id, gain));
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.gain = gain;
                }
            }
            Message::SetTrackPan(track_id, pan) => {
                let pan = pan.clamp(0.0, 1.0);
                self.send_command(EngineCommand::SetTrackPan(track_id, pan));
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.pan = pan;
                }
            }
            Message::SetTrackMute(track_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.mute = !track.mute;
                    let mute = track.mute;
                    self.send_command(EngineCommand::SetTrackMute(track_id, mute));
                }
            }
            Message::SetTrackSolo(track_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.solo = !track.solo;
                    let solo = track.solo;
                    self.send_command(EngineCommand::SetTrackSolo(track_id, solo));
                }
            }
            Message::EngineTrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.peak_l = peak_l.max(track.peak_l * 0.85);
                    track.peak_r = peak_r.max(track.peak_r * 0.85);
                }
            }

            // -- Effects --
            Message::AddEffect(track_id, effect_type) => {
                let effect_id = vibez_core::id::EffectId::new();
                let fx =
                    vibez_dsp::factory::create_effect(effect_type, self.state.sample_rate as f32);
                let descriptors = fx.param_descriptors();
                let params: Vec<f32> = descriptors.iter().map(|d| d.default).collect();

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.effects.push(UiEffect {
                        id: effect_id,
                        effect_type,
                        bypass: false,
                        params,
                        descriptors,
                        plugin_name: None,
                        has_plugin_gui: false,
                    });
                }
                self.send_command(EngineCommand::AddEffect {
                    track_id,
                    effect_id,
                    effect_type,
                    position: None,
                });
                self.state.device_context_menu = None;
                self.state.status_text = format!("Added {} effect", effect_type.name());
            }
            Message::RemoveEffect(track_id, effect_id) => {
                // Close plugin GUI window if open
                let gui_key = PluginGuiKey::Effect {
                    track_id,
                    effect_id,
                };
                if let Some(ref mut mgr) = self.plugin_window_manager {
                    mgr.close(gui_key);
                }
                self.plugin_gui_raw_ptrs.remove(&gui_key);

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.effects.retain(|e| e.id != effect_id);
                }
                self.send_command(EngineCommand::RemoveEffect(track_id, effect_id));
                self.state.status_text = "Removed effect".to_string();
            }
            Message::SetEffectParam(track_id, effect_id, param_index, value) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        if param_index < effect.params.len() {
                            let desc = &effect.descriptors[param_index];
                            let clamped = value.clamp(desc.min, desc.max);
                            effect.params[param_index] = clamped;
                            self.send_command(EngineCommand::SetEffectParam {
                                track_id,
                                effect_id,
                                param_index,
                                value: clamped,
                            });
                        }
                    }
                }
            }
            Message::ToggleEffectBypass(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        effect.bypass = !effect.bypass;
                        let bypass = effect.bypass;
                        self.send_command(EngineCommand::SetEffectBypass {
                            track_id,
                            effect_id,
                            bypass,
                        });
                    }
                }
            }
            Message::MoveEffectUp(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx > 0 {
                            track.effects.swap(idx, idx - 1);
                            self.send_command(EngineCommand::MoveEffect {
                                track_id,
                                effect_id,
                                new_index: idx - 1,
                            });
                        }
                    }
                }
            }
            Message::MoveEffectDown(track_id, effect_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx + 1 < track.effects.len() {
                            track.effects.swap(idx, idx + 1);
                            self.send_command(EngineCommand::MoveEffect {
                                track_id,
                                effect_id,
                                new_index: idx + 1,
                            });
                        }
                    }
                }
            }

            // -- Instrument tracks --
            Message::AddInstrumentTrack => {
                let track_num = self.state.next_track_number;
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("MIDI {track_num}");
                let kind = TrackKind::Midi;

                self.send_command(EngineCommand::AddMidiTrack(id, name.clone()));
                let mut track = UiTrack::new_instrument(id, name, kind, color_index);
                track.has_instrument = false;
                self.state.tracks.push(track);
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::SetInstrumentParam(track_id, param_index, value) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if param_index < track.instrument_params.len() {
                        track.instrument_params[param_index] = value;
                    }
                }
                self.send_command(EngineCommand::SetInstrumentParam {
                    track_id,
                    param_index,
                    value,
                });
                self.state.status_text = format!("Param {param_index} = {value:.2}");
            }

            // -- Sampler --
            Message::LoadSamplerSample(track_id) => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Load Sample")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    move |path| Message::SamplerFileSelected(track_id, path),
                );
            }
            Message::SamplerFileSelected(track_id, path) => {
                if let Some(path) = path {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.state.status_text = format!("Loading {file_name}...");
                    let source = MediaSourceRef::LocalFile { path: path.clone() };

                    return Task::perform(
                        decode_file_async(path),
                        move |result| match result {
                            Ok(audio) => Message::SamplerSampleDecoded(
                                track_id,
                                Arc::new(audio),
                                file_name.clone(),
                                source.clone(),
                            ),
                            Err(e) => Message::SamplerDecodeError(track_id, e),
                        },
                    );
                }
            }
            Message::SamplerSampleDecoded(track_id, audio, name, source) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.sample_name = Some(name.clone());
                    track.sample_source = Some(source);
                }
                self.send_command(EngineCommand::LoadSamplerSample {
                    track_id,
                    sample: audio,
                    sample_name: name.clone(),
                });
                self.state.status_text = format!("Loaded sample: {name}");
            }
            Message::SamplerDecodeError(track_id, err) => {
                let _ = track_id;
                self.state.status_text = format!("Sample load error: {err}");
            }

            // -- Clip looping --
            Message::ToggleClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        if clip.loop_enabled && clip.loop_end == 0 {
                            clip.loop_start = clip.source_offset;
                            clip.loop_end = clip.source_offset + clip.duration;
                        }
                        cmd_data = Some((clip.loop_enabled, clip.loop_start, clip.loop_end));
                    }
                }
                if let Some((enabled, loop_start, loop_end)) = cmd_data {
                    self.send_command(EngineCommand::SetClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start,
                        loop_end,
                    });
                }
            }
            Message::SetClipLoopRegion {
                track_id,
                clip_id,
                loop_start,
                loop_end,
            } => {
                let mut enabled = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_start = loop_start;
                        clip.loop_end = loop_end;
                        enabled = clip.loop_enabled;
                    }
                }
                self.send_command(EngineCommand::SetClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start,
                    loop_end,
                });
            }
            Message::ToggleNoteClipLoop(track_id, clip_id) => {
                let mut cmd_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_enabled = !clip.loop_enabled;
                        if clip.loop_enabled && clip.loop_end_beats == 0.0 {
                            clip.loop_start_beats = 0.0;
                            clip.loop_end_beats = clip.duration_beats;
                        }
                        cmd_data = Some((
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        ));
                    }
                }
                if let Some((enabled, loop_start_beats, loop_end_beats)) = cmd_data {
                    self.send_command(EngineCommand::SetNoteClipLoop {
                        track_id,
                        clip_id,
                        enabled,
                        loop_start_beats,
                        loop_end_beats,
                    });
                }
            }
            Message::SetNoteClipLoopRegion {
                track_id,
                clip_id,
                loop_start_beats,
                loop_end_beats,
            } => {
                let mut enabled = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.loop_start_beats = loop_start_beats;
                        clip.loop_end_beats = loop_end_beats;
                        enabled = clip.loop_enabled;
                    }
                }
                self.send_command(EngineCommand::SetNoteClipLoop {
                    track_id,
                    clip_id,
                    enabled,
                    loop_start_beats,
                    loop_end_beats,
                });
            }

            // -- Piano roll / note clips --
            Message::AddNoteClipToTrack(track_id) => {
                let clip_id = ClipId::new();
                let position_beats = 0.0;
                let duration_beats = 16.0;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.note_clips.push(UiNoteClip {
                        id: clip_id,
                        name: format!("Pattern {}", track.note_clips.len() + 1),
                        position_beats,
                        duration_beats,
                        notes: Vec::new(),
                        selected_notes: HashSet::new(),
                        loop_enabled: true,
                        loop_start_beats: 0.0,
                        loop_end_beats: duration_beats,
                    });
                }
                self.send_command(EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    loop_enabled: true,
                    loop_start_beats: 0.0,
                    loop_end_beats: duration_beats,
                });
                // Auto-select the new note clip for piano roll editing
                self.state.selected_note_clip = Some((track_id, clip_id));
                self.state.status_text = "Added note clip".to_string();
            }
            Message::SelectNoteClip(track_id, clip_id) => {
                self.state.selected_note_clip = Some((track_id, clip_id));
            }
            Message::AddNote {
                track_id,
                clip_id,
                pitch,
                start_beat,
                duration_beats,
            } => {
                let note = MidiNote {
                    pitch,
                    velocity: 100,
                    start_beat,
                    duration_beats,
                };
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.notes.push(note);
                    }
                }
                self.send_command(EngineCommand::AddNote {
                    track_id,
                    clip_id,
                    note,
                });
            }
            Message::RemoveNote(track_id, clip_id, note_index) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes.remove(note_index);
                            // Re-index: remove deleted index, shift down any higher indices
                            clip.selected_notes.remove(&note_index);
                            clip.selected_notes = clip.selected_notes.iter()
                                .map(|&i| if i > note_index { i - 1 } else { i })
                                .collect();
                        }
                    }
                }
                self.send_command(EngineCommand::RemoveNote {
                    track_id,
                    clip_id,
                    note_index,
                });
            }
            Message::EditNote(track_id, clip_id, note_index, new_note) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if note_index < clip.notes.len() {
                            clip.notes[note_index] = new_note;
                        }
                    }
                }
                self.send_command(EngineCommand::EditNote {
                    track_id,
                    clip_id,
                    note_index,
                    note: new_note,
                });
            }
            Message::SelectNote(track_id, clip_id, note_index, shift_held) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        match note_index {
                            Some(idx) => {
                                if shift_held {
                                    // Toggle note in/out of selection
                                    if !clip.selected_notes.remove(&idx) {
                                        clip.selected_notes.insert(idx);
                                    }
                                } else {
                                    // Clear all, select only this note
                                    clip.selected_notes.clear();
                                    clip.selected_notes.insert(idx);
                                }
                            }
                            None => {
                                clip.selected_notes.clear();
                            }
                        }
                    }
                }
            }
            Message::SelectAllNotes(track_id, clip_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.selected_notes = (0..clip.notes.len()).collect();
                    }
                }
            }
            Message::RemoveSelectedNotes(track_id, clip_id) => {
                // Collect indices to remove in reverse order
                let indices_to_remove: Vec<usize> = if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let mut indices: Vec<usize> = clip.selected_notes.iter().copied()
                            .filter(|&i| i < clip.notes.len())
                            .collect();
                        indices.sort_unstable_by(|a, b| b.cmp(a));
                        indices
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };

                // Remove from engine in reverse order (indices stay valid)
                for &idx in &indices_to_remove {
                    self.send_command(EngineCommand::RemoveNote {
                        track_id,
                        clip_id,
                        note_index: idx,
                    });
                }

                // Remove from UI state
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        for &idx in &indices_to_remove {
                            if idx < clip.notes.len() {
                                clip.notes.remove(idx);
                            }
                        }
                        clip.selected_notes.clear();
                    }
                }
            }
            Message::NudgeSelectedNotes { track_id, clip_id, delta_beats, delta_semitones } => {
                let mut updates: Vec<(usize, MidiNote)> = Vec::new();
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let indices: Vec<usize> = clip.selected_notes.iter().copied()
                            .filter(|&i| i < clip.notes.len())
                            .collect();
                        for &idx in &indices {
                            let note = &mut clip.notes[idx];
                            note.start_beat = (note.start_beat + delta_beats).max(0.0);
                            note.pitch = (note.pitch as i16 + delta_semitones as i16)
                                .clamp(0, 127) as u8;
                            updates.push((idx, *note));
                        }
                    }
                }
                for (idx, note) in updates {
                    self.send_command(EngineCommand::EditNote {
                        track_id,
                        clip_id,
                        note_index: idx,
                        note,
                    });
                }
            }

            Message::MoveNotesAbsolute { track_id, clip_id, moves } => {
                let mut updates: Vec<(usize, MidiNote)> = Vec::new();
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        for &(idx, new_beat, new_pitch) in &moves {
                            if idx < clip.notes.len() {
                                clip.notes[idx].start_beat = new_beat;
                                clip.notes[idx].pitch = new_pitch;
                                updates.push((idx, clip.notes[idx]));
                            }
                        }
                    }
                }
                for (idx, note) in updates {
                    self.send_command(EngineCommand::EditNote {
                        track_id,
                        clip_id,
                        note_index: idx,
                        note,
                    });
                }
            }

            // -- Clip operations --
            Message::DuplicateNoteClip(track_id, clip_id) => {
                let new_clip_id = ClipId::new();
                let mut new_clip_data = None;

                if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let new_pos = clip.position_beats + clip.duration_beats;
                        new_clip_data = Some((
                            UiNoteClip {
                                id: new_clip_id,
                                name: format!("{} (copy)", clip.name),
                                position_beats: new_pos,
                                duration_beats: clip.duration_beats,
                                notes: clip.notes.clone(),
                                selected_notes: HashSet::new(),
                                loop_enabled: clip.loop_enabled,
                                loop_start_beats: clip.loop_start_beats,
                                loop_end_beats: clip.loop_end_beats,
                            },
                            new_pos,
                            clip.duration_beats,
                            clip.notes.clone(),
                        ));
                    }
                }

                if let Some((new_clip, pos, dur, notes)) = new_clip_data {
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.push(new_clip);
                    }
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: new_clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id: new_clip_id,
                            note: *note,
                        });
                    }
                    self.state.selected_note_clip = Some((track_id, new_clip_id));
                    self.state.status_text = "Duplicated clip".to_string();
                }
            }
            Message::DoubleNoteClip(track_id, clip_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let orig_dur = clip.duration_beats;
                        let cloned_notes: Vec<MidiNote> = clip
                            .notes
                            .iter()
                            .map(|n| MidiNote {
                                start_beat: n.start_beat + orig_dur,
                                ..*n
                            })
                            .collect();
                        clip.notes.extend_from_slice(&cloned_notes);
                        clip.duration_beats *= 2.0;

                        // Send new notes to engine
                        for note in &cloned_notes {
                            self.send_command(EngineCommand::AddNote {
                                track_id,
                                clip_id,
                                note: *note,
                            });
                        }
                    }
                }
                self.state.status_text = "Doubled clip length".to_string();
            }
            Message::CropNoteClip(track_id, clip_id) => {
                let mut sync_data = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        if !clip.notes.is_empty() {
                            let min_beat = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat)
                                .fold(f64::INFINITY, f64::min);
                            let max_beat = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat + n.duration_beats)
                                .fold(f64::NEG_INFINITY, f64::max);

                            // Shift notes so first note starts at 0
                            for note in &mut clip.notes {
                                note.start_beat -= min_beat;
                            }
                            clip.position_beats += min_beat;
                            clip.duration_beats = max_beat - min_beat;

                            sync_data = Some((
                                clip.position_beats,
                                clip.duration_beats,
                                clip.notes.clone(),
                            ));
                        }
                    }
                }
                // Sync to engine outside the mutable borrow
                if let Some((pos, dur, notes)) = sync_data {
                    self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id,
                            note: *note,
                        });
                    }
                }
                self.state.status_text = "Cropped clip to content".to_string();
            }

            // -- Piano roll scroll --
            Message::PianoRollScrollY(y) => {
                self.state.piano_roll_scroll_y = y;
            }

            // ── Arrangement clip interaction ──
            Message::SelectArrangementClip {
                selection,
                shift_held,
            } => {
                if shift_held {
                    // Toggle in/out of selection set
                    if !self.state.selected_clips.remove(&selection) {
                        self.state.selected_clips.insert(selection);
                    }
                } else {
                    // Replace selection
                    self.state.selected_clips.clear();
                    self.state.selected_clips.insert(selection);
                }
                self.state.detail_panel_tab = DetailPanelTab::Clip;
                // Also update track selection and note clip selection for detail panel
                match selection {
                    ArrangementSelection::AudioClip { track_id, .. } => {
                        self.state.selected_track = Some(track_id);
                        // Clear note clip selection when an audio clip is selected
                        self.state.selected_note_clip = None;
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        self.state.selected_track = Some(track_id);
                        self.state.selected_note_clip = Some((track_id, clip_id));
                    }
                }
            }

            Message::MoveAudioClip {
                track_id,
                clip_id,
                new_position,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.position = new_position;
                    }
                }
                self.send_command(EngineCommand::MoveClip {
                    track_id,
                    clip_id,
                    new_position,
                });
                self.state.drag_resize_active = true;
            }

            Message::MoveNoteClipPosition {
                track_id,
                clip_id,
                new_position_beats,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.position_beats = new_position_beats;
                    }
                }
                self.send_command(EngineCommand::MoveNoteClip {
                    track_id,
                    clip_id,
                    new_position_beats,
                });
                self.state.drag_resize_active = true;
            }

            Message::ResizeAudioClip {
                track_id,
                clip_id,
                new_duration,
            } => {
                // Update UI state — auto-enable loop when extending past source length
                let spb = 60.0 * self.state.sample_rate as f64 / self.state.bpm;
                let mut sync_data = None;
                let mut clip_end_beat = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        let source_len = clip.audio.num_frames() as u64 - clip.source_offset;
                        if new_duration > source_len {
                            // Extending past source: enable loop over full source region
                            clip.duration = new_duration;
                            if !clip.loop_enabled {
                                clip.loop_enabled = true;
                                clip.loop_start = clip.source_offset;
                                clip.loop_end = clip.source_offset + source_len;
                            }
                        } else {
                            clip.duration = new_duration;
                        }
                        clip_end_beat =
                            Some((clip.position + clip.duration) as f64 / spb);
                        sync_data = Some((
                            Arc::clone(&clip.audio),
                            clip.position,
                            clip.source_offset,
                            clip.duration,
                            clip.loop_enabled,
                            clip.loop_start,
                            clip.loop_end,
                        ));
                    }
                }
                // Sync to engine via Remove+Add (loop state included atomically)
                if let Some((
                    audio,
                    position,
                    source_offset,
                    duration,
                    loop_enabled,
                    loop_start,
                    loop_end,
                )) = sync_data
                {
                    self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                    self.send_command(EngineCommand::AddClip {
                        track_id,
                        clip_id,
                        audio,
                        position,
                        source_offset,
                        duration,
                        loop_enabled,
                        loop_start,
                        loop_end,
                    });
                }
                if let Some(end_beat) = clip_end_beat {
                    self.auto_scroll_to_beat(end_beat);
                }
                self.state.drag_resize_active = true;
            }

            Message::ResizeNoteClipDuration {
                track_id,
                clip_id,
                new_duration_beats,
            } => {
                let mut sync_data = None;
                let mut clip_end_beat = None;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.duration_beats = new_duration_beats;

                        // Auto-enable loop when extending past note content
                        // Only if the clip actually has notes — empty clips don't loop
                        if !clip.notes.is_empty() && !clip.loop_enabled {
                            let content_end = clip
                                .notes
                                .iter()
                                .map(|n| n.start_beat + n.duration_beats)
                                .fold(0.0_f64, f64::max);
                            if content_end > 0.0 && new_duration_beats > content_end {
                                clip.loop_enabled = true;
                                clip.loop_start_beats = 0.0;
                                clip.loop_end_beats = content_end;
                            }
                        }

                        clip_end_beat =
                            Some(clip.position_beats + clip.duration_beats);
                        sync_data = Some((
                            clip.position_beats,
                            clip.duration_beats,
                            clip.notes.clone(),
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        ));
                    }
                }
                // Sync to engine via Remove+Add+re-add-notes (loop state included atomically)
                if let Some((pos, dur, notes, loop_enabled, loop_start_beats, loop_end_beats)) =
                    sync_data
                {
                    self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id,
                        position_beats: pos,
                        duration_beats: dur,
                        loop_enabled,
                        loop_start_beats,
                        loop_end_beats,
                    });
                    for note in &notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id,
                            note: *note,
                        });
                    }
                }
                if let Some(end_beat) = clip_end_beat {
                    self.auto_scroll_to_beat(end_beat);
                }
                self.state.drag_resize_active = true;
            }

            Message::MoveClipToTrack {
                source_track,
                target_track,
                clip_id,
                is_note_clip,
            } => {
                if is_note_clip {
                    // Move note clip between instrument tracks
                    let mut clip_data = None;
                    if let Some(track) = self.state.find_track_mut(source_track) {
                        if let Some(idx) = track.note_clips.iter().position(|c| c.id == clip_id) {
                            clip_data = Some(track.note_clips.remove(idx));
                        }
                    }
                    if let Some(clip) = clip_data {
                        // Remove from engine source track
                        self.send_command(EngineCommand::RemoveNoteClip(source_track, clip_id));
                        // Add to engine target track
                        self.send_command(EngineCommand::AddNoteClip {
                            track_id: target_track,
                            clip_id,
                            position_beats: clip.position_beats,
                            duration_beats: clip.duration_beats,
                            loop_enabled: clip.loop_enabled,
                            loop_start_beats: clip.loop_start_beats,
                            loop_end_beats: clip.loop_end_beats,
                        });
                        for note in &clip.notes {
                            self.send_command(EngineCommand::AddNote {
                                track_id: target_track,
                                clip_id,
                                note: *note,
                            });
                        }
                        // Add to UI target track
                        if let Some(track) = self.state.find_track_mut(target_track) {
                            track.note_clips.push(clip);
                        }
                        // Update selection
                        self.state
                            .selected_clips
                            .remove(&ArrangementSelection::NoteClip {
                                track_id: source_track,
                                clip_id,
                            });
                        self.state
                            .selected_clips
                            .insert(ArrangementSelection::NoteClip {
                                track_id: target_track,
                                clip_id,
                            });
                        self.state.selected_track = Some(target_track);
                        self.state.selected_note_clip = Some((target_track, clip_id));
                    }
                } else {
                    // Move audio clip between audio tracks
                    let mut clip_data = None;
                    if let Some(track) = self.state.find_track_mut(source_track) {
                        if let Some(idx) = track.clips.iter().position(|c| c.id == clip_id) {
                            clip_data = Some(track.clips.remove(idx));
                        }
                    }
                    if let Some(clip) = clip_data {
                        // Remove from engine source track
                        self.send_command(EngineCommand::RemoveClip(source_track, clip_id));
                        // Add to engine target track
                        self.send_command(EngineCommand::AddClip {
                            track_id: target_track,
                            clip_id,
                            audio: Arc::clone(&clip.audio),
                            position: clip.position,
                            source_offset: clip.source_offset,
                            duration: clip.duration,
                            loop_enabled: clip.loop_enabled,
                            loop_start: clip.loop_start,
                            loop_end: clip.loop_end,
                        });
                        // Add to UI target track
                        if let Some(track) = self.state.find_track_mut(target_track) {
                            track.clips.push(clip);
                        }
                        // Update selection
                        self.state
                            .selected_clips
                            .remove(&ArrangementSelection::AudioClip {
                                track_id: source_track,
                                clip_id,
                            });
                        self.state
                            .selected_clips
                            .insert(ArrangementSelection::AudioClip {
                                track_id: target_track,
                                clip_id,
                            });
                        self.state.selected_track = Some(target_track);
                    }
                }
            }

            Message::SplitAudioClip {
                track_id,
                clip_id,
                split_position,
            } => {
                let mut split_data = None;
                if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                        if split_position > clip.position
                            && split_position < clip.position + clip.duration
                        {
                            let left_dur = split_position - clip.position;
                            let right_dur = clip.duration - left_dur;
                            let right_source_offset = clip.source_offset + left_dur;
                            split_data = Some((
                                Arc::clone(&clip.audio),
                                clip.name.clone(),
                                clip.source.clone(),
                                clip.position,
                                clip.source_offset,
                                left_dur,
                                split_position,
                                right_source_offset,
                                right_dur,
                            ));
                        }
                    }
                }
                if let Some((
                    audio,
                    name,
                    source,
                    orig_pos,
                    orig_offset,
                    left_dur,
                    right_pos,
                    right_offset,
                    right_dur,
                )) = split_data
                {
                    let left_id = ClipId::new();
                    let right_id = ClipId::new();

                    // Remove original
                    self.send_command(EngineCommand::RemoveClip(track_id, clip_id));
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.clips.retain(|c| c.id != clip_id);
                    }

                    // Add left half
                    self.send_command(EngineCommand::AddClip {
                        track_id,
                        clip_id: left_id,
                        audio: Arc::clone(&audio),
                        position: orig_pos,
                        source_offset: orig_offset,
                        duration: left_dur,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.clips.push(UiClip {
                            id: left_id,
                            name: format!("{name} L"),
                            audio: Arc::clone(&audio),
                            source: source.clone(),
                            position: orig_pos,
                            source_offset: orig_offset,
                            duration: left_dur,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        });
                    }

                    // Add right half
                    self.send_command(EngineCommand::AddClip {
                        track_id,
                        clip_id: right_id,
                        audio: Arc::clone(&audio),
                        position: right_pos,
                        source_offset: right_offset,
                        duration: right_dur,
                        loop_enabled: false,
                        loop_start: 0,
                        loop_end: 0,
                    });
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.clips.push(UiClip {
                            id: right_id,
                            name: format!("{name} R"),
                            audio,
                            source,
                            position: right_pos,
                            source_offset: right_offset,
                            duration: right_dur,
                            loop_enabled: false,
                            loop_start: 0,
                            loop_end: 0,
                        });
                    }

                    // Update selection: remove original, add left half
                    self.state
                        .selected_clips
                        .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
                    self.state
                        .selected_clips
                        .insert(ArrangementSelection::AudioClip {
                            track_id,
                            clip_id: left_id,
                        });
                    self.state.status_text = "Split audio clip".to_string();
                }
            }

            Message::SplitNoteClip {
                track_id,
                clip_id,
                split_beat,
            } => {
                let mut split_data = None;
                if let Some(track) = self.state.find_track(track_id) {
                    if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                        let clip_end = clip.position_beats + clip.duration_beats;
                        if split_beat > clip.position_beats && split_beat < clip_end {
                            let local_split = split_beat - clip.position_beats;
                            let left_dur = local_split;
                            let right_dur = clip.duration_beats - local_split;

                            let mut left_notes = Vec::new();
                            let mut right_notes = Vec::new();
                            for note in &clip.notes {
                                if note.start_beat < local_split {
                                    left_notes.push(*note);
                                } else {
                                    right_notes.push(MidiNote {
                                        start_beat: note.start_beat - local_split,
                                        ..*note
                                    });
                                }
                            }

                            split_data = Some((
                                clip.name.clone(),
                                clip.position_beats,
                                left_dur,
                                split_beat,
                                right_dur,
                                left_notes,
                                right_notes,
                            ));
                        }
                    }
                }
                if let Some((
                    name,
                    orig_pos,
                    left_dur,
                    right_pos,
                    right_dur,
                    left_notes,
                    right_notes,
                )) = split_data
                {
                    let left_id = ClipId::new();
                    let right_id = ClipId::new();

                    // Remove original
                    self.send_command(EngineCommand::RemoveNoteClip(track_id, clip_id));
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.retain(|c| c.id != clip_id);
                    }

                    // Add left half
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: left_id,
                        position_beats: orig_pos,
                        duration_beats: left_dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &left_notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id: left_id,
                            note: *note,
                        });
                    }
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.push(UiNoteClip {
                            id: left_id,
                            name: format!("{name} L"),
                            position_beats: orig_pos,
                            duration_beats: left_dur,
                            notes: left_notes,
                            selected_notes: HashSet::new(),
                            loop_enabled: false,
                            loop_start_beats: 0.0,
                            loop_end_beats: 0.0,
                        });
                    }

                    // Add right half
                    self.send_command(EngineCommand::AddNoteClip {
                        track_id,
                        clip_id: right_id,
                        position_beats: right_pos,
                        duration_beats: right_dur,
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                    for note in &right_notes {
                        self.send_command(EngineCommand::AddNote {
                            track_id,
                            clip_id: right_id,
                            note: *note,
                        });
                    }
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        track.note_clips.push(UiNoteClip {
                            id: right_id,
                            name: format!("{name} R"),
                            position_beats: right_pos,
                            duration_beats: right_dur,
                            notes: right_notes,
                            selected_notes: HashSet::new(),
                            loop_enabled: false,
                            loop_start_beats: 0.0,
                            loop_end_beats: 0.0,
                        });
                    }

                    // Update selection: remove original, add left half
                    self.state
                        .selected_clips
                        .remove(&ArrangementSelection::NoteClip { track_id, clip_id });
                    self.state
                        .selected_clips
                        .insert(ArrangementSelection::NoteClip {
                            track_id,
                            clip_id: left_id,
                        });
                    self.state.selected_note_clip = Some((track_id, left_id));
                    self.state.status_text = "Split note clip".to_string();
                }
            }

            Message::DeleteSelectedClip => {
                let selections: Vec<_> = self.state.selected_clips.drain().collect();
                if !selections.is_empty() {
                    for selection in &selections {
                        match selection {
                            ArrangementSelection::AudioClip { track_id, clip_id } => {
                                self.send_command(EngineCommand::RemoveClip(*track_id, *clip_id));
                                if let Some(track) = self.state.find_track_mut(*track_id) {
                                    track.clips.retain(|c| c.id != *clip_id);
                                }
                            }
                            ArrangementSelection::NoteClip { track_id, clip_id } => {
                                self.send_command(EngineCommand::RemoveNoteClip(
                                    *track_id, *clip_id,
                                ));
                                if let Some(track) = self.state.find_track_mut(*track_id) {
                                    track.note_clips.retain(|c| c.id != *clip_id);
                                }
                                if self
                                    .state
                                    .selected_note_clip
                                    .is_some_and(|(tid, cid)| tid == *track_id && cid == *clip_id)
                                {
                                    self.state.selected_note_clip = None;
                                }
                            }
                        }
                    }
                    let count = selections.len();
                    self.state.status_text = if count == 1 {
                        "Deleted clip".to_string()
                    } else {
                        format!("Deleted {count} clips")
                    };
                }
            }

            Message::DuplicateSelectedClip => {
                let selections: Vec<_> = self.state.selected_clips.iter().copied().collect();
                if !selections.is_empty() {
                    let mut new_selections = HashSet::new();
                    for selection in &selections {
                        match selection {
                            ArrangementSelection::AudioClip { track_id, clip_id } => {
                                let mut dup_data = None;
                                if let Some(track) = self.state.find_track(*track_id) {
                                    if let Some(clip) =
                                        track.clips.iter().find(|c| c.id == *clip_id)
                                    {
                                        let new_pos = clip.position + clip.duration;
                                        dup_data = Some((
                                            Arc::clone(&clip.audio),
                                            clip.name.clone(),
                                            clip.source.clone(),
                                            new_pos,
                                            clip.source_offset,
                                            clip.duration,
                                        ));
                                    }
                                }
                                if let Some((audio, name, source, position, source_offset, duration)) =
                                    dup_data
                                {
                                    let new_id = ClipId::new();
                                    self.send_command(EngineCommand::AddClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                        audio: Arc::clone(&audio),
                                        position,
                                        source_offset,
                                        duration,
                                        loop_enabled: false,
                                        loop_start: 0,
                                        loop_end: 0,
                                    });
                                    if let Some(track) = self.state.find_track_mut(*track_id) {
                                        track.clips.push(UiClip {
                                            id: new_id,
                                            name: format!("{name} (copy)"),
                                            audio,
                                            source,
                                            position,
                                            source_offset,
                                            duration,
                                            loop_enabled: false,
                                            loop_start: 0,
                                            loop_end: 0,
                                        });
                                    }
                                    new_selections.insert(ArrangementSelection::AudioClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                    });
                                }
                            }
                            ArrangementSelection::NoteClip { track_id, clip_id } => {
                                // Duplicate note clip inline
                                let mut dup_data = None;
                                if let Some(track) = self.state.find_track(*track_id) {
                                    if let Some(clip) =
                                        track.note_clips.iter().find(|c| c.id == *clip_id)
                                    {
                                        dup_data = Some((
                                            clip.name.clone(),
                                            clip.position_beats + clip.duration_beats,
                                            clip.duration_beats,
                                            clip.notes.clone(),
                                        ));
                                    }
                                }
                                if let Some((name, new_pos, dur, notes)) = dup_data {
                                    let new_id = ClipId::new();
                                    self.send_command(EngineCommand::AddNoteClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                        position_beats: new_pos,
                                        duration_beats: dur,
                                        loop_enabled: false,
                                        loop_start_beats: 0.0,
                                        loop_end_beats: 0.0,
                                    });
                                    for note in &notes {
                                        self.send_command(EngineCommand::AddNote {
                                            track_id: *track_id,
                                            clip_id: new_id,
                                            note: *note,
                                        });
                                    }
                                    if let Some(track) = self.state.find_track_mut(*track_id) {
                                        track.note_clips.push(UiNoteClip {
                                            id: new_id,
                                            name: format!("{name} (copy)"),
                                            position_beats: new_pos,
                                            duration_beats: dur,
                                            notes,
                                            selected_notes: HashSet::new(),
                                            loop_enabled: false,
                                            loop_start_beats: 0.0,
                                            loop_end_beats: 0.0,
                                        });
                                    }
                                    new_selections.insert(ArrangementSelection::NoteClip {
                                        track_id: *track_id,
                                        clip_id: new_id,
                                    });
                                }
                            }
                        }
                    }
                    // Select the new copies
                    self.state.selected_clips = new_selections;
                    let count = selections.len();
                    self.state.status_text = if count == 1 {
                        "Duplicated clip".to_string()
                    } else {
                        format!("Duplicated {count} clips")
                    };
                }
            }

            // -- Split (Ctrl+E) --
            // If time selection is active → split all clips at region boundaries.
            // Otherwise → split selected clips at the playhead.
            Message::SplitSelectedAtPlayhead => {
                if self.state.time_selection_active
                    && self.state.selection_end_beats > self.state.selection_start_beats
                {
                    return self.update(Message::SplitClipsAtRegion {
                        start_beats: self.state.selection_start_beats,
                        end_beats: self.state.selection_end_beats,
                    });
                }

                let clips: Vec<_> = self.state.selected_clips.iter().copied().collect();
                for selection in clips {
                    match selection {
                        ArrangementSelection::AudioClip { track_id, clip_id } => {
                            let _ = self.update(Message::SplitAudioClip {
                                track_id,
                                clip_id,
                                split_position: self.state.position_samples,
                            });
                        }
                        ArrangementSelection::NoteClip { track_id, clip_id } => {
                            let _ = self.update(Message::SplitNoteClip {
                                track_id,
                                clip_id,
                                split_beat: self.state.position_beats(),
                            });
                        }
                    }
                }
            }

            // -- Join selected clips (Ctrl+J) --
            Message::JoinSelectedClips => {
                let clips: Vec<_> = self.state.selected_clips.iter().copied().collect();
                if clips.len() < 2 {
                    return Task::none();
                }

                // Validate: all must be same type and same track
                let first_track = match clips[0] {
                    ArrangementSelection::AudioClip { track_id, .. } => track_id,
                    ArrangementSelection::NoteClip { track_id, .. } => track_id,
                };
                let all_audio = clips.iter().all(|s| {
                    matches!(s, ArrangementSelection::AudioClip { track_id, .. } if *track_id == first_track)
                });
                let all_note = clips.iter().all(|s| {
                    matches!(s, ArrangementSelection::NoteClip { track_id, .. } if *track_id == first_track)
                });

                if all_audio {
                    self.join_audio_clips(first_track, &clips);
                } else if all_note {
                    self.join_note_clips(first_track, &clips);
                } else {
                    self.state.status_text = "Join requires same type and track".to_string();
                }
            }

            // -- Arrangement loop --
            Message::ToggleArrangementLoop => {
                self.state.loop_enabled = !self.state.loop_enabled;
                self.send_command(EngineCommand::SetArrangementLoop(self.state.loop_enabled));
                if self.state.loop_enabled {
                    // Copy selection to loop region when enabling loop with active selection
                    if self.state.time_selection_active
                        && self.state.selection_end_beats > self.state.selection_start_beats
                    {
                        self.state.loop_start_beats = self.state.selection_start_beats;
                        self.state.loop_end_beats = self.state.selection_end_beats;
                    }
                    let start = self.state.beats_to_samples(self.state.loop_start_beats);
                    let end = self.state.beats_to_samples(self.state.loop_end_beats);
                    self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
                }
            }
            Message::SetArrangementLoopRegion {
                start_beats,
                end_beats,
            } => {
                self.state.loop_start_beats = start_beats;
                self.state.loop_end_beats = end_beats;
                let start = self.state.beats_to_samples(start_beats);
                let end = self.state.beats_to_samples(end_beats);
                self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
            }

            // -- Time selection + context menu --
            Message::SetTimeSelection {
                start_beats,
                end_beats,
            } => {
                self.state.selection_start_beats = start_beats;
                self.state.selection_end_beats = end_beats;
                self.state.time_selection_active = true;
            }
            Message::SetSelectionAsLoop => {
                self.state.context_menu = None;
                self.state.loop_start_beats = self.state.selection_start_beats;
                self.state.loop_end_beats = self.state.selection_end_beats;
                let start = self.state.beats_to_samples(self.state.loop_start_beats);
                let end = self.state.beats_to_samples(self.state.loop_end_beats);
                self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
                if !self.state.loop_enabled {
                    self.state.loop_enabled = true;
                    self.send_command(EngineCommand::SetArrangementLoop(true));
                }
            }
            Message::SetTimeSelectionActive(active) => {
                self.state.time_selection_active = active;
            }
            Message::CursorMoved(x, y) => {
                self.state.cursor_x = x;
                self.state.cursor_y = y;
            }
            Message::MouseReleased => {
                self.state.drag_resize_active = false;
            }
            Message::ShowContextMenu { x, y, target } => {
                // For ArrangementEmpty from mouse_area (no cursor coords),
                // use the globally tracked cursor position instead.
                let (menu_x, menu_y) = if matches!(target, ContextMenuTarget::ArrangementEmpty) {
                    (self.state.cursor_x, self.state.cursor_y)
                } else {
                    (x, y)
                };
                // Also select the clip if targeting one (add to set if not already there)
                if let ContextMenuTarget::Clip {
                    track_id,
                    clip_id,
                    is_note_clip,
                } = &target
                {
                    let selection = if *is_note_clip {
                        ArrangementSelection::NoteClip {
                            track_id: *track_id,
                            clip_id: *clip_id,
                        }
                    } else {
                        ArrangementSelection::AudioClip {
                            track_id: *track_id,
                            clip_id: *clip_id,
                        }
                    };
                    if !self.state.selected_clips.contains(&selection) {
                        self.state.selected_clips.clear();
                        self.state.selected_clips.insert(selection);
                    }
                    self.state.selected_track = Some(*track_id);
                }
                self.state.context_menu = Some(crate::state::ContextMenu {
                    x: menu_x,
                    y: menu_y,
                    target,
                });
            }
            Message::DismissContextMenu => {
                self.state.context_menu = None;
            }
            Message::DeleteClipsInRegion {
                start_beats,
                end_beats,
            } => {
                self.state.context_menu = None;
                let spb = if self.state.bpm > 0.0 {
                    self.state.sample_rate as f64 * 60.0 / self.state.bpm
                } else {
                    0.0
                };
                // Collect clip IDs to remove
                let mut audio_removals: Vec<(TrackId, ClipId)> = Vec::new();
                let mut note_removals: Vec<(TrackId, ClipId)> = Vec::new();
                for track in &self.state.tracks {
                    if spb > 0.0 {
                        for clip in &track.clips {
                            let clip_start = clip.position as f64 / spb;
                            let clip_end = (clip.position + clip.duration) as f64 / spb;
                            if clip_start < end_beats && clip_end > start_beats {
                                audio_removals.push((track.id, clip.id));
                            }
                        }
                    }
                    for nc in &track.note_clips {
                        let clip_end = nc.position_beats + nc.duration_beats;
                        if nc.position_beats < end_beats && clip_end > start_beats {
                            note_removals.push((track.id, nc.id));
                        }
                    }
                }
                for (tid, cid) in &audio_removals {
                    self.send_command(EngineCommand::RemoveClip(*tid, *cid));
                    if let Some(track) = self.state.find_track_mut(*tid) {
                        track.clips.retain(|c| c.id != *cid);
                    }
                }
                for (tid, cid) in &note_removals {
                    self.send_command(EngineCommand::RemoveNoteClip(*tid, *cid));
                    if let Some(track) = self.state.find_track_mut(*tid) {
                        track.note_clips.retain(|c| c.id != *cid);
                    }
                }
                self.state.selected_clips.clear();
                self.state.selected_note_clip = None;
                self.state.time_selection_active = false;
                let count = audio_removals.len() + note_removals.len();
                self.state.status_text = format!("Deleted {count} clips in region");
            }
            Message::SplitClipsAtRegion {
                start_beats,
                end_beats,
            } => {
                self.state.context_menu = None;
                let spb = if self.state.bpm > 0.0 {
                    self.state.sample_rate as f64 * 60.0 / self.state.bpm
                } else {
                    0.0
                };
                let mut split_count = 0u32;

                // Split at start boundary first, then end boundary.
                // After a split, new clips replace the original, so we
                // re-scan the track list between boundary passes.
                for boundary_beats in [start_beats, end_beats] {
                    let boundary_sample = (boundary_beats * spb) as u64;

                    // Collect audio splits for this boundary
                    let audio_hits: Vec<(TrackId, ClipId)> = if spb > 0.0 {
                        self.state
                            .tracks
                            .iter()
                            .flat_map(|t| {
                                t.clips.iter().filter_map(|c| {
                                    let cs = c.position as f64 / spb;
                                    let ce = (c.position + c.duration) as f64 / spb;
                                    if boundary_beats > cs && boundary_beats < ce {
                                        Some((t.id, c.id))
                                    } else {
                                        None
                                    }
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    };

                    let note_hits: Vec<(TrackId, ClipId)> = self
                        .state
                        .tracks
                        .iter()
                        .flat_map(|t| {
                            t.note_clips.iter().filter_map(|c| {
                                let ce = c.position_beats + c.duration_beats;
                                if boundary_beats > c.position_beats && boundary_beats < ce {
                                    Some((t.id, c.id))
                                } else {
                                    None
                                }
                            })
                        })
                        .collect();

                    for (tid, cid) in audio_hits {
                        let _ = self.update(Message::SplitAudioClip {
                            track_id: tid,
                            clip_id: cid,
                            split_position: boundary_sample,
                        });
                        split_count += 1;
                    }
                    for (tid, cid) in note_hits {
                        let _ = self.update(Message::SplitNoteClip {
                            track_id: tid,
                            clip_id: cid,
                            split_beat: boundary_beats,
                        });
                        split_count += 1;
                    }
                }

                if split_count > 0 {
                    self.state.status_text =
                        format!("Split {split_count} clips at region boundaries");
                }
            }

            // -- Clip creation from region --
            Message::CreateClipFromSelection => {
                if let Some(tid) = self.state.selected_track {
                    if let Some(track) = self.state.find_track(tid) {
                        if track.kind.is_midi() {
                            return self.update(Message::CreateNoteClipFromSelection(tid));
                        } else {
                            self.state.status_text =
                                "Select a time region on a MIDI track".to_string();
                        }
                    }
                } else {
                    self.state.status_text = "No track selected".to_string();
                }
            }
            Message::CreateNoteClipFromSelection(track_id) => {
                self.state.context_menu = None;
                if !self.state.time_selection_active
                    || self.state.selection_end_beats <= self.state.selection_start_beats
                {
                    self.state.status_text = "No time selection active".to_string();
                    return Task::none();
                }
                if let Some(track) = self.state.find_track(track_id) {
                    if !track.kind.is_midi() {
                        self.state.status_text =
                            "Can only create note clips on MIDI tracks".to_string();
                        return Task::none();
                    }
                }
                let clip_id = ClipId::new();
                let position_beats = self.state.selection_start_beats;
                let duration_beats =
                    self.state.selection_end_beats - self.state.selection_start_beats;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.note_clips.push(UiNoteClip {
                        id: clip_id,
                        name: format!("Pattern {}", track.note_clips.len() + 1),
                        position_beats,
                        duration_beats,
                        notes: Vec::new(),
                        selected_notes: HashSet::new(),
                        loop_enabled: false,
                        loop_start_beats: 0.0,
                        loop_end_beats: 0.0,
                    });
                }
                self.send_command(EngineCommand::AddNoteClip {
                    track_id,
                    clip_id,
                    position_beats,
                    duration_beats,
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                });
                self.state.selected_note_clip = Some((track_id, clip_id));
                self.state.selected_clips.clear();
                self.state.selected_clips.insert(ArrangementSelection::NoteClip {
                    track_id,
                    clip_id,
                });
                self.state.status_text = "Created note clip from selection".to_string();
            }

            // -- Track reordering --
            Message::MoveTrackUp(track_id) => {
                if let Some(idx) = self.state.tracks.iter().position(|t| t.id == track_id) {
                    if idx > 0 {
                        self.state.tracks.swap(idx, idx - 1);
                        let order: Vec<TrackId> =
                            self.state.tracks.iter().map(|t| t.id).collect();
                        self.send_command(EngineCommand::ReorderTracks(order));
                    }
                }
            }
            Message::MoveTrackDown(track_id) => {
                if let Some(idx) = self.state.tracks.iter().position(|t| t.id == track_id) {
                    if idx + 1 < self.state.tracks.len() {
                        self.state.tracks.swap(idx, idx + 1);
                        let order: Vec<TrackId> =
                            self.state.tracks.iter().map(|t| t.id).collect();
                        self.send_command(EngineCommand::ReorderTracks(order));
                    }
                }
            }
            Message::MoveSelectedTrackUp => {
                if let Some(tid) = self.state.selected_track {
                    return self.update(Message::MoveTrackUp(tid));
                }
            }
            Message::MoveSelectedTrackDown => {
                if let Some(tid) = self.state.selected_track {
                    return self.update(Message::MoveTrackDown(tid));
                }
            }

            // -- Renaming --
            Message::StartEditingTrackName(track_id) => {
                if let Some(track) = self.state.find_track(track_id) {
                    self.state.edit_name_text = track.name.clone();
                    self.state.editing_track_name = Some(track_id);
                    self.state.editing_clip_name = None;
                }
            }
            Message::StartEditingClipName(track_id, clip_id) => {
                self.state.context_menu = None;
                let name = self.state.find_track(track_id).and_then(|t| {
                    t.clips
                        .iter()
                        .find(|c| c.id == clip_id)
                        .map(|c| c.name.clone())
                        .or_else(|| {
                            t.note_clips
                                .iter()
                                .find(|c| c.id == clip_id)
                                .map(|c| c.name.clone())
                        })
                });
                if let Some(name) = name {
                    self.state.edit_name_text = name;
                    self.state.editing_clip_name = Some((track_id, clip_id));
                    self.state.editing_track_name = None;
                }
            }
            Message::EditNameText(t) => {
                self.state.edit_name_text = t;
            }
            Message::FinishEditing => {
                let new_name = self.state.edit_name_text.clone();
                if let Some(track_id) = self.state.editing_track_name.take() {
                    if !new_name.is_empty() {
                        return self.update(Message::RenameTrack(track_id, new_name));
                    }
                }
                if let Some((track_id, clip_id)) = self.state.editing_clip_name.take() {
                    if !new_name.is_empty() {
                        return self.update(Message::RenameClip(track_id, clip_id, new_name));
                    }
                }
            }
            Message::CancelEditing => {
                self.state.editing_track_name = None;
                self.state.editing_clip_name = None;
                self.state.edit_name_text.clear();
                self.state.device_context_menu = None;
            }
            Message::RenameTrack(track_id, new_name) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.name = new_name;
                }
            }
            Message::RenameClip(track_id, clip_id, new_name) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(c) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        c.name = new_name.clone();
                    }
                    if let Some(c) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        c.name = new_name;
                    }
                }
            }

            // -- MIDI track (no auto-synth) --
            Message::AddMidiTrack => {
                let track_num = self.state.next_track_number;
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number += 1;
                let id = TrackId::new();
                let name = format!("MIDI {track_num}");
                let kind = TrackKind::Midi;

                self.send_command(EngineCommand::AddMidiTrack(id, name.clone()));
                let mut track = UiTrack::new_instrument(id, name, kind, color_index);
                track.has_instrument = false;
                self.state.tracks.push(track);
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }

            // -- Instrument attach/detach --
            Message::SetTrackInstrument(track_id, instrument_kind) => {
                let sample_rate = self.state.sample_rate as f32;
                let instrument_params = default_instrument_params(instrument_kind, sample_rate);
                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.has_instrument = true;
                    track.instrument_kind = Some(instrument_kind);
                    track.sample_name = None;
                    track.sample_source = None;
                    track.instrument_params = instrument_params.clone();
                }
                self.send_command(EngineCommand::SetTrackInstrument(track_id, instrument_kind));
                for (param_index, value) in instrument_params.into_iter().enumerate() {
                    self.send_command(EngineCommand::SetInstrumentParam {
                        track_id,
                        param_index,
                        value,
                    });
                }
                self.state.device_context_menu = None;
                self.state.status_text = format!("Added {}", instrument_kind.name());
            }
            Message::RemoveTrackInstrument(track_id) => {
                // Close plugin GUI window if open
                let gui_key = PluginGuiKey::Instrument { track_id };
                if let Some(ref mut mgr) = self.plugin_window_manager {
                    mgr.close(gui_key);
                }
                self.plugin_gui_raw_ptrs.remove(&gui_key);

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.has_instrument = false;
                    track.instrument_kind = None;
                    track.sample_name = None;
                    track.sample_source = None;
                    track.instrument_params.clear();
                    track.plugin_instrument_name = None;
                    track.has_plugin_instrument_gui = false;
                }
                self.send_command(EngineCommand::RemoveTrackInstrument(track_id));
                self.state.status_text = "Removed instrument".to_string();
            }

            // -- Pattern halve --
            Message::HalveNoteClip(track_id, clip_id) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let new_dur = (clip.duration_beats / 2.0).max(0.25);
                        clip.duration_beats = new_dur;
                        self.send_command(EngineCommand::SetNoteClipDuration {
                            track_id,
                            clip_id,
                            duration_beats: new_dur,
                        });
                    }
                }
                self.state.status_text = "Halved clip duration".to_string();
            }

            // -- Edit mode --
            Message::TogglePianoRollEditMode => {
                use crate::state::PianoRollEditMode;
                self.state.piano_roll_edit_mode = match self.state.piano_roll_edit_mode {
                    PianoRollEditMode::Select => PianoRollEditMode::Draw,
                    PianoRollEditMode::Draw => PianoRollEditMode::Select,
                };
                let mode_name = match self.state.piano_roll_edit_mode {
                    PianoRollEditMode::Select => "Select",
                    PianoRollEditMode::Draw => "Draw",
                };
                self.state.status_text = format!("Piano roll: {mode_name} mode");
            }

            // -- Device context menu --
            Message::ShowDeviceContextMenu { x, y, track_id } => {
                use crate::state::{DeviceContextMenu, DeviceMenuCategory};
                let is_midi = self
                    .state
                    .find_track(track_id)
                    .is_some_and(|t| t.kind.is_midi());
                self.state.device_context_menu = Some(DeviceContextMenu {
                    x,
                    y,
                    track_id,
                    category: Some(if is_midi {
                        DeviceMenuCategory::Instruments
                    } else {
                        DeviceMenuCategory::Effects
                    }),
                    search: String::new(),
                });
            }
            Message::DismissDeviceContextMenu => {
                self.state.device_context_menu = None;
            }
            Message::SetDeviceMenuCategory(category) => {
                if let Some(ref mut menu) = self.state.device_context_menu {
                    menu.category = Some(category);
                }
            }
            Message::DeviceMenuSearch(query) => {
                if let Some(ref mut menu) = self.state.device_context_menu {
                    menu.search = query;
                }
            }

            // -- File menu --
            Message::NewProject => {
                self.reset_to_new_project();
            }
            Message::OpenProject => {
                self.state.file_menu_open = false;
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Open Vibez Project")
                            .add_filter("Vibez Project", &["vibez", "json"])
                            .pick_file()
                            .await;
                        handle.map(|file| file.path().to_path_buf())
                    },
                    Message::ProjectOpenPathSelected,
                );
            }
            Message::SaveProject => {
                self.state.file_menu_open = false;
                let project = self.project_from_state();
                if let Some(path) = self.state.current_project_path.clone() {
                    return Task::perform(save_project_async(path, project), Message::ProjectSaved);
                }
                return self.update(Message::SaveProjectAs);
            }
            Message::SaveProjectAs => {
                self.state.file_menu_open = false;
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Save Vibez Project")
                            .set_file_name("Untitled.vibez")
                            .add_filter("Vibez Project", &["vibez"])
                            .save_file()
                            .await;
                        handle.map(|file| file.path().to_path_buf())
                    },
                    Message::ProjectSavePathSelected,
                );
            }
            Message::ToggleFileMenu => {
                self.state.file_menu_open = !self.state.file_menu_open;
            }
            Message::DismissFileMenu => {
                self.state.file_menu_open = false;
            }
            Message::ProjectOpenPathSelected(path) => {
                if let Some(path) = path {
                    self.state.status_text = format!("Opening {}...", path.display());
                    return Task::perform(load_project_async(path), Message::ProjectLoaded);
                }
            }
            Message::ProjectSavePathSelected(path) => {
                if let Some(mut path) = path {
                    if path.extension().is_none() {
                        path.set_extension("vibez");
                    }
                    let project = self.project_from_state();
                    return Task::perform(save_project_async(path, project), Message::ProjectSaved);
                }
            }
            Message::ProjectLoaded(result) => match result {
                Ok(loaded) => {
                    self.rebuild_from_loaded_project(loaded);
                }
                Err(err) => {
                    self.state.status_text = format!("Project load error: {err}");
                }
            },
            Message::ProjectSaved(result) => match result {
                Ok(path) => {
                    self.state.current_project_path = Some(path.clone());
                    self.state.project_dirty = false;
                    self.state.status_text = format!("Saved {}", path.display());
                }
                Err(err) => {
                    self.state.status_text = format!("Project save error: {err}");
                }
            },

            // -- Settings --
            Message::OpenSettings => {
                self.state.settings_open = true;
                self.state.file_menu_open = false;
            }
            Message::CloseSettings => {
                self.state.settings_open = false;
                let _ = self.state.plugin_settings.save();
            }
            Message::SelectSettingsTab(tab) => {
                self.state.settings_tab = tab;
            }
            Message::SetBufferSize(size) => {
                self.state.settings_buffer_size = size;

                if let Some(stream) = self._stream.as_mut() {
                    match stream.reconfigure(Some(size)) {
                        Ok(()) => {
                            let sr = stream.sample_rate();
                            if let Err(e) = stream.play() {
                                eprintln!("vibez: failed to restart audio stream: {e}");
                            }
                            self.state.sample_rate = sr;
                            self.state.status_text =
                                format!("Audio restarted — buffer {size}, {sr} Hz");
                        }
                        Err(e) => {
                            eprintln!("vibez: failed to reconfigure audio stream: {e}");
                            self.state.status_text = format!("Audio error: {e}");
                        }
                    }
                } else {
                    self.state.status_text =
                        "No audio device — cannot change buffer size".to_string();
                }
            }

            // -- Plugin scanning --
            Message::ScanPlugins => {
                if !self.state.plugin_scan_in_progress {
                    self.state.plugin_scan_in_progress = true;
                    self.state.plugin_scan_status = "Scanning...".to_string();
                    let settings = self.state.plugin_settings.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                vibez_plugin_host::scan_plugins(&settings)
                            })
                            .await
                            .unwrap_or_default()
                        },
                        Message::ScanPluginsComplete,
                    );
                }
            }
            Message::ScanPluginsComplete(plugins) => {
                let count = plugins.len();
                self.state.plugin_settings.cache = plugins;
                self.state.plugin_scan_in_progress = false;
                self.state.plugin_scan_status = format!("Found {count} plugins");
                let _ = self.state.plugin_settings.save();
            }
            Message::AddPluginScanPath => {
                return Task::perform(
                    async {
                        let result = rfd::AsyncFileDialog::new()
                            .set_title("Select Plugin Scan Directory")
                            .pick_folder()
                            .await;
                        result.map(|h| h.path().to_path_buf())
                    },
                    Message::PluginScanPathSelected,
                );
            }
            Message::PluginScanPathSelected(path) => {
                if let Some(path) = path {
                    if !self.state.plugin_settings.extra_scan_paths.contains(&path) {
                        self.state.plugin_settings.extra_scan_paths.push(path);
                        let _ = self.state.plugin_settings.save();
                    }
                }
            }
            Message::RemovePluginScanPath(index) => {
                if index < self.state.plugin_settings.extra_scan_paths.len() {
                    self.state.plugin_settings.extra_scan_paths.remove(index);
                    let _ = self.state.plugin_settings.save();
                }
            }
            Message::ToggleScanDefaultPaths => {
                self.state.plugin_settings.scan_default_paths =
                    !self.state.plugin_settings.scan_default_paths;
                let _ = self.state.plugin_settings.save();
            }

            // -- Plugin loading --
            Message::AddPluginToTrack(track_id, plugin_id) => {
                self.state.device_context_menu = None;
                if let Some(info) = self
                    .state
                    .plugin_settings
                    .cache
                    .iter()
                    .find(|p| p.id == plugin_id)
                    .cloned()
                {
                    let sample_rate = self.state.sample_rate as f64;
                    let is_instrument = info.category.is_instrument();
                    let loading_name = info.name.clone();

                    if is_instrument {
                        let tx = self.plugin_instrument_tx.clone();
                        std::thread::spawn(move || {
                            match load_plugin_instrument_bg(&info, sample_rate) {
                                Ok(mut result) => {
                                    result.track_id = track_id;
                                    let _ = tx.send(result);
                                }
                                Err(e) => {
                                    eprintln!("Plugin load error: {e}");
                                }
                            }
                        });
                    } else {
                        let tx = self.plugin_effect_tx.clone();
                        std::thread::spawn(move || {
                            match load_plugin_effect_bg(&info, sample_rate) {
                                Ok(mut result) => {
                                    result.track_id = track_id;
                                    let _ = tx.send(result);
                                }
                                Err(e) => {
                                    eprintln!("Plugin load error: {e}");
                                }
                            }
                        });
                    }
                    self.state.status_text = format!("Loading {loading_name}...");
                }
            }
            Message::PluginLoadError(err) => {
                self.state.status_text = format!("Plugin error: {err}");
            }

            // -- Plugin GUI windows --
            Message::OpenPluginGui(key) => {
                // If the window is already open, raise it
                if let Some(ref mgr) = self.plugin_window_manager {
                    if mgr.is_open(key) {
                        mgr.raise(key);
                        return Task::none();
                    }
                }
                if let Some(&raw_ptr) = self.plugin_gui_raw_ptrs.get(&key) {
                    let title = match key {
                        PluginGuiKey::Effect {
                            track_id,
                            effect_id,
                        } => self
                            .state
                            .find_track(track_id)
                            .and_then(|t| {
                                t.effects
                                    .iter()
                                    .find(|e| e.id == effect_id)
                                    .and_then(|e| e.plugin_name.clone())
                            })
                            .unwrap_or_else(|| "Plugin".to_string()),
                        PluginGuiKey::Instrument { track_id } => self
                            .state
                            .find_track(track_id)
                            .and_then(|t| t.plugin_instrument_name.clone())
                            .unwrap_or_else(|| "Plugin".to_string()),
                    };
                    if let Some(ref mut mgr) = self.plugin_window_manager {
                        if mgr.open(key, raw_ptr, title) {
                            self.state.status_text = "Plugin GUI opened".to_string();
                        } else {
                            self.state.status_text =
                                "Failed to open plugin GUI".to_string();
                        }
                    } else {
                        self.state.status_text =
                            "No X11 display — plugin GUI unavailable".to_string();
                    }
                } else {
                    self.state.status_text =
                        "Plugin GUI handle not available".to_string();
                }
            }
            Message::ClosePluginGui(key) => {
                if let Some(ref mut mgr) = self.plugin_window_manager {
                    mgr.close(key);
                }
            }
        }
        Task::none()
    }

    fn join_audio_clips(&mut self, track_id: TrackId, selections: &[ArrangementSelection]) {
        // Collect clip data sorted by position
        let clip_ids: Vec<ClipId> = selections
            .iter()
            .filter_map(|s| match s {
                ArrangementSelection::AudioClip { clip_id, .. } => Some(*clip_id),
                _ => None,
            })
            .collect();

        let mut clip_data: Vec<(u64, u64, u64, Arc<vibez_core::audio_buffer::DecodedAudio>)> =
            Vec::new();
        if let Some(track) = self.state.find_track(track_id) {
            for cid in &clip_ids {
                if let Some(clip) = track.clips.iter().find(|c| c.id == *cid) {
                    clip_data.push((
                        clip.position,
                        clip.source_offset,
                        clip.duration,
                        Arc::clone(&clip.audio),
                    ));
                }
            }
        }

        if clip_data.len() < 2 {
            return;
        }

        // Sort by position
        clip_data.sort_by_key(|(pos, _, _, _)| *pos);

        let start_pos = clip_data[0].0;
        let end_pos = clip_data
            .iter()
            .map(|(pos, _, dur, _)| pos + dur)
            .max()
            .unwrap_or(start_pos);
        let total_duration = end_pos - start_pos;

        // Determine channel count from first clip
        let channels = clip_data[0].3.num_channels();
        let sr = clip_data[0].3.sample_rate;

        // Create joined buffer filled with silence
        let mut joined_channels: Vec<Vec<f32>> = (0..channels)
            .map(|_| vec![0.0f32; total_duration as usize])
            .collect();

        // Copy each clip's audio into the correct offset
        for (pos, source_offset, duration, audio) in &clip_data {
            let offset_in_joined = (*pos - start_pos) as usize;
            let src_off = *source_offset as usize;
            let dur = *duration as usize;
            let ch_count = channels.min(audio.num_channels());
            for (ch, dst) in joined_channels.iter_mut().enumerate().take(ch_count) {
                let src = &audio.channels[ch];
                let src_end = (src_off + dur).min(src.len());
                let copy_len = src_end.saturating_sub(src_off);
                let dst_end = (offset_in_joined + copy_len).min(dst.len());
                let actual_len = dst_end.saturating_sub(offset_in_joined);
                if actual_len > 0 {
                    dst[offset_in_joined..offset_in_joined + actual_len]
                        .copy_from_slice(&src[src_off..src_off + actual_len]);
                }
            }
        }

        // Create DecodedAudio
        let joined_audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: joined_channels,
            sample_rate: sr,
        });

        // Remove all originals
        for cid in &clip_ids {
            self.send_command(EngineCommand::RemoveClip(track_id, *cid));
            if let Some(track) = self.state.find_track_mut(track_id) {
                track.clips.retain(|c| c.id != *cid);
            }
        }

        // Add joined clip
        let new_id = ClipId::new();
        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id: new_id,
            audio: Arc::clone(&joined_audio),
            position: start_pos,
            source_offset: 0,
            duration: total_duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: new_id,
                name: "Joined".to_string(),
                audio: joined_audio,
                source: None,
                position: start_pos,
                source_offset: 0,
                duration: total_duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
            });
        }

        self.state.selected_clips.clear();
        self.state
            .selected_clips
            .insert(ArrangementSelection::AudioClip {
                track_id,
                clip_id: new_id,
            });
        self.state.status_text = "Joined audio clips".to_string();
    }

    fn join_note_clips(&mut self, track_id: TrackId, selections: &[ArrangementSelection]) {
        let clip_ids: Vec<ClipId> = selections
            .iter()
            .filter_map(|s| match s {
                ArrangementSelection::NoteClip { clip_id, .. } => Some(*clip_id),
                _ => None,
            })
            .collect();

        let mut clip_data: Vec<(f64, f64, Vec<MidiNote>)> = Vec::new();
        if let Some(track) = self.state.find_track(track_id) {
            for cid in &clip_ids {
                if let Some(clip) = track.note_clips.iter().find(|c| c.id == *cid) {
                    clip_data.push((clip.position_beats, clip.duration_beats, clip.notes.clone()));
                }
            }
        }

        if clip_data.len() < 2 {
            return;
        }

        // Sort by position
        clip_data.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let start_pos = clip_data[0].0;
        let end_pos = clip_data
            .iter()
            .map(|(pos, dur, _)| pos + dur)
            .fold(0.0_f64, f64::max);
        let total_duration = end_pos - start_pos;

        // Merge notes with adjusted offsets
        let mut merged_notes: Vec<MidiNote> = Vec::new();
        for (pos, _, notes) in &clip_data {
            let offset = pos - start_pos;
            for note in notes {
                merged_notes.push(MidiNote {
                    start_beat: note.start_beat + offset,
                    ..*note
                });
            }
        }

        // Remove all originals
        for cid in &clip_ids {
            self.send_command(EngineCommand::RemoveNoteClip(track_id, *cid));
            if let Some(track) = self.state.find_track_mut(track_id) {
                track.note_clips.retain(|c| c.id != *cid);
            }
        }

        // Add joined clip
        let new_id = ClipId::new();
        self.send_command(EngineCommand::AddNoteClip {
            track_id,
            clip_id: new_id,
            position_beats: start_pos,
            duration_beats: total_duration,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        });
        for note in &merged_notes {
            self.send_command(EngineCommand::AddNote {
                track_id,
                clip_id: new_id,
                note: *note,
            });
        }
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.note_clips.push(UiNoteClip {
                id: new_id,
                name: "Joined".to_string(),
                position_beats: start_pos,
                duration_beats: total_duration,
                notes: merged_notes,
                selected_notes: HashSet::new(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            });
        }

        self.state.selected_clips.clear();
        self.state
            .selected_clips
            .insert(ArrangementSelection::NoteClip {
                track_id,
                clip_id: new_id,
            });
        self.state.selected_note_clip = Some((track_id, new_id));
        self.state.status_text = "Joined note clips".to_string();
    }

    fn poll_plugin_loads(&mut self) {
        // Poll for loaded plugin effects
        while let Ok(result) = self.plugin_effect_rx.try_recv() {
            let track_id = result.track_id;
            let effect_id = result.effect_id;
            let plugin_name = result.plugin_name.clone();

            // Phase 2: If this is a CLAP plugin, finish init on the UI thread.
            // This is critical — JUCE creates its MessageManager during init(),
            // and guiCreate() needs to be on the same thread.
            let (effect, gui_raw_ptr): (
                Box<dyn vibez_dsp::effect::AudioEffect>,
                Option<PluginRawPtr>,
            ) = if let Some(partial) = result.clap_partial {
                match vibez_plugin_host::clap_host::instance::ClapPluginInstance::init_on_main_thread(
                    partial,
                    result.sample_rate,
                    4096,
                ) {
                    Ok(clap_inst) => {
                        let raw_ptr = Some(PluginRawPtr::Clap(
                            clap_inst.plugin_ptr() as *const std::ffi::c_void,
                        ));
                        let wrapper =
                            vibez_plugin_host::PluginEffectWrapper::new(Box::new(clap_inst));
                        (Box::new(wrapper), raw_ptr)
                    }
                    Err(e) => {
                        eprintln!("vibez: CLAP init failed on UI thread: {e}");
                        self.state.status_text = format!("Plugin init failed: {e}");
                        continue;
                    }
                }
            } else if let Some(effect) = result.effect {
                (effect, result.gui_raw_ptr)
            } else {
                continue;
            };

            let has_gui = gui_raw_ptr.is_some();

            if let Some(raw_ptr) = gui_raw_ptr {
                let key = PluginGuiKey::Effect {
                    track_id,
                    effect_id,
                };
                self.plugin_gui_raw_ptrs.insert(key, raw_ptr);
            }

            if let Some(track) = self.state.find_track_mut(track_id) {
                let descriptors: &'static [vibez_core::effect::ParamDescriptor] =
                    Box::leak(Vec::new().into_boxed_slice());
                track.effects.push(UiEffect {
                    id: effect_id,
                    effect_type: EffectType::Gain,
                    bypass: false,
                    params: Vec::new(),
                    descriptors,
                    plugin_name: Some(plugin_name.clone()),
                    has_plugin_gui: has_gui,
                });
            }
            self.send_command(EngineCommand::AddPluginEffect {
                track_id,
                effect_id,
                effect,
                position: None,
            });
            self.state.status_text = format!("Loaded {plugin_name}");
        }

        // Poll for loaded plugin instruments
        while let Ok(result) = self.plugin_instrument_rx.try_recv() {
            let track_id = result.track_id;
            let plugin_name = result.plugin_name.clone();

            // Phase 2: finish CLAP init on UI thread
            let (instrument, gui_raw_ptr): (
                Box<dyn vibez_instruments::Instrument>,
                Option<PluginRawPtr>,
            ) = if let Some(partial) = result.clap_partial {
                match vibez_plugin_host::clap_host::instance::ClapPluginInstance::init_on_main_thread(
                    partial,
                    result.sample_rate,
                    4096,
                ) {
                    Ok(clap_inst) => {
                        let raw_ptr = Some(PluginRawPtr::Clap(
                            clap_inst.plugin_ptr() as *const std::ffi::c_void,
                        ));
                        let wrapper =
                            vibez_plugin_host::PluginInstrumentWrapper::new(Box::new(clap_inst));
                        (Box::new(wrapper), raw_ptr)
                    }
                    Err(e) => {
                        eprintln!("vibez: CLAP instrument init failed on UI thread: {e}");
                        self.state.status_text = format!("Plugin init failed: {e}");
                        continue;
                    }
                }
            } else if let Some(instrument) = result.instrument {
                (instrument, result.gui_raw_ptr)
            } else {
                continue;
            };

            let has_gui = gui_raw_ptr.is_some();

            if let Some(raw_ptr) = gui_raw_ptr {
                let key = PluginGuiKey::Instrument { track_id };
                self.plugin_gui_raw_ptrs.insert(key, raw_ptr);
            }

            if let Some(track) = self.state.find_track_mut(track_id) {
                track.has_instrument = true;
                track.plugin_instrument_name = Some(plugin_name.clone());
                track.has_plugin_instrument_gui = has_gui;
            }
            self.send_command(EngineCommand::SetPluginInstrument {
                track_id,
                instrument,
            });
            self.state.status_text = format!("Loaded {plugin_name}");
        }
    }

    fn poll_plugin_windows(&mut self) {
        if let Some(ref mut mgr) = self.plugin_window_manager {
            for event in mgr.poll_events() {
                match event {
                    PluginWindowEvent::Closed(_key) => {
                        self.state.status_text = "Plugin GUI closed".to_string();
                    }
                }
            }
        }
    }

    fn poll_engine_events(&mut self) {
        if let Some(ref mut rx) = self.event_rx {
            while let Ok(event) = rx.pop() {
                match event {
                    EngineEvent::PlaybackPosition(pos) => {
                        self.state.position_samples = pos;
                    }
                    EngineEvent::Metering { peak_l, peak_r, .. } => {
                        self.state.peak_l = peak_l.max(self.state.peak_l * 0.85);
                        self.state.peak_r = peak_r.max(self.state.peak_r * 0.85);
                    }
                    EngineEvent::PlaybackStopped => {
                        self.state.playing = false;
                    }
                    EngineEvent::PlaybackStarted => {
                        self.state.playing = true;
                    }
                    EngineEvent::TrackMeter {
                        track_id,
                        peak_l,
                        peak_r,
                    } => {
                        if let Some(track) = self.state.find_track_mut(track_id) {
                            track.peak_l = peak_l.max(track.peak_l * 0.85);
                            track.peak_r = peak_r.max(track.peak_r * 0.85);
                        }
                    }
                }
            }
        }
    }

    // ── View ──

    fn view(&self) -> Element<'_, Message> {
        let header = self.view_header();

        let content = match self.state.workspace {
            Workspace::Arrange => self.view_arrangement(),
            Workspace::Mix => self.view_mixer(),
        };

        let detail_panel = self.view_detail_panel();
        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![header, transport_bar, content, detail_panel, status_bar];

        let base_layout: Element<'_, Message> = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        if self.state.settings_open {
            stack![base_layout, self.view_settings_modal()].into()
        } else if self.state.file_menu_open {
            stack![base_layout, self.view_file_menu_overlay()].into()
        } else if self.state.context_menu.is_some() {
            stack![base_layout, self.view_context_menu_overlay()].into()
        } else if self.state.editing_clip_name.is_some() {
            stack![base_layout, self.view_rename_overlay()].into()
        } else if self.state.device_context_menu.is_some() {
            stack![base_layout, self.view_device_context_menu_overlay()].into()
        } else {
            base_layout
        }
    }

    fn view_file_menu_overlay(&self) -> Element<'_, Message> {
        let make_menu_btn =
            |label: &'static str, icon: char, msg: Message| {
                button(
                    row![
                        icons::icon(icon).size(12).color(th::TEXT),
                        text(label).size(12).color(th::TEXT)
                    ]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(msg)
                .padding([8, 16])
                .width(Length::Fill)
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT,
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
            };

        let new_btn = make_menu_btn("New Project", icons::PLUS, Message::NewProject);
        let open_btn = make_menu_btn("Open...", icons::MUSIC, Message::OpenProject);
        let save_label = if self.state.project_dirty {
            "Save*"
        } else {
            "Save"
        };
        let save_btn = make_menu_btn(save_label, icons::COPY, Message::SaveProject);
        let save_as_btn = make_menu_btn("Save As...", icons::COPY, Message::SaveProjectAs);
        let settings_btn = button(
            row![
                icons::icon(icons::SLIDERS_VERTICAL).size(12).color(th::TEXT),
                text("Settings...").size(12).color(th::TEXT)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::OpenSettings)
        .padding([8, 16])
        .width(Length::Fill)
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(th::BG_HOVER.into())
                }
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT,
                border: iced::Border::default(),
                ..Default::default()
            }
        });

        let menu_content = column![new_btn, open_btn, save_btn, save_as_btn, settings_btn]
            .spacing(2)
            .padding(4)
            .width(Length::Fixed(160.0));

        let menu_card = container(menu_content).style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // Position below the header, near the File button
        let padded = column![
            vertical_space().height(Length::Fixed(42.0)),
            row![
                horizontal_space().width(Length::Fixed(60.0)),
                menu_card,
            ]
        ];

        mouse_area(
            container(padded)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::DismissFileMenu)
        .into()
    }

    fn view_settings_modal(&self) -> Element<'_, Message> {
        let title = text("Settings").size(18).color(th::ACCENT);
        let close_btn = button(
            icons::icon(icons::X).size(14).color(th::TEXT_DIM),
        )
        .on_press(Message::CloseSettings)
        .padding([4, 8])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(th::BG_HOVER.into())
                }
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT_DIM,
                border: iced::Border::default(),
                ..Default::default()
            }
        });

        let header = row![title, horizontal_space(), close_btn].align_y(iced::Alignment::Center);

        // -- Tab bar --
        let make_tab_btn =
            |label: &'static str, tab: SettingsTab, is_active: bool| {
                let color = if is_active { th::ACCENT } else { th::TEXT_DIM };
                button(text(label).size(13).color(color))
                    .on_press(Message::SelectSettingsTab(tab))
                    .padding([6, 16])
                    .style(move |_theme: &Theme, status| {
                        let bg = if is_active {
                            None
                        } else {
                            match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(th::BG_HOVER.into())
                                }
                                _ => None,
                            }
                        };
                        button::Style {
                            background: bg,
                            text_color: color,
                            border: iced::Border {
                                color: if is_active {
                                    th::ACCENT
                                } else {
                                    Color::TRANSPARENT
                                },
                                width: if is_active { 2.0 } else { 0.0 },
                                radius: 0.0.into(),
                            },
                            ..Default::default()
                        }
                    })
            };

        let active = self.state.settings_tab;
        let tab_bar = row![
            make_tab_btn("Audio", SettingsTab::Audio, active == SettingsTab::Audio),
            make_tab_btn("Plugins", SettingsTab::Plugins, active == SettingsTab::Plugins),
        ]
        .spacing(0);

        // -- Tab body --
        let tab_body: Element<'_, Message> = match self.state.settings_tab {
            SettingsTab::Audio => self.view_settings_audio_tab(),
            SettingsTab::Plugins => self.view_settings_plugins_tab(),
        };

        let content = column![
            header,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }),
            tab_bar,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }),
            tab_body,
        ]
        .spacing(8)
        .padding(20)
        .width(Length::Fixed(480.0));

        let dialog = container(content).style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 8.0.into(),
            },
            ..Default::default()
        });

        // Centered overlay with dimmed background
        mouse_area(
            container(
                center(dialog)
                    .width(Length::Fill)
                    .height(Length::Fill),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.5).into()),
                ..Default::default()
            }),
        )
        .on_press(Message::CloseSettings)
        .into()
    }

    fn view_settings_audio_tab(&self) -> Element<'_, Message> {
        let buf_label = text("Buffer Size").size(14).color(th::TEXT);
        let buf_hint = text("Lower = less latency, higher = more CPU headroom")
            .size(11)
            .color(th::TEXT_DIM);

        let sizes: &[u32] = &[64, 128, 256, 512, 1024, 2048, 4096];
        let mut buf_row = row![].spacing(4);
        for &size in sizes {
            let is_selected = self.state.settings_buffer_size == size;
            let label = format!("{size}");
            let btn = button(text(label).size(11).color(if is_selected {
                th::TEXT
            } else {
                th::TEXT_DIM
            }))
            .on_press(Message::SetBufferSize(size))
            .padding([6, 10])
            .style(move |_theme: &Theme, status| {
                if is_selected {
                    button::Style {
                        background: Some(th::ACCENT.into()),
                        text_color: th::TEXT,
                        border: iced::Border {
                            color: th::ACCENT,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::TEXT_DIM,
                        border: iced::Border {
                            color: th::BORDER,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                }
            });
            buf_row = buf_row.push(btn);
        }

        let sr_label = text("Sample Rate").size(14).color(th::TEXT);
        let sr_value = text(format!("{} Hz", self.state.sample_rate))
            .size(13)
            .color(th::TEXT_DIM);

        column![buf_label, buf_hint, buf_row, sr_label, sr_value]
            .spacing(8)
            .into()
    }

    fn view_settings_plugins_tab(&self) -> Element<'_, Message> {
        // Plugin section header
        let plugin_title = text("Plugin Library").size(14).color(th::TEXT);

        // Default paths checkbox
        let default_paths_label = if self.state.plugin_settings.scan_default_paths {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::ACCENT)
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::TEXT_DIM)
        };
        let default_paths_btn = button(
            row![
                default_paths_label,
                text("Scan default system paths").size(12).color(th::TEXT)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleScanDefaultPaths)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::TEXT,
            border: iced::Border::default(),
            ..Default::default()
        });

        // Scan paths list
        let mut paths_col = column![].spacing(4);
        for (i, path) in self.state.plugin_settings.extra_scan_paths.iter().enumerate() {
            let path_text = text(path.display().to_string()).size(11).color(th::TEXT_DIM);
            let remove_btn = button(
                icons::icon(icons::X).size(10).color(th::DANGER),
            )
            .on_press(Message::RemovePluginScanPath(i))
            .padding([2, 6])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::BG_HOVER.into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::DANGER,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });
            paths_col = paths_col.push(
                row![path_text, horizontal_space(), remove_btn]
                    .align_y(iced::Alignment::Center)
                    .spacing(4),
            );
        }

        let add_path_btn = button(
            row![
                icons::icon(icons::PLUS).size(12).color(th::ACCENT),
                text("Add Path").size(12).color(th::ACCENT)
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::AddPluginScanPath)
        .padding([6, 12])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(th::BG_HOVER.into())
                }
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::ACCENT,
                border: iced::Border {
                    color: th::ACCENT_DIM,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        // Scan button
        let scan_label = if self.state.plugin_scan_in_progress {
            "Scanning..."
        } else {
            "Scan Plugins"
        };
        let scan_btn = button(
            text(scan_label).size(12).color(th::TEXT),
        )
        .on_press(Message::ScanPlugins)
        .padding([8, 16])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(th::ACCENT_DIM.into())
                }
                _ => Some(th::BG_ELEVATED.into()),
            };
            button::Style {
                background: bg,
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        // Status
        let cache_count = self.state.plugin_settings.cache.len();
        let status = if !self.state.plugin_scan_status.is_empty() {
            text(&self.state.plugin_scan_status).size(11).color(th::TEXT_DIM)
        } else {
            text(format!("{cache_count} plugins cached")).size(11).color(th::TEXT_DIM)
        };

        column![
            plugin_title,
            default_paths_btn,
            paths_col,
            row![add_path_btn, horizontal_space(), scan_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            status,
        ]
        .spacing(8)
        .into()
    }

    fn view_rename_overlay(&self) -> Element<'_, Message> {
        let input = text_input("Name", &self.state.edit_name_text)
            .on_input(Message::EditNameText)
            .on_submit(Message::FinishEditing)
            .size(14)
            .width(Length::Fixed(250.0));

        let label = text("Rename Clip").size(14).color(th::TEXT);

        let dialog = container(
            column![label, input]
                .spacing(8)
                .padding(16)
                .width(Length::Fixed(280.0)),
        )
        .style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        let centered = center(dialog).width(Length::Fill).height(Length::Fill);

        mouse_area(centered)
            .on_press(Message::CancelEditing)
            .into()
    }

    fn view_context_menu_overlay(&self) -> Element<'_, Message> {
        let menu = self.state.context_menu.as_ref().unwrap();
        let x = menu.x;
        let y = menu.y;

        let menu_btn =
            |icon_char: char, label_text: String, msg: Message| -> Element<'_, Message> {
                button(
                    row![
                        icons::icon(icon_char).size(13).color(th::TEXT),
                        text(label_text).size(13).color(th::TEXT)
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(msg)
                .padding([6, 12])
                .width(Length::Fill)
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => th::BG_HOVER,
                        _ => th::BG_SURFACE,
                    };
                    button::Style {
                        background: Some(bg.into()),
                        text_color: th::TEXT,
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
                .into()
            };

        let menu_items: Element<'_, Message> = match &menu.target {
            ContextMenuTarget::Clip {
                track_id,
                clip_id,
                is_note_clip,
            } => {
                let track_id = *track_id;
                let clip_id = *clip_id;
                let is_note_clip = *is_note_clip;

                let mut col = column![].spacing(0).width(Length::Fixed(200.0));

                col = col.push(menu_btn(
                    icons::TRASH_2,
                    "Delete".into(),
                    Message::DeleteSelectedClip,
                ));
                col = col.push(menu_btn(
                    icons::COPY,
                    "Duplicate".into(),
                    Message::DuplicateSelectedClip,
                ));

                // Split at playhead
                let playhead_beats = self.state.position_beats();
                if is_note_clip {
                    col = col.push(menu_btn(
                        icons::SCISSORS,
                        "Split at Playhead".into(),
                        Message::SplitNoteClip {
                            track_id,
                            clip_id,
                            split_beat: playhead_beats,
                        },
                    ));
                } else {
                    let split_sample = self.state.position_samples;
                    col = col.push(menu_btn(
                        icons::SCISSORS,
                        "Split at Playhead".into(),
                        Message::SplitAudioClip {
                            track_id,
                            clip_id,
                            split_position: split_sample,
                        },
                    ));
                }

                // Rename clip
                col = col.push(menu_btn(
                    icons::PENCIL,
                    "Rename".into(),
                    Message::StartEditingClipName(track_id, clip_id),
                ));

                col.into()
            }
            ContextMenuTarget::TimeSelection {
                start_beats,
                end_beats,
                track_id,
            } => {
                let start = *start_beats;
                let end = *end_beats;
                let mut col = column![].spacing(0).width(Length::Fixed(200.0));

                // "Create Note Clip" if track is an instrument track
                let effective_track = track_id.or(self.state.selected_track);
                if let Some(tid) = effective_track {
                    if let Some(track) = self.state.find_track(tid) {
                        if track.kind.is_midi() {
                            col = col.push(menu_btn(
                                icons::MUSIC,
                                "Create Note Clip".into(),
                                Message::CreateNoteClipFromSelection(tid),
                            ));
                        }
                    }
                }

                col = col.push(menu_btn(
                    icons::SCISSORS,
                    "Split Clips at Region".into(),
                    Message::SplitClipsAtRegion {
                        start_beats: start,
                        end_beats: end,
                    },
                ));
                col = col.push(menu_btn(
                    icons::TRASH_2,
                    "Delete Clips in Region".into(),
                    Message::DeleteClipsInRegion {
                        start_beats: start,
                        end_beats: end,
                    },
                ));
                col = col.push(menu_btn(
                    icons::REPEAT,
                    "Set as Loop Region".into(),
                    Message::SetSelectionAsLoop,
                ));

                col.into()
            }
            ContextMenuTarget::ArrangementEmpty => column![
                menu_btn(
                    icons::AUDIO_WAVEFORM,
                    "Add Audio Track".into(),
                    Message::AddTrack,
                ),
                menu_btn(
                    icons::MUSIC,
                    "Add MIDI Track".into(),
                    Message::AddInstrumentTrack,
                ),
            ]
            .spacing(0)
            .width(Length::Fixed(200.0))
            .into(),
        };

        let menu_container = container(menu_items)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .padding(4);

        // Position menu at (x, y) using spacers in a column+row layout
        let positioned = column![
            vertical_space().height(Length::Fixed(y)),
            row![horizontal_space().width(Length::Fixed(x)), menu_container,]
        ];

        // Full-screen click-eating backdrop
        mouse_area(
            container(positioned)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::DismissContextMenu)
        .into()
    }

    fn view_header(&self) -> Element<'_, Message> {
        let title = text("vibez").size(22).color(th::ACCENT);

        // Workspace tabs
        let arrange_tab = {
            let active = self.state.workspace == Workspace::Arrange;
            let (bg, text_color, border_color) = if active {
                (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::TEXT_DIM,
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::LAYOUT_LIST).size(13).color(text_color),
                    text("Arrange").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SwitchWorkspace(Workspace::Arrange))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let mix_tab = {
            let active = self.state.workspace == Workspace::Mix;
            let (bg, text_color, border_color) = if active {
                (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::TEXT_DIM,
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::SLIDERS_VERTICAL)
                        .size(13)
                        .color(text_color),
                    text("Mix").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::SwitchWorkspace(Workspace::Mix))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let tabs = row![arrange_tab, mix_tab].spacing(4);

        let file_btn = button(
            text("File").size(13).color(th::TEXT_DIM),
        )
        .on_press(Message::ToggleFileMenu)
        .padding([6, 14])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => {
                    Some(th::BG_HOVER.into())
                }
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT_DIM,
                border: iced::Border::default(),
                ..Default::default()
            }
        });

        let header_row = row![title, file_btn, tabs, horizontal_space()].spacing(8);

        let header = header_row.padding(10).align_y(iced::Alignment::Center);

        container(header)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Arrangement view ──

    fn view_arrangement(&self) -> Element<'_, Message> {
        if self.state.tracks.is_empty() {
            let prompt = text("Right-click or Ctrl+T to add a track")
                .size(16)
                .color(th::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return mouse_area(
                container(centered)
                    .width(Length::Fill)
                    .height(Length::FillPortion(5))
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::BG_DARK.into()),
                        ..Default::default()
                    }),
            )
            .on_right_press(Message::ShowContextMenu {
                x: 400.0,
                y: 300.0,
                target: ContextMenuTarget::ArrangementEmpty,
            })
            .into();
        }

        let playhead_beats = self.state.position_beats();
        let sample_rate = self.state.sample_rate;
        let bpm = self.state.bpm;
        let zoom_level = self.state.zoom_level;
        let scroll_offset = self.state.scroll_offset_beats;
        let total_beats = self.state.total_beats();

        // Beat-based ruler across the top (offset by track header width)
        let ruler = RulerWidget {
            playhead_beats,
            bpm,
            zoom_level,
            scroll_offset_beats: scroll_offset,
            total_beats,
            loop_enabled: self.state.loop_enabled,
            loop_start_beats: self.state.loop_start_beats,
            loop_end_beats: self.state.loop_end_beats,
            time_selection_active: self.state.time_selection_active,
            selection_start_beats: self.state.selection_start_beats,
            selection_end_beats: self.state.selection_end_beats,
        };
        let ruler_canvas: Element<'_, Message> = canvas(ruler)
            .width(Length::Fill)
            .height(Length::Fixed(28.0))
            .into();

        // Spacer matching header width (including color bar) for the ruler row
        let ruler_spacer = container(text(""))
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            ))
            .height(Length::Fixed(28.0))
            .style(|_theme: &Theme| iced::widget::container::Style {
                background: Some(crate::theme::BG_SURFACE.into()),
                ..Default::default()
            });

        let ruler_row = row![ruler_spacer, ruler_canvas];

        // Arrangement overview minimap
        let spb = if bpm > 0.0 {
            60.0 * sample_rate as f64 / bpm
        } else {
            1.0
        };
        let minimap = ArrangementMinimap {
            total_beats,
            scroll_offset_beats: scroll_offset,
            zoom_level,
            playhead_beats,
            bpm,
            loop_enabled: self.state.loop_enabled,
            loop_start_beats: self.state.loop_start_beats,
            loop_end_beats: self.state.loop_end_beats,
            tracks: self
                .state
                .tracks
                .iter()
                .map(|t| {
                    let color = th::track_color(t.color_index);
                    let mut clips: Vec<(f64, f64)> = t
                        .clips
                        .iter()
                        .map(|c| (c.position as f64 / spb, c.duration as f64 / spb))
                        .collect();
                    clips.extend(
                        t.note_clips
                            .iter()
                            .map(|c| (c.position_beats, c.duration_beats)),
                    );
                    MinimapTrack { color, clips }
                })
                .collect(),
        };
        let minimap_spacer = container(text(""))
            .width(Length::Fixed(
                crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH,
            ))
            .height(Length::Fixed(40.0))
            .style(|_theme: &Theme| iced::widget::container::Style {
                background: Some(th::BG_SURFACE.into()),
                ..Default::default()
            });
        let minimap_canvas: Element<'_, Message> = canvas(minimap)
            .width(Length::Fill)
            .height(Length::Fixed(40.0))
            .into();
        let minimap_row = row![minimap_spacer, minimap_canvas];

        // Collect track IDs and kinds for cross-track drag
        let track_ids: Vec<TrackId> = self.state.tracks.iter().map(|t| t.id).collect();
        let track_kinds: Vec<bool> = self
            .state
            .tracks
            .iter()
            .map(|t| t.kind.is_midi())
            .collect();
        let total_track_count = self.state.tracks.len();

        // Track rows: header widgets + clip canvas
        let mut track_rows = column![].spacing(0);

        for (track_index, track) in self.state.tracks.iter().enumerate() {
            let selected = self.state.selected_track == Some(track.id);
            let track_color = th::track_color(track.color_index);

            // Collect selected clip IDs for this track
            let selected_clips: HashSet<ClipId> = self
                .state
                .selected_clips
                .iter()
                .filter_map(|sel| match sel {
                    ArrangementSelection::AudioClip { track_id, clip_id }
                        if *track_id == track.id =>
                    {
                        Some(*clip_id)
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id }
                        if *track_id == track.id =>
                    {
                        Some(*clip_id)
                    }
                    _ => None,
                })
                .collect();

            // Track header (iced widgets)
            let editing = self.state.editing_track_name == Some(track.id);
            let header = view_track_header(track, selected, editing, &self.state.edit_name_text);

            // Clip canvas for this track
            let clip_canvas_widget = TrackClipCanvas::from_track(
                track,
                playhead_beats,
                zoom_level,
                scroll_offset,
                total_beats,
                sample_rate,
                selected,
                track_color,
                bpm,
                track.id,
                track_index,
                total_track_count,
                track_ids.clone(),
                track_kinds.clone(),
                selected_clips,
                self.state.loop_enabled,
                self.state.loop_start_beats,
                self.state.loop_end_beats,
                self.state.time_selection_active,
                self.state.selection_start_beats,
                self.state.selection_end_beats,
            );
            let clip_canvas: Element<'_, Message> = canvas(clip_canvas_widget)
                .width(Length::Fill)
                .height(Length::Fixed(70.0))
                .into();

            let track_row = row![header, clip_canvas].height(Length::Fixed(70.0));

            track_rows = track_rows.push(track_row);
        }

        let content = column![ruler_row, minimap_row, track_rows];

        let scrollable_content = scrollable(content).direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::default(),
        ));

        // mouse_area only provides on_right_press (no cursor position),
        // so the right-click context menu from the scrollable background
        // opens at a default position. Track canvas right-clicks still
        // use the precise cursor location.
        mouse_area(
            container(scrollable_content)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
                    ..Default::default()
                }),
        )
        .on_right_press(Message::ShowContextMenu {
            x: 400.0,
            y: 300.0,
            target: ContextMenuTarget::ArrangementEmpty,
        })
        .into()
    }

    // ── Mixer view ──

    fn view_mixer(&self) -> Element<'_, Message> {
        if self.state.tracks.is_empty() {
            let prompt = text("Add a track to get started")
                .size(16)
                .color(th::TEXT_DIM);

            let centered = center(prompt).width(Length::Fill).height(Length::Fill);

            return container(centered)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
                    ..Default::default()
                })
                .into();
        }

        // ── Channel strips + pinned master ──
        let mut strips = row![].spacing(4).padding(8).height(Length::Fill);

        for track in &self.state.tracks {
            let strip = view_mixer_strip(track);
            strips = strips.push(strip);
        }

        // Master strip — pinned to far right
        let master_label = text("Master").size(12).color(th::TEXT);
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(32.0))
            .height(Length::Fill)
            .into();

        let master_col = column![master_label, master_meter_canvas]
            .spacing(4)
            .padding(8)
            .width(Length::Fixed(100.0))
            .height(Length::Fill)
            .align_x(iced::Alignment::Center);

        let master_container =
            container(master_col)
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                });

        let mixer_row = row![strips, horizontal_space(), master_container]
            .spacing(4)
            .padding([8, 4])
            .height(Length::Fill);

        let mixer_content = container(mixer_row)
            .width(Length::Fill)
            .height(Length::Fill);

        mouse_area(
            container(mixer_content)
                .width(Length::Fill)
                .height(Length::FillPortion(5))
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_DARK.into()),
                    ..Default::default()
                }),
        )
        .on_right_press(Message::ShowContextMenu {
            x: 400.0,
            y: 300.0,
            target: ContextMenuTarget::ArrangementEmpty,
        })
        .into()
    }

    // ── Detail panel (Ableton-style device chain) ──

    fn view_detail_panel(&self) -> Element<'_, Message> {
        let detail_content: Element<'_, Message> = if let Some(track) = self
            .state
            .selected_track
            .and_then(|id| self.state.find_track(id))
        {
            let track_id = track.id;
            let track_color = th::track_color(track.color_index);

            // Tab bar
            let clip_tab = {
                let active = self.state.detail_panel_tab == DetailPanelTab::Clip;
                let (bg, text_color, border_color) = if active {
                    (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::TEXT_DIM,
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Clip").size(12).color(text_color))
                    .on_press(Message::SwitchDetailTab(DetailPanelTab::Clip))
                    .padding([4, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(bg.into()),
                        text_color,
                        border: iced::Border {
                            color: border_color,
                            width: if active { 1.0 } else { 0.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
            };
            let devices_tab = {
                let active = self.state.detail_panel_tab == DetailPanelTab::Devices;
                let (bg, text_color, border_color) = if active {
                    (th::BG_ELEVATED, th::ACCENT, th::ACCENT_DIM)
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::TEXT_DIM,
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Devices").size(12).color(text_color))
                    .on_press(Message::SwitchDetailTab(DetailPanelTab::Devices))
                    .padding([4, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(bg.into()),
                        text_color,
                        border: iced::Border {
                            color: border_color,
                            width: if active { 1.0 } else { 0.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
            };
            let tab_bar = row![clip_tab, devices_tab].spacing(4).padding([4, 8]);

            // Tab content
            let tab_content: Element<'_, Message> = match self.state.detail_panel_tab {
                DetailPanelTab::Clip => {
                    let is_midi = track.kind.is_midi();
                    // Check for note clip selection on this MIDI track
                    let has_note_clip = is_midi
                        && (self.state.selected_clips.iter().any(|s| {
                            matches!(s, ArrangementSelection::NoteClip { track_id: tid, .. } if *tid == track_id)
                        }) || self
                            .state
                            .selected_note_clip
                            .is_some_and(|(tid, _)| tid == track_id));

                    if has_note_clip {
                        self.view_piano_roll_panel(track_id, track_color)
                    } else {
                        // Find a single selected audio clip on this track
                        let audio_sel = self.state.selected_clips.iter().find_map(|s| match s {
                            ArrangementSelection::AudioClip {
                                track_id: tid,
                                clip_id: cid,
                            } if *tid == track_id => Some(*cid),
                            _ => None,
                        });
                        if let Some(sel_cid) = audio_sel {
                            if let Some(clip) = track.clips.iter().find(|c| c.id == sel_cid) {
                                self.view_audio_clip_panel(clip, track_color)
                            } else {
                                self.view_clip_placeholder()
                            }
                        } else {
                            self.view_clip_placeholder()
                        }
                    }
                }
                DetailPanelTab::Devices => self.view_device_chain(track_id, track, track_color),
            };

            column![tab_bar, tab_content].height(Length::Fill).into()
        } else {
            let label = text("Select a track to view devices")
                .size(14)
                .color(th::TEXT_DIM);
            center(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        container(detail_content)
            .width(Length::Fill)
            .height(Length::FillPortion(2))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_clip_placeholder(&self) -> Element<'_, Message> {
        let label = text("Select a clip to view details")
            .size(14)
            .color(th::TEXT_DIM);
        center(label)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Build the device chain for the detail panel.
    fn view_device_chain<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        // Header: track name + right-click hint
        let track_label = text(format!("{} — Devices", track.name))
            .size(13)
            .color(th::TEXT);

        let hint_label = text("Right-click to add")
            .size(10)
            .color(th::TEXT_MUTED);

        let header = row![track_label, horizontal_space(), hint_label]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        // Device cards
        let mut devices_row = row![].spacing(6);

        // Instrument device card (branched by kind)
        if track.has_instrument {
            if track.plugin_instrument_name.is_some() {
                // External plugin instrument — clickable card
                let card =
                    self.view_plugin_instrument_device(track_id, track, track_color);
                devices_row = devices_row.push(card);
            } else {
                match track.instrument_kind {
                    Some(vibez_core::midi::InstrumentKind::Sampler) => {
                        let card = self.view_sampler_device(track_id, track, track_color);
                        devices_row = devices_row.push(card);
                    }
                    _ => {
                        let synth_card = self.view_synth_device(track_id, track_color);
                        devices_row = devices_row.push(synth_card);
                    }
                }
            }
        } else if track.kind.is_midi() {
            let placeholder = self.view_add_instrument_placeholder();
            devices_row = devices_row.push(placeholder);
        }

        // Effect cards
        for effect in &track.effects {
            let slot = view_effect_slot(track_id, effect, track_color);
            devices_row = devices_row.push(slot);
        }

        let scrollable_devices = scrollable(devices_row).direction(
            scrollable::Direction::Horizontal(scrollable::Scrollbar::default()),
        );

        let content = column![header, scrollable_devices]
            .spacing(6)
            .padding(8)
            .width(Length::Fill);

        // Wrap in mouse_area for right-click context menu
        mouse_area(content)
            .on_right_press(Message::ShowDeviceContextMenu {
                x: self.state.cursor_x,
                y: self.state.cursor_y,
                track_id,
            })
            .into()
    }

    // ── Shared device card helpers ──────────────────────────────────

    /// Dark title bar used by all device cards.
    fn device_title_bar<'a>(
        content: impl Into<Element<'a, Message>>,
    ) -> iced::widget::Container<'a, Message> {
        container(content)
            .padding([4, 6])
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                ..Default::default()
            })
    }

    /// Wrap card content in the standard device card container.
    fn device_card(content: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
        container(content)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_ELEVATED.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Small icon-only button for device card actions.
    fn device_icon_btn(
        icon_char: char,
        color: Color,
        hover_color: Color,
        msg: Message,
    ) -> iced::widget::Button<'static, Message> {
        button(icons::icon(icon_char).size(12).color(color))
            .on_press(msg)
            .padding([3, 5])
            .style(move |_theme: &Theme, status| {
                let (bg, tc) = match status {
                    button::Status::Hovered => (Some(th::BG_HOVER.into()), hover_color),
                    button::Status::Pressed => (Some(th::BG_DARK.into()), hover_color),
                    _ => (None, color),
                };
                button::Style {
                    background: bg,
                    text_color: tc,
                    border: iced::Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
    }

    /// Device card for an external plugin instrument.
    fn view_plugin_instrument_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        let dot = text("\u{25CF}").size(9).color(track_color);
        let plugin_name = track
            .plugin_instrument_name
            .as_deref()
            .unwrap_or("Plugin");

        let name_section =
            container(text(plugin_name).size(11).color(th::TEXT)).width(Length::Fill);

        // Edit button for plugins with a native GUI
        let edit_btn: Option<iced::widget::Button<'_, Message>> =
            if track.has_plugin_instrument_gui {
                let gui_key = PluginGuiKey::Instrument { track_id };
                Some(
                    button(text("Edit").size(9).color(th::TEXT_DIM))
                        .on_press(Message::OpenPluginGui(gui_key))
                        .padding([2, 5])
                        .style(|_theme: &Theme, status| {
                            let (bg, tc) = match status {
                                button::Status::Hovered => {
                                    (Some(th::BG_HOVER.into()), th::ACCENT)
                                }
                                _ => (None, th::TEXT_DIM),
                            };
                            button::Style {
                                background: bg,
                                text_color: tc,
                                border: iced::Border {
                                    color: th::BORDER,
                                    width: 1.0,
                                    radius: 3.0.into(),
                                },
                                ..Default::default()
                            }
                        }),
                )
            } else {
                None
            };

        let remove: Element<'a, Message> = Self::device_icon_btn(
            icons::X,
            th::TEXT_DIM,
            th::DANGER,
            Message::RemoveTrackInstrument(track_id),
        )
        .into();

        let mut title_row = row![dot, name_section]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        if let Some(eb) = edit_btn {
            title_row = title_row.push(eb);
        }
        title_row = title_row.push(remove);

        let title = Self::device_title_bar(title_row);

        Self::device_card(column![title].width(Length::Fixed(200.0)))
    }

    /// Synth device card for instrument tracks.
    fn view_synth_device(&self, _track_id: TrackId, track_color: Color) -> Element<'_, Message> {
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text("Synth").size(11).color(th::TEXT);

        let title = Self::device_title_bar(
            row![dot, name].spacing(4).align_y(iced::Alignment::Center),
        );

        let param_names = ["Attack", "Decay", "Sustain", "Release"];
        let mut params_col = column![].spacing(3);
        for pn in &param_names {
            let label = text(*pn).size(9).color(th::TEXT_DIM);
            let value = text("0.50").size(8).color(th::TEXT_MUTED);
            params_col = params_col.push(column![label, value].spacing(1));
        }
        let body = container(params_col).padding([6, 6]).width(Length::Fill);

        Self::device_card(column![title, body].width(Length::Fixed(120.0)))
    }

    /// Sampler device card.
    fn view_sampler_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text("Sampler").size(11).color(th::TEXT);

        let title = Self::device_title_bar(
            row![dot, name].spacing(4).align_y(iced::Alignment::Center),
        );

        let sample_label = match &track.sample_name {
            Some(name) => text(name.as_str()).size(10).color(th::TEXT),
            None => text("No Sample").size(10).color(th::TEXT_MUTED),
        };

        let load_btn = button(text("Load").size(9).color(th::TEXT))
            .on_press(Message::LoadSamplerSample(track_id))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => th::BG_HOVER,
                    _ => th::BG_DARK,
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::TEXT,
                    ..Default::default()
                }
            });

        let sample_row = row![sample_label, load_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        let param_names = ["Attack", "Decay", "Sustain", "Release", "Volume"];
        let mut params_col = column![].spacing(2);
        for pn in &param_names {
            let label = text(*pn).size(9).color(th::TEXT_DIM);
            let value = text("\u{2014}").size(8).color(th::TEXT_MUTED);
            params_col = params_col.push(column![label, value].spacing(1));
        }

        let body = container(column![sample_row, params_col].spacing(6))
            .padding([6, 6])
            .width(Length::Fill);

        Self::device_card(column![title, body].width(Length::Fixed(140.0)))
    }

    /// Placeholder card for MIDI tracks with no instrument attached.
    fn view_add_instrument_placeholder(&self) -> Element<'_, Message> {
        let title = Self::device_title_bar(
            text("No Instrument").size(11).color(th::TEXT_DIM),
        );
        let body = container(
            text("Right-click to add").size(9).color(th::TEXT_MUTED),
        )
        .padding([8, 6])
        .width(Length::Fill);

        Self::device_card(
            column![title, body]
                .width(Length::Fixed(120.0))
                .align_x(iced::Alignment::Center),
        )
    }

    /// Device context menu overlay (instruments + effects browser).
    fn view_device_context_menu_overlay(&self) -> Element<'_, Message> {
        use crate::state::DeviceMenuCategory;

        let menu = self.state.device_context_menu.as_ref().unwrap();
        let track_id = menu.track_id;
        let is_midi = self
            .state
            .find_track(track_id)
            .is_some_and(|t| t.kind.is_midi());

        // Category tabs
        let mut tabs_row = row![].spacing(2);
        if is_midi {
            let inst_active = menu.category == Some(DeviceMenuCategory::Instruments);
            let (bg, tc) = if inst_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            let inst_tab = button(text("Instruments").size(11).color(tc))
                .on_press(Message::SetDeviceMenuCategory(DeviceMenuCategory::Instruments))
                .padding([4, 10])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if inst_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
            tabs_row = tabs_row.push(inst_tab);
        }
        let fx_active = menu.category == Some(DeviceMenuCategory::Effects);
        let (bg, tc) = if fx_active {
            (th::ACCENT_DIM, th::ACCENT)
        } else {
            (th::BG_ELEVATED, th::TEXT_DIM)
        };
        let fx_tab = button(text("Effects").size(11).color(tc))
            .on_press(Message::SetDeviceMenuCategory(DeviceMenuCategory::Effects))
            .padding([4, 10])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color: tc,
                border: iced::Border {
                    color: if fx_active {
                        th::ACCENT_DIM
                    } else {
                        th::BORDER
                    },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
        tabs_row = tabs_row.push(fx_tab);

        // Plugins tab
        let plugins_active = menu.category == Some(DeviceMenuCategory::Plugins);
        let (bg, tc) = if plugins_active {
            (th::ACCENT_DIM, th::ACCENT)
        } else {
            (th::BG_ELEVATED, th::TEXT_DIM)
        };
        let plugins_tab = button(text("Plugins").size(11).color(tc))
            .on_press(Message::SetDeviceMenuCategory(DeviceMenuCategory::Plugins))
            .padding([4, 10])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color: tc,
                border: iced::Border {
                    color: if plugins_active {
                        th::ACCENT_DIM
                    } else {
                        th::BORDER
                    },
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
        tabs_row = tabs_row.push(plugins_tab);

        // Search input
        let search_input = text_input("Search...", &menu.search)
            .on_input(Message::DeviceMenuSearch)
            .size(12)
            .width(Length::Fill);

        // Items list
        let mut items_col = column![].spacing(2);
        let search_lower = menu.search.to_lowercase();

        match menu.category {
            Some(DeviceMenuCategory::Instruments) => {
                for &kind in InstrumentKind::all() {
                    let name = kind.name();
                    if !search_lower.is_empty() && !name.to_lowercase().contains(&search_lower) {
                        continue;
                    }
                    let btn = button(text(name).size(12).color(th::TEXT))
                        .on_press(Message::SetTrackInstrument(track_id, kind))
                        .padding([6, 10])
                        .width(Length::Fill)
                        .style(|_theme: &Theme, status| {
                            let bg = match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(th::BG_HOVER.into())
                                }
                                _ => None,
                            };
                            button::Style {
                                background: bg,
                                text_color: th::TEXT,
                                border: iced::Border::default(),
                                ..Default::default()
                            }
                        });
                    items_col = items_col.push(btn);
                }
            }
            Some(DeviceMenuCategory::Plugins) => {
                if self.state.plugin_settings.cache.is_empty() {
                    items_col = items_col.push(
                        text("No plugins scanned yet.\nUse File → Settings to scan.")
                            .size(11)
                            .color(th::TEXT_DIM),
                    );
                } else {
                    for plugin in &self.state.plugin_settings.cache {
                        let name = &plugin.name;
                        if !search_lower.is_empty()
                            && !name.to_lowercase().contains(&search_lower)
                            && !plugin.vendor.to_lowercase().contains(&search_lower)
                        {
                            continue;
                        }
                        let format_badge = match plugin.format {
                            PluginFormat::Clap => "CLAP",
                            PluginFormat::Vst3 => "VST3",
                        };
                        let cat_label = match plugin.category {
                            PluginCategory::Effect => "fx",
                            PluginCategory::Instrument => "inst",
                            PluginCategory::Both => "fx+inst",
                        };
                        let label_text = format!("{name}  [{format_badge}] {cat_label}");
                        let plugin_id = plugin.id.clone();
                        let btn = button(text(label_text).size(11).color(th::TEXT))
                            .on_press(Message::AddPluginToTrack(track_id, plugin_id))
                            .padding([6, 10])
                            .width(Length::Fill)
                            .style(|_theme: &Theme, status| {
                                let bg = match status {
                                    button::Status::Hovered | button::Status::Pressed => {
                                        Some(th::BG_HOVER.into())
                                    }
                                    _ => None,
                                };
                                button::Style {
                                    background: bg,
                                    text_color: th::TEXT,
                                    border: iced::Border::default(),
                                    ..Default::default()
                                }
                            });
                        items_col = items_col.push(btn);
                    }
                }
            }
            Some(DeviceMenuCategory::Effects) | None => {
                for &et in EffectType::all() {
                    let name = et.name();
                    if !search_lower.is_empty() && !name.to_lowercase().contains(&search_lower) {
                        continue;
                    }
                    let btn = button(text(name).size(12).color(th::TEXT))
                        .on_press(Message::AddEffect(track_id, et))
                        .padding([6, 10])
                        .width(Length::Fill)
                        .style(|_theme: &Theme, status| {
                            let bg = match status {
                                button::Status::Hovered | button::Status::Pressed => {
                                    Some(th::BG_HOVER.into())
                                }
                                _ => None,
                            };
                            button::Style {
                                background: bg,
                                text_color: th::TEXT,
                                border: iced::Border::default(),
                                ..Default::default()
                            }
                        });
                    items_col = items_col.push(btn);
                }
            }
        }

        let menu_content = column![tabs_row, search_input, items_col]
            .spacing(6)
            .padding(8)
            .width(Length::Fixed(220.0));

        let menu_card = container(menu_content).style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // Position the menu near where it was triggered
        let padded = column![
            vertical_space().height(Length::Fixed(menu.y.max(0.0))),
            row![
                horizontal_space().width(Length::Fixed(menu.x.max(0.0))),
                menu_card,
            ]
        ];

        mouse_area(
            container(padded)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::DismissDeviceContextMenu)
        .into()
    }

    /// Piano roll panel for the detail panel split view.
    fn view_piano_roll_panel(&self, track_id: TrackId, track_color: Color) -> Element<'_, Message> {
        use crate::state::PianoRollEditMode;

        let playhead_beats = self.state.position_beats();

        // Extract clip data as owned values (avoids lifetime conflicts with widget construction)
        let clip_data: Option<(String, f64, f64, bool, TrackId, ClipId)> =
            if let Some((tid, cid)) = self.state.selected_note_clip {
                if tid == track_id {
                    self.state
                        .tracks
                        .iter()
                        .find(|t| t.id == track_id)
                        .and_then(|t| t.note_clips.iter().find(|c| c.id == cid))
                        .map(|c| {
                            (
                                c.name.clone(),
                                c.position_beats,
                                c.duration_beats,
                                c.loop_enabled,
                                tid,
                                cid,
                            )
                        })
                } else {
                    None
                }
            } else {
                None
            };

        let piano_widget = if let Some(ref cd) = clip_data {
            if let Some(track) = self.state.find_track(track_id) {
                if let Some(clip) = track.note_clips.iter().find(|c| c.id == cd.5) {
                    let clip_relative_playhead = playhead_beats - clip.position_beats;
                    PianoRollWidget::from_clip(
                        track_id,
                        clip,
                        clip_relative_playhead,
                        clip.duration_beats,
                        track_color,
                        self.state.snap_grid,
                        self.state.piano_roll_scroll_y,
                        self.state.piano_roll_edit_mode,
                    )
                } else {
                    PianoRollWidget::empty(track_id, playhead_beats, track_color)
                }
            } else {
                PianoRollWidget::empty(track_id, playhead_beats, track_color)
            }
        } else {
            PianoRollWidget::empty(track_id, playhead_beats, track_color)
        };

        let piano_canvas: Element<'_, Message> = canvas(piano_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        // ── Clip properties bar (shown when a clip is selected) ──
        let mut content_col = column![].spacing(2).padding(4);

        if let Some((ref clip_name_str, clip_pos, clip_dur, clip_loop, tid, cid)) = clip_data {
            let clip_name = text(clip_name_str.clone()).size(11).color(th::TEXT);
            let pos_label = text(format!("Pos: {clip_pos:.1}"))
                .size(10)
                .color(th::TEXT_DIM);
            let dur_label = text(format!("Dur: {clip_dur:.1}"))
                .size(10)
                .color(th::TEXT_DIM);

            // Loop toggle
            let loop_icon_color = if clip_loop {
                th::ACCENT
            } else {
                th::TEXT_DIM
            };
            let loop_btn = button(icons::icon(icons::REPEAT).size(10).color(loop_icon_color))
                .on_press(Message::ToggleNoteClipLoop(tid, cid))
                .padding([2, 4])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: if clip_loop {
                        Some(th::ACCENT_DIM.into())
                    } else {
                        Some(th::BG_ELEVATED.into())
                    },
                    text_color: loop_icon_color,
                    border: iced::Border {
                        color: if clip_loop {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

            // Clip operation buttons
            let op_btn_style = |_theme: &Theme, _status| button::Style {
                background: Some(th::BG_ELEVATED.into()),
                text_color: th::TEXT_DIM,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            };

            let dup_btn = button(
                row![
                    icons::icon(icons::COPY).size(10).color(th::TEXT_DIM),
                    text("Dup").size(10).color(th::TEXT_DIM)
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::DuplicateNoteClip(tid, cid))
            .padding([2, 6])
            .style(op_btn_style);

            let double_btn = button(text("2x").size(10).color(th::TEXT_DIM))
                .on_press(Message::DoubleNoteClip(tid, cid))
                .padding([2, 6])
                .style(op_btn_style);

            let halve_btn = button(text("\u{00BD}x").size(10).color(th::TEXT_DIM))
                .on_press(Message::HalveNoteClip(tid, cid))
                .padding([2, 6])
                .style(op_btn_style);

            let crop_btn = button(
                row![
                    icons::icon(icons::SCISSORS).size(10).color(th::TEXT_DIM),
                    text("Crop").size(10).color(th::TEXT_DIM)
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::CropNoteClip(tid, cid))
            .padding([2, 6])
            .style(op_btn_style);

            let props_row = row![
                clip_name,
                pos_label,
                dur_label,
                loop_btn,
                dup_btn,
                double_btn,
                halve_btn,
                crop_btn,
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);

            content_col = content_col.push(props_row);
        }

        // ── Header row: label, edit mode toggle, snap grid ──
        let label = text("Piano Roll").size(11).color(th::TEXT_DIM);

        // Edit mode toggle: Select / Draw
        let select_active = self.state.piano_roll_edit_mode == PianoRollEditMode::Select;
        let draw_active = self.state.piano_roll_edit_mode == PianoRollEditMode::Draw;

        let select_btn = {
            let (bg, tc) = if select_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            button(icons::icon(icons::MOUSE_POINTER).size(10).color(tc))
                .on_press(Message::TogglePianoRollEditMode)
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if select_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let draw_btn = {
            let (bg, tc) = if draw_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            button(icons::icon(icons::PENCIL).size(10).color(tc))
                .on_press(Message::TogglePianoRollEditMode)
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if draw_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let mode_row = row![select_btn, draw_btn].spacing(1);

        // Snap grid selector
        use crate::state::SnapGrid;
        let mut snap_row = row![].spacing(2);
        for &grid in SnapGrid::all() {
            let is_active = self.state.snap_grid == grid;
            let (bg, text_color) = if is_active {
                (th::ACCENT_DIM, th::ACCENT)
            } else {
                (th::BG_ELEVATED, th::TEXT_DIM)
            };
            let btn = button(text(grid.label()).size(10).color(text_color))
                .on_press(Message::SetSnapGrid(grid))
                .padding([2, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color,
                    border: iced::Border {
                        color: if is_active {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            snap_row = snap_row.push(btn);
        }
        let snap_label = text("Snap:").size(10).color(th::TEXT_DIM);
        let header_row =
            row![label, mode_row, horizontal_space(), snap_label, snap_row]
                .spacing(4)
                .align_y(iced::Alignment::Center);

        content_col = content_col.push(header_row).push(piano_canvas);

        container(content_col)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Audio clip waveform panel for the detail panel split view.
    fn view_audio_clip_panel(&self, clip: &UiClip, track_color: Color) -> Element<'_, Message> {
        let playhead_samples = self.state.position_samples;
        let playhead_normalized = if clip.duration > 0
            && playhead_samples >= clip.position
            && playhead_samples < clip.position + clip.duration
        {
            (playhead_samples - clip.position) as f64 / clip.duration as f64
        } else {
            -1.0
        };

        let waveform_widget = AudioClipDetailWidget {
            audio: Arc::clone(&clip.audio),
            duration_samples: clip.duration,
            source_offset: clip.source_offset,
            sample_rate: self.state.sample_rate,
            track_color,
            playhead_normalized,
            loop_enabled: clip.loop_enabled,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
        };

        let waveform_canvas: Element<'_, Message> = canvas(waveform_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();

        let label = text("Waveform").size(11).color(th::TEXT_DIM);
        let clip_info = text(format!(
            "{} — {:.1}s",
            clip.name,
            clip.duration as f64 / self.state.sample_rate as f64
        ))
        .size(10)
        .color(th::TEXT_MUTED);

        let header_row = row![label, horizontal_space(), clip_info]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        let content = column![header_row, waveform_canvas].spacing(4).padding(4);

        container(content)
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    // ── Transport bar ──

    fn view_transport(&self) -> Element<'_, Message> {
        // Skip back button
        let skip_back_btn = button(icons::icon(icons::SKIP_BACK).size(16).color(th::TEXT))
            .on_press(Message::Stop)
            .padding([8, 12])
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(th::BG_ELEVATED.into()),
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // Play/Pause button
        let play_pause_btn = if self.state.playing {
            button(icons::icon(icons::PAUSE).size(16).color(th::ACCENT))
                .on_press(Message::Stop)
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::ACCENT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::PLAY).size(16).color(th::SUCCESS))
                .on_press(Message::Play)
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::SUCCESS,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        // Loop toggle button
        let loop_btn = if self.state.loop_enabled {
            button(icons::icon(icons::REPEAT).size(16).color(th::ACCENT))
                .on_press(Message::ToggleArrangementLoop)
                .padding([8, 12])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::ACCENT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::REPEAT).size(16).color(th::TEXT_DIM))
                .on_press(Message::ToggleArrangementLoop)
                .padding([8, 12])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::BG_ELEVATED.into()),
                    text_color: th::TEXT_DIM,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        let transport_buttons = row![skip_back_btn, play_pause_btn, loop_btn].spacing(4);

        // Time display
        let time_text = text(format!(
            "{} / {}",
            AppState::format_time(self.state.position_seconds()),
            AppState::format_time(self.state.duration_seconds()),
        ))
        .size(14)
        .color(th::TEXT);

        // BPM
        let bpm_input = text_input("BPM", &self.state.bpm_text)
            .on_input(Message::BpmChanged)
            .on_submit(Message::BpmSubmit)
            .width(Length::Fixed(55.0))
            .size(14);

        let bpm_label = text("BPM").size(12).color(th::TEXT_DIM);

        // Master VU meter
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(28.0))
            .into();

        let volume_icon = icons::icon(icons::VOLUME_2).size(14).color(th::TEXT_DIM);

        let transport = row![
            transport_buttons,
            horizontal_space(),
            time_text,
            horizontal_space(),
            volume_icon,
            master_meter_canvas,
            bpm_input,
            bpm_label,
        ]
        .spacing(12)
        .padding(10)
        .align_y(iced::Alignment::Center);

        container(transport)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    fn view_status(&self) -> Element<'_, Message> {
        let status = text(&self.state.status_text).size(11).color(th::TEXT_DIM);

        container(status)
            .width(Length::Fill)
            .padding([3, 12])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_DARK.into()),
                ..Default::default()
            })
            .into()
    }

    fn theme(&self) -> Theme {
        th::vibez_theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(std::time::Duration::from_millis(UI_TICK_MS)).map(|_| Message::Tick),
            iced::keyboard::on_key_press(global_key_handler),
            iced::event::listen_with(|event, _status, _id| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorMoved(position.x, position.y))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::MouseReleased),
                _ => None,
            }),
        ])
    }
}

/// Phase 1 of plugin loading (runs on background thread).
/// For CLAP: only loads the DSO — NO CLAP API calls (not even create_plugin).
/// For VST3: fully loads (VST3 doesn't have JUCE MessageManager issues).
fn load_plugin_effect_bg(
    info: &PluginInfo,
    sample_rate: f64,
) -> Result<PluginLoadResult, String> {
    match info.format {
        PluginFormat::Clap => {
            let partial = vibez_plugin_host::clap_host::instance::ClapPluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                false,
            )?;
            Ok(PluginLoadResult {
                track_id: TrackId::default(), // filled in by caller
                effect_id: EffectId::new(),
                plugin_name: info.name.clone(),
                effect: None,
                gui_raw_ptr: None,
                clap_partial: Some(partial),
                sample_rate,
            })
        }
        PluginFormat::Vst3 => {
            let vst3_inst = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load(
                &info.path,
                &info.id.uid,
                false,
                sample_rate,
                4096,
            )?;
            let gui_raw_ptr = Some(PluginRawPtr::Vst3(vst3_inst.component_ptr()));
            let name = vst3_inst.name().to_string();
            let wrapper = vibez_plugin_host::PluginEffectWrapper::new(Box::new(vst3_inst));
            Ok(PluginLoadResult {
                track_id: TrackId::default(),
                effect_id: EffectId::new(),
                plugin_name: name,
                effect: Some(Box::new(wrapper)),
                gui_raw_ptr,
                clap_partial: None,
                sample_rate,
            })
        }
    }
}

/// Phase 1 of instrument loading (runs on background thread).
fn load_plugin_instrument_bg(
    info: &PluginInfo,
    sample_rate: f64,
) -> Result<PluginInstrumentLoadResult, String> {
    match info.format {
        PluginFormat::Clap => {
            let partial = vibez_plugin_host::clap_host::instance::ClapPluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                true,
            )?;
            Ok(PluginInstrumentLoadResult {
                track_id: TrackId::default(),
                plugin_name: info.name.clone(),
                instrument: None,
                gui_raw_ptr: None,
                clap_partial: Some(partial),
                sample_rate,
            })
        }
        PluginFormat::Vst3 => {
            let vst3_inst = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load(
                &info.path,
                &info.id.uid,
                true,
                sample_rate,
                4096,
            )?;
            let gui_raw_ptr = Some(PluginRawPtr::Vst3(vst3_inst.component_ptr()));
            let name = vst3_inst.name().to_string();
            let wrapper =
                vibez_plugin_host::PluginInstrumentWrapper::new(Box::new(vst3_inst));
            Ok(PluginInstrumentLoadResult {
                track_id: TrackId::default(),
                plugin_name: name,
                instrument: Some(Box::new(wrapper)),
                gui_raw_ptr,
                clap_partial: None,
                sample_rate,
            })
        }
    }
}

fn global_key_handler(
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    use iced::keyboard::key::Named;

    // Space: toggle playback (no modifiers required)
    if matches!(key, iced::keyboard::Key::Named(Named::Space)) {
        return Some(Message::TogglePlayback);
    }

    // Escape: cancel editing
    if matches!(key, iced::keyboard::Key::Named(Named::Escape)) {
        return Some(Message::CancelEditing);
    }

    // B: toggle piano roll draw mode (no modifiers)
    if !modifiers.control()
        && !modifiers.shift()
        && matches!(key, iced::keyboard::Key::Character(ref c) if c.as_str() == "b")
    {
        return Some(Message::TogglePianoRollEditMode);
    }

    if !modifiers.control() {
        return None;
    }
    match key {
        iced::keyboard::Key::Named(Named::ArrowUp) => Some(Message::MoveSelectedTrackUp),
        iced::keyboard::Key::Named(Named::ArrowDown) => Some(Message::MoveSelectedTrackDown),
        iced::keyboard::Key::Character(ref c) => match c.as_str() {
            "t" | "T" => {
                if modifiers.shift() {
                    Some(Message::AddInstrumentTrack)
                } else {
                    Some(Message::AddTrack)
                }
            }
            "m" => Some(Message::CreateClipFromSelection),
            "e" => Some(Message::SplitSelectedAtPlayhead),
            "j" => Some(Message::JoinSelectedClips),
            "l" => Some(Message::ToggleArrangementLoop),
            "0" => Some(Message::ZoomToFit),
            _ => None,
        },
        _ => None,
    }
}

async fn decode_file_async(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

async fn save_project_async(path: PathBuf, project: Project) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        let save_path = path;
        project
            .save_to_file(&save_path)
            .map(|_| save_path)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("save task failed: {err}"))?
}

async fn load_project_async(path: PathBuf) -> Result<ProjectLoadResult, String> {
    tokio::task::spawn_blocking(move || {
        let project = Project::load_from_file(&path).map_err(|err| err.to_string())?;
        let mut clips = Vec::new();
        let mut sampler_samples = Vec::new();
        let mut warnings = Vec::new();

        for clip in &project.clips {
            match clip.resolved_source().cloned() {
                Some(MediaSourceRef::LocalFile { path: clip_path }) => {
                    match file_io::decode_audio_file(&clip_path) {
                        Ok(audio) => clips.push(LoadedClipData {
                            info: clip.clone(),
                            audio: Arc::new(audio),
                        }),
                        Err(err) => warnings.push(format!(
                            "Skipped clip '{}' ({})",
                            clip.name, err
                        )),
                    }
                }
                Some(MediaSourceRef::DropboxFile { .. }) => warnings.push(format!(
                    "Skipped clip '{}' (Dropbox sources are not available yet)",
                    clip.name
                )),
                None => warnings.push(format!(
                    "Skipped clip '{}' (missing source reference)",
                    clip.name
                )),
            }
        }

        for track in &project.tracks {
            if let Some(InstrumentStateInfo::Sampler {
                source: Some(source),
                ..
            }) = &track.native_instrument
            {
                match source {
                    MediaSourceRef::LocalFile { path: sample_path } => {
                        match file_io::decode_audio_file(sample_path) {
                            Ok(audio) => sampler_samples.push(LoadedSamplerData {
                                track_id: track.id,
                                source: source.clone(),
                                audio: Arc::new(audio),
                                name: source.display_name(),
                            }),
                            Err(err) => warnings.push(format!(
                                "Skipped sampler source on '{}' ({})",
                                track.name, err
                            )),
                        }
                    }
                    MediaSourceRef::DropboxFile { .. } => warnings.push(format!(
                        "Skipped sampler source on '{}' (Dropbox sources are not available yet)",
                        track.name
                    )),
                }
            }
        }

        Ok(ProjectLoadResult {
            path,
            project,
            clips,
            sampler_samples,
            warnings,
        })
    })
    .await
    .map_err(|err| format!("load task failed: {err}"))?
}

fn default_instrument_params(kind: InstrumentKind, sample_rate: f32) -> Vec<f32> {
    let instrument = vibez_instruments::create_instrument(kind, sample_rate);
    instrument
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.default)
        .collect()
}
