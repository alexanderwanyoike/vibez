use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, mouse_area, row, scrollable,
    slider, stack, text, text_input, vertical_space,
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
use vibez_dropbox::{
    load_app_key_with_env_override, DropboxCache, DropboxClient, DropboxEntry, DropboxSettings,
};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;
use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::{PluginCategory, PluginFormat, PluginInfo};
use vibez_project::Project;

use crate::icons;
use crate::message::{
    BrowserImportTarget, DrumPadParam, LoadedClipData, LoadedDrumRackPadData, LoadedSamplerData,
    Message, ProjectLoadResult, SampleLibraryScanResult,
};
use crate::plugin_window::{PluginRawPtr, PluginWindowEvent, PluginWindowManager};
use crate::state::{
    AppState, ArrangementSelection, ContextMenuTarget, DetailPanelTab, SampleBrowserEntry,
    SettingsTab, UiClip, UiDrumPad, UiEffect, UiNoteClip, UiTrack, Workspace,
};
use crate::theme as th;
use crate::ui_settings::UiSettings;
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
    /// VST3 two-phase: dlopen'd module to be instantiated on the UI
    /// thread (JUCE MessageManager binds to the instantiating thread).
    vst3_partial: Option<vibez_plugin_host::vst3_host::instance::PartialVst3Plugin>,
    sample_rate: f64,
    /// Persistent identity for project save.
    device_ref: vibez_core::effect::PluginDeviceInfo,
    /// Pointer for live state capture at save time.
    state_ptr: Option<vibez_plugin_host::PluginStatePtr>,
    /// Saved state to restore (project reload), applied on the UI
    /// thread after phase-2 init.
    pending_state: Option<Vec<u8>>,
    /// Chain position to restore (project reload).
    position: Option<usize>,
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
    /// VST3 two-phase: dlopen'd module to be instantiated on the UI thread.
    vst3_partial: Option<vibez_plugin_host::vst3_host::instance::PartialVst3Plugin>,
    sample_rate: f64,
    /// Persistent identity for project save.
    device_ref: vibez_core::effect::PluginDeviceInfo,
    /// Pointer for live state capture at save time.
    state_ptr: Option<vibez_plugin_host::PluginStatePtr>,
    /// Saved state to restore (project reload).
    pending_state: Option<Vec<u8>>,
}

struct BounceAssets {
    clips: std::collections::HashMap<ClipId, Arc<vibez_core::audio_buffer::DecodedAudio>>,
    samplers:
        std::collections::HashMap<TrackId, (Arc<vibez_core::audio_buffer::DecodedAudio>, String)>,
    pads: std::collections::HashMap<
        (TrackId, usize),
        (Arc<vibez_core::audio_buffer::DecodedAudio>, String),
    >,
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
    /// Raw pointers for live state capture at save time, keyed like
    /// the GUI pointers. Entries live exactly as long as the device.
    plugin_state_ptrs: std::collections::HashMap<PluginGuiKey, vibez_plugin_host::PluginStatePtr>,

    // Dropbox
    dropbox_settings: DropboxSettings,
    dropbox_cache: DropboxCache,
    dropbox_client: Option<Arc<DropboxClient>>,

    // External MIDI input (USB keyboard, Ableton Push, virtual cable...).
    // Dropping the handle closes the port.
    midi_input: Option<vibez_audio_io::midi_input::MidiInputHandle>,
    /// Cached list of port names last seen by `list_midi_input_ports`.
    /// Populated when the user opens the MIDI picker; used so the UI
    /// can show a dropdown without re-scanning on every frame.
    midi_input_ports: Vec<String>,

    // Undo / redo
    undo_history: crate::state::UndoHistory,
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
        let ui_settings = UiSettings::load();

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

        let dropbox_settings = DropboxSettings::load();
        let dropbox_cache = DropboxCache::new();
        let resolved_key = load_app_key_with_env_override(&dropbox_settings);
        let dropbox_client = match (&resolved_key, &dropbox_settings.tokens) {
            (Some(key), Some(tokens)) => {
                Some(Arc::new(DropboxClient::new(key.clone(), tokens.clone())))
            }
            _ => None,
        };
        let dropbox_ui_state = crate::state::DropboxUiState {
            connected: dropbox_client.is_some(),
            account_email: dropbox_settings.account_email.clone(),
            app_key_input: dropbox_settings.app_key.clone().unwrap_or_default(),
            has_app_key: resolved_key.is_some(),
            ..Default::default()
        };

        let state = AppState {
            sample_rate,
            sample_browser_open: ui_settings.sample_browser_open,
            sample_browser_roots: ui_settings.sample_library_roots,
            auto_warp_on_import: ui_settings.auto_warp_on_import,
            warp_confidence_threshold: ui_settings.warp_confidence_threshold,
            dropbox: dropbox_ui_state,
            ..Default::default()
        };

        let (plugin_effect_tx, plugin_effect_rx) = std::sync::mpsc::channel();
        let (plugin_instrument_tx, plugin_instrument_rx) = std::sync::mpsc::channel();

        // Register the UI thread (process main thread) as the CLAP "main thread".
        // JUCE-based CLAP plugins require GUI calls on this thread.
        vibez_plugin_host::set_clap_main_thread();

        let plugin_window_manager = PluginWindowManager::new();

        // Auto-connect to the preferred MIDI input if the saved name
        // matches one currently visible, else to the first available
        // port. Silent on failure: the user can still work without
        // MIDI, and Settings → Audio lets them pick manually later.
        let midi_input = auto_open_midi_input(ui_settings.preferred_midi_input.as_deref());

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
            plugin_state_ptrs: std::collections::HashMap::new(),
            dropbox_settings,
            dropbox_cache,
            dropbox_client,
            midi_input,
            midi_input_ports: Vec::new(),
            undo_history: crate::state::UndoHistory::default(),
        };

        // Inform the engine of the actual sample rate
        app.send_command(EngineCommand::SetBpm(app.state.bpm));

        let startup_task = if app.state.sample_browser_roots.is_empty() {
            Task::none()
        } else {
            app.state.sample_browser_scan_in_progress = true;
            Task::perform(
                scan_sample_library_async(app.state.sample_browser_roots.clone()),
                Message::SampleLibraryScanned,
            )
        };

        (app, startup_task)
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
        // The engine drops all plugin instances with their tracks;
        // stale raw pointers must go with them.
        self.plugin_gui_raw_ptrs.clear();
        self.plugin_state_ptrs.clear();
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
        self.undo_history.clear();
        self.state.status_text = "New project".to_string();
    }

    fn persist_ui_settings(&mut self) {
        let settings = UiSettings {
            sample_library_roots: self.state.sample_browser_roots.clone(),
            sample_browser_open: self.state.sample_browser_open,
            auto_warp_on_import: self.state.auto_warp_on_import,
            warp_confidence_threshold: self.state.warp_confidence_threshold,
            preferred_midi_input: self.midi_input.as_ref().map(|h| h.port_name.clone()),
        };
        if let Err(err) = settings.save() {
            self.state.status_text = format!("UI settings save error: {err}");
        }
    }

    fn selected_sample_browser_entry(&self) -> Option<&SampleBrowserEntry> {
        let selected = self.state.sample_browser_selected_source.as_ref()?;
        self.state
            .sample_browser_entries
            .iter()
            .find(|entry| &entry.source == selected)
    }

    fn selected_browser_device_target(&self) -> Option<BrowserImportTarget> {
        let track = self
            .state
            .selected_track
            .and_then(|track_id| self.state.find_track(track_id))?;
        match track.instrument_kind {
            Some(InstrumentKind::Sampler) => Some(BrowserImportTarget::Sampler(track.id)),
            Some(InstrumentKind::DrumRack) => Some(BrowserImportTarget::DrumRackPad {
                track_id: track.id,
                pad_index: track
                    .selected_drum_pad
                    .min(track.drum_rack_pads.len().saturating_sub(1)),
            }),
            _ => None,
        }
    }

    fn sync_drum_rack_pad_state(&mut self, track_id: TrackId, pad_index: usize) {
        let state = self
            .state
            .find_track(track_id)
            .and_then(|track| track.drum_rack_pads.get(pad_index))
            .map(UiDrumPad::to_state);
        if let Some(state) = state {
            self.send_command(EngineCommand::SetDrumRackPadState {
                track_id,
                pad_index,
                state,
            });
        }
    }

    fn apply_sampler_sample_loaded(
        &mut self,
        track_id: TrackId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) {
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.sample_name = Some(name.clone());
            track.sample_source = Some(source.clone());
            track.sample_audio = Some(Arc::clone(&audio));
        }
        self.send_command(EngineCommand::LoadSamplerSample {
            track_id,
            sample: audio,
            sample_name: name.clone(),
        });
        self.state.status_text = format!("Loaded sample: {name}");
    }

    fn apply_drum_rack_pad_loaded(
        &mut self,
        track_id: TrackId,
        pad_index: usize,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) {
        if let Some(track) = self.state.find_track_mut(track_id) {
            if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                pad.name = Some(name.clone());
                pad.source = Some(source.clone());
                pad.audio = Some(Arc::clone(&audio));
            }
        }
        self.sync_drum_rack_pad_state(track_id, pad_index);
        self.send_command(EngineCommand::LoadDrumRackPadSample {
            track_id,
            pad_index,
            sample: audio,
            sample_name: name.clone(),
        });
        self.state.status_text = format!("Loaded pad {}: {name}", pad_index + 1);
    }

    /// Walk `next_track_number` forward past any names already in use so
    /// that `format!("{prefix} {n}")` is unique. Prevents e.g. two lanes
    /// both named "Track 2" when numbering gets out of sync after loads,
    /// deletes, or undo chains.
    fn next_unique_track_number(&mut self, prefix: &str) -> u32 {
        loop {
            let candidate = self.state.next_track_number;
            let name = format!("{prefix} {candidate}");
            let clash = self.state.tracks.iter().any(|t| t.name == name);
            if !clash {
                return candidate;
            }
            self.state.next_track_number += 1;
        }
    }

    fn ensure_audio_track_for_import(&mut self, preferred: Option<TrackId>) -> TrackId {
        if let Some(track_id) = preferred {
            if self
                .state
                .find_track(track_id)
                .is_some_and(|track| matches!(track.kind, TrackKind::Audio))
            {
                return track_id;
            }
        }

        let track_num = self.next_unique_track_number("Audio");
        self.state.next_track_number = track_num + 1;
        let id = TrackId::new();
        let color_index = ((track_num - 1) % 8) as u8;
        let name = format!("Audio {track_num}");

        self.send_command(EngineCommand::AddTrack(id, name.clone()));
        self.state.tracks.push(UiTrack::new(id, name, color_index));
        self.state.selected_track = Some(id);
        id
    }

    fn add_audio_clip_to_track(
        &mut self,
        track_id: TrackId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        let clip_id = ClipId::new();
        let existing_end = self
            .state
            .find_track(track_id)
            .map(|track| {
                track
                    .clips
                    .iter()
                    .map(|clip| clip.position.saturating_add(clip.duration))
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
                audio: Arc::clone(&audio),
                source: Some(source),
                position: existing_end,
                source_offset: 0,
                duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }

        self.state.selected_track = Some(track_id);
        self.state.status_text = format!("Added clip: {name}");
        self.schedule_auto_warp_if_enabled(track_id, clip_id, audio)
    }

    fn apply_browser_sample_decoded(
        &mut self,
        target: BrowserImportTarget,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        match target {
            BrowserImportTarget::ArrangementClip(preferred_track) => {
                let track_id = self.ensure_audio_track_for_import(preferred_track);
                self.add_audio_clip_to_track(track_id, audio, name, source)
            }
            BrowserImportTarget::ArrangementClipAt {
                track_id,
                position_samples,
            } => self.add_audio_clip_to_track_at(track_id, position_samples, audio, name, source),
            BrowserImportTarget::Sampler(track_id) => {
                self.apply_sampler_sample_loaded(track_id, audio, name, source);
                Task::none()
            }
            BrowserImportTarget::DrumRackPad {
                track_id,
                pad_index,
            } => {
                self.apply_drum_rack_pad_loaded(track_id, pad_index, audio, name, source);
                Task::none()
            }
        }
    }

    fn add_audio_clip_to_track_at(
        &mut self,
        track_id: TrackId,
        position_samples: u64,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        // Guard: if the target is not an audio track, refuse rather than
        // silently redirecting the drop. Prevents the "clip lands on the
        // wrong lane" surprise.
        let track_name = match self.state.find_track(track_id) {
            Some(t) if matches!(t.kind, TrackKind::Audio) => t.name.clone(),
            Some(t) => {
                self.state.status_text = format!(
                    "Can't drop audio on non-audio track '{}'; drag to an audio lane.",
                    t.name
                );
                return Task::none();
            }
            None => {
                self.state.status_text = "Drop target not found; drag cancelled".to_string();
                return Task::none();
            }
        };

        let clip_id = ClipId::new();
        let duration = audio.num_frames() as u64;

        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id,
            audio: Arc::clone(&audio),
            position: position_samples,
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
                audio: Arc::clone(&audio),
                source: Some(source),
                position: position_samples,
                source_offset: 0,
                duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }
        self.state.selected_track = Some(track_id);
        self.state.status_text = format!("Dropped '{name}' on {track_name}");
        self.schedule_auto_warp_if_enabled(track_id, clip_id, audio)
    }

    fn dispatch_drop_on_arrangement(
        &mut self,
        track_id: TrackId,
        position_samples: u64,
        source: MediaSourceRef,
    ) -> Task<Message> {
        let target = BrowserImportTarget::ArrangementClipAt {
            track_id,
            position_samples,
        };
        self.dispatch_drop_for_target(source, target)
    }

    fn dispatch_drop_for_target(
        &mut self,
        source: MediaSourceRef,
        target: BrowserImportTarget,
    ) -> Task<Message> {
        match source {
            MediaSourceRef::LocalFile { path } => {
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let ret_source = MediaSourceRef::LocalFile { path: path.clone() };
                self.state.status_text = format!("Dropping {name}...");
                Task::perform(decode_file_async(path), move |result| match result {
                    Ok(audio) => Message::BrowserSampleDecoded(
                        target.clone(),
                        Arc::new(audio),
                        name.clone(),
                        ret_source.clone(),
                    ),
                    Err(err) => Message::BrowserSampleDecodeError(err),
                })
            }
            MediaSourceRef::DropboxFile {
                path_lower,
                display_path,
                rev,
            } => {
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.status_text = "Not connected to Dropbox for this drop".to_string();
                    return Task::none();
                };
                let cache = self.dropbox_cache.clone();
                let name = display_path
                    .rsplit_once('/')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| display_path.clone());
                let entry = DropboxEntry {
                    path_lower,
                    path_display: display_path,
                    name: name.clone(),
                    is_folder: false,
                    rev,
                    size: None,
                };
                self.state.status_text = format!("Dropping {name}...");
                Task::perform(
                    fetch_dropbox_sample_async(client, cache, entry),
                    move |result| match result {
                        Ok((audio, decoded_name, source)) => Message::BrowserSampleDecoded(
                            target.clone(),
                            audio,
                            decoded_name,
                            source,
                        ),
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                )
            }
        }
    }

    fn track_info_from_ui(&self, track: &UiTrack) -> TrackInfo {
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
        }
    }

    /// Base64-encoded live state of a plugin device, captured on the
    /// UI thread via the pointer stashed at load time.
    fn capture_device_state(&self, key: PluginGuiKey) -> Option<String> {
        use base64::Engine;
        let ptr = self.plugin_state_ptrs.get(&key)?;
        let data = unsafe { vibez_plugin_host::capture_plugin_state(ptr) }?;
        Some(base64::engine::general_purpose::STANDARD.encode(data))
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
                    original_bpm: clip.original_bpm,
                    warped: clip.warped,
                    warped_to_bpm: clip.warped_to_bpm,
                })
            })
            .collect();

        let note_clips = self
            .state
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
        self.undo_history.clear();
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

            self.state.next_track_number = self
                .state
                .next_track_number
                .max(self.state.tracks.len() as u32 + 1);
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

        self.spawn_project_plugin_loads(plugin_effect_requests, plugin_instrument_requests);
    }

    /// Reload persisted plugin devices on one background thread, in
    /// file order, so results arrive in order and chain positions
    /// restore deterministically. Results flow through the same
    /// channels as interactive plugin loads.
    fn spawn_project_plugin_loads(
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

        let effect_tx = self.plugin_effect_tx.clone();
        let instrument_tx = self.plugin_instrument_tx.clone();
        let sample_rate = self.state.sample_rate as f64;

        std::thread::spawn(move || {
            use base64::Engine;
            let decode = |dev: &vibez_core::effect::PluginDeviceInfo| {
                dev.state_b64.as_ref().and_then(|b64| {
                    base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .map_err(|e| {
                            eprintln!("vibez: bad plugin state blob for {}: {e}", dev.name)
                        })
                        .ok()
                })
            };
            let scan_info =
                |dev: &vibez_core::effect::PluginDeviceInfo,
                 category: vibez_plugin_host::PluginCategory| {
                    let format = match dev.format.as_str() {
                        "clap" => PluginFormat::Clap,
                        _ => PluginFormat::Vst3,
                    };
                    PluginInfo {
                        id: vibez_plugin_host::PluginId {
                            format,
                            uid: dev.uid.clone(),
                        },
                        name: dev.name.clone(),
                        vendor: String::new(),
                        category,
                        format,
                        path: dev.path.clone(),
                    }
                };

            for (track_id, effect_id, chain_pos, dev) in effect_requests {
                let info = scan_info(&dev, vibez_plugin_host::PluginCategory::Effect);
                match load_plugin_effect_bg(&info, sample_rate, decode(&dev)) {
                    Ok(mut result) => {
                        result.track_id = track_id;
                        result.effect_id = effect_id;
                        result.position = Some(chain_pos);
                        let _ = effect_tx.send(result);
                    }
                    Err(e) => {
                        eprintln!("vibez: failed to reload plugin {}: {e}", dev.name);
                    }
                }
            }
            for (track_id, dev) in instrument_requests {
                let info = scan_info(&dev, vibez_plugin_host::PluginCategory::Instrument);
                match load_plugin_instrument_bg(&info, sample_rate, decode(&dev)) {
                    Ok(mut result) => {
                        result.track_id = track_id;
                        let _ = instrument_tx.send(result);
                    }
                    Err(e) => {
                        eprintln!("vibez: failed to reload plugin {}: {e}", dev.name);
                    }
                }
            }
        });
    }

    fn collect_bounce_assets(&self) -> BounceAssets {
        let mut clips = std::collections::HashMap::new();
        let mut samplers = std::collections::HashMap::new();
        let mut pads = std::collections::HashMap::new();
        for track in &self.state.tracks {
            for clip in &track.clips {
                clips.insert(clip.id, Arc::clone(&clip.audio));
            }
            if let Some(audio) = &track.sample_audio {
                samplers.insert(
                    track.id,
                    (
                        Arc::clone(audio),
                        track.sample_name.clone().unwrap_or_default(),
                    ),
                );
            }
            for (i, pad) in track.drum_rack_pads.iter().enumerate() {
                if let Some(audio) = &pad.audio {
                    pads.insert(
                        (track.id, i),
                        (Arc::clone(audio), pad.name.clone().unwrap_or_default()),
                    );
                }
            }
        }
        BounceAssets {
            clips,
            samplers,
            pads,
        }
    }

    fn next_bounce_path(&self) -> PathBuf {
        let base = match &self.state.current_project_path {
            Some(project_path) => project_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("renders"),
            None => std::env::temp_dir().join("vibez-renders"),
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        base.join(format!("bounce-{stamp}.wav"))
    }

    fn dispatch_bounce(
        &mut self,
        mode: vibez_engine::render::BounceMode,
        range_samples: (u64, u64),
        insert_position_samples: u64,
        clip_name: String,
    ) -> Task<Message> {
        if range_samples.1 <= range_samples.0 {
            self.state.status_text = "Empty range, nothing to bounce".to_string();
            return Task::none();
        }

        let assets = self.collect_bounce_assets();
        let project = self.project_from_state();
        let wav_path = self.next_bounce_path();
        let sample_rate = self.state.sample_rate;
        let bpm = self.state.bpm;

        let request = vibez_engine::render::BounceRequest {
            tracks: project.tracks,
            audio_clips: project.clips,
            note_clips: project.note_clips,
            clip_audio: assets.clips,
            sampler_audio: assets.samplers,
            drum_pad_audio: assets.pads,
            mode,
            range_samples,
            bpm,
            sample_rate,
        };

        self.state.status_text = format!("Bouncing {clip_name}...");
        Task::perform(
            bounce_async(request, wav_path, clip_name, insert_position_samples),
            Message::BounceComplete,
        )
    }

    fn finalize_bounce(&mut self, outcome: crate::message::BounceOutcome) {
        let track_num = self.next_unique_track_number("Bounce");
        self.state.next_track_number = track_num + 1;
        let color_index = (track_num.wrapping_sub(1) % 8) as u8;
        let track_id = TrackId::new();
        let track_name = format!("Bounce {track_num}");

        self.send_command(EngineCommand::AddTrack(track_id, track_name.clone()));
        self.state
            .tracks
            .push(UiTrack::new(track_id, track_name, color_index));

        let clip_id = ClipId::new();
        let duration = outcome.audio.num_frames() as u64;
        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id,
            audio: Arc::clone(&outcome.audio),
            position: outcome.insert_position_samples,
            source_offset: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });

        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: clip_id,
                name: outcome.clip_name.clone(),
                audio: Arc::clone(&outcome.audio),
                source: Some(outcome.source.clone()),
                position: outcome.insert_position_samples,
                source_offset: 0,
                duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }

        self.state.selected_track = Some(track_id);
        self.state.selected_clips.clear();
        self.state
            .selected_clips
            .insert(ArrangementSelection::AudioClip { track_id, clip_id });
        self.mark_project_dirty();

        let warnings_note = if outcome.warnings.is_empty() {
            String::new()
        } else {
            format!(" ({} warning(s))", outcome.warnings.len())
        };
        self.state.status_text = format!(
            "Bounced '{}' to {}{}",
            outcome.clip_name,
            outcome.path.display(),
            warnings_note
        );
    }

    fn take_snapshot(&self) -> crate::state::ProjectSnapshot {
        crate::state::ProjectSnapshot {
            tracks: self.state.tracks.clone(),
            bpm: self.state.bpm,
            bpm_text: self.state.bpm_text.clone(),
            loop_enabled: self.state.loop_enabled,
            loop_start_beats: self.state.loop_start_beats,
            loop_end_beats: self.state.loop_end_beats,
            selected_track: self.state.selected_track,
            selected_clips: self.state.selected_clips.clone(),
            selected_note_clip: self.state.selected_note_clip,
            next_track_number: self.state.next_track_number,
        }
    }

    fn push_undo_snapshot(&mut self) {
        let snapshot = self.take_snapshot();
        self.undo_history.push_undo(snapshot);
    }

    fn apply_snapshot(&mut self, snapshot: crate::state::ProjectSnapshot) {
        // Tear down the engine side.
        let existing_track_ids: Vec<TrackId> = self.state.tracks.iter().map(|t| t.id).collect();
        for track_id in existing_track_ids {
            self.send_command(EngineCommand::RemoveTrack(track_id));
        }

        self.state.tracks = snapshot.tracks;
        self.state.bpm = snapshot.bpm;
        self.state.bpm_text = snapshot.bpm_text;
        self.state.loop_enabled = snapshot.loop_enabled;
        self.state.loop_start_beats = snapshot.loop_start_beats;
        self.state.loop_end_beats = snapshot.loop_end_beats;
        self.state.selected_track = snapshot.selected_track;
        self.state.selected_clips = snapshot.selected_clips;
        self.state.selected_note_clip = snapshot.selected_note_clip;
        self.state.next_track_number = snapshot.next_track_number;

        self.send_command(EngineCommand::SetBpm(self.state.bpm));
        self.send_command(EngineCommand::SetArrangementLoop(self.state.loop_enabled));
        if self.state.loop_enabled {
            let start = self.state.beats_to_samples(self.state.loop_start_beats);
            let end = self.state.beats_to_samples(self.state.loop_end_beats);
            self.send_command(EngineCommand::SetArrangementLoopRegion { start, end });
        }

        let tracks = self.state.tracks.clone();
        for track in &tracks {
            self.replay_track_to_engine(track);
        }
    }

    fn replay_track_to_engine(&mut self, track: &UiTrack) {
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

    fn undo(&mut self) {
        let Some(snapshot) = self.undo_history.pop_undo() else {
            self.state.status_text = "Nothing to undo".to_string();
            return;
        };
        let redo = self.take_snapshot();
        self.undo_history.push_redo(redo);
        self.apply_snapshot(snapshot);
        self.state.status_text = "Undo".to_string();
    }

    fn redo(&mut self) {
        let Some(snapshot) = self.undo_history.pop_redo() else {
            self.state.status_text = "Nothing to redo".to_string();
            return;
        };
        let undo = self.take_snapshot();
        self.undo_history.push_undo(undo);
        self.apply_snapshot(snapshot);
        self.state.status_text = "Redo".to_string();
    }

    fn dispatch_audio_quantize(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        grid: crate::state::SnapGrid,
    ) -> Task<Message> {
        let Some(track) = self.state.find_track(track_id) else {
            self.state.status_text = "Track not found".to_string();
            return Task::none();
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        if self.state.bpm <= 0.0 || self.state.sample_rate == 0 {
            self.state.status_text = "Cannot quantize at zero BPM".to_string();
            return Task::none();
        }

        let input = QuantizeInput {
            audio: Arc::clone(&clip.audio),
            bpm: self.state.bpm,
            sample_rate: self.state.sample_rate,
            grid,
            clip_position: clip.position,
            clip_source_offset: clip.source_offset,
            clip_duration: clip.duration,
            original_name: clip.name.clone(),
            new_clip_id: ClipId::new(),
        };

        self.state.status_text = format!("Quantizing {}...", input.original_name);
        Task::perform(quantize_audio_clip_async(input), move |result| {
            Message::AudioQuantizeReady {
                track_id,
                old_clip_id: clip_id,
                result,
            }
        })
    }

    fn apply_audio_quantize_success(
        &mut self,
        track_id: TrackId,
        old_clip_id: ClipId,
        success: crate::message::AudioQuantizeSuccess,
    ) {
        self.send_command(EngineCommand::RemoveClip(track_id, old_clip_id));
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.retain(|c| c.id != old_clip_id);
        }
        self.state.selected_clips.retain(|sel| match sel {
            ArrangementSelection::AudioClip {
                clip_id: cid,
                track_id: tid,
            } => !(*tid == track_id && *cid == old_clip_id),
            _ => true,
        });

        self.send_command(EngineCommand::AddClip {
            track_id,
            clip_id: success.new_clip_id,
            audio: Arc::clone(&success.new_audio),
            position: success.new_position,
            source_offset: 0,
            duration: success.new_duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        });
        if let Some(track) = self.state.find_track_mut(track_id) {
            track.clips.push(UiClip {
                id: success.new_clip_id,
                name: success.new_name,
                audio: Arc::clone(&success.new_audio),
                source: None,
                position: success.new_position,
                source_offset: 0,
                duration: success.new_duration,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        }
        self.state
            .selected_clips
            .insert(ArrangementSelection::AudioClip {
                track_id,
                clip_id: success.new_clip_id,
            });

        let duration_seconds = success.new_duration as f64 / self.state.sample_rate as f64;
        self.state.status_text = format!(
            "Quantized {} slice(s) to {} ({:.1}s)",
            success.slice_count, success.grid_label, duration_seconds
        );
    }

    fn dispatch_detect_clip_bpm(&mut self, track_id: TrackId, clip_id: ClipId) -> Task<Message> {
        let Some(track) = self.state.find_track(track_id) else {
            self.state.status_text = "Track not found".to_string();
            return Task::none();
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        // Always detect against the un-warped audio so the result is
        // the sample's intrinsic tempo, not the warped-to-project tempo.
        let audio = clip
            .original_audio
            .clone()
            .unwrap_or_else(|| Arc::clone(&clip.audio));
        let sample_rate = self.state.sample_rate;
        self.state.status_text = format!("Detecting BPM for {}...", clip.name);
        Task::perform(detect_clip_bpm_async(audio, sample_rate), move |estimate| {
            Message::ClipBpmDetected {
                track_id,
                clip_id,
                bpm: estimate.map(|e| e.bpm),
                confidence: estimate.map(|e| e.confidence).unwrap_or(0.0),
            }
        })
    }

    fn dispatch_warp_clip_to_project(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Task<Message> {
        let project_bpm = self.state.bpm;
        let sample_rate = self.state.sample_rate;
        if project_bpm <= 0.0 || sample_rate == 0 {
            self.state.status_text = "Cannot warp at zero BPM".to_string();
            return Task::none();
        }
        let Some(track) = self.state.find_track(track_id) else {
            self.state.status_text = "Track not found".to_string();
            return Task::none();
        };
        let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) else {
            self.state.status_text = "Clip not found".to_string();
            return Task::none();
        };
        let Some(clip_bpm) = clip.original_bpm else {
            self.state.status_text = "Set or detect the clip's BPM before warping".to_string();
            return Task::none();
        };
        if clip_bpm <= 0.0 {
            self.state.status_text = "Invalid BPM".to_string();
            return Task::none();
        }
        // If the clip was already warped once, warp the retained
        // original_audio. Otherwise the current `audio` is itself the
        // original. Either way the clip's geometry fields are in
        // samples of the CURRENT buffer, so `fields_frames` must be
        // the current buffer's frame count for the rescale to be
        // idempotent across repeated warps.
        let source_audio = clip
            .original_audio
            .clone()
            .unwrap_or_else(|| Arc::clone(&clip.audio));
        let input = crate::warp::WarpClipInput {
            audio: source_audio,
            fields_frames: clip.audio.num_frames() as u64,
            source_offset: clip.source_offset,
            duration: clip.duration,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
            clip_bpm,
            project_bpm,
        };
        let _ = sample_rate;
        self.state.status_text = format!("Warping to {project_bpm:.0} BPM...");
        Task::perform(crate::warp::warp_clip_async(input), move |result| {
            Message::ClipWarpReady {
                track_id,
                clip_id,
                result,
            }
        })
    }

    fn apply_clip_warp_success(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        success: crate::message::ClipWarpSuccess,
    ) {
        self.send_command(EngineCommand::ReplaceClipAudio {
            track_id,
            clip_id,
            audio: Arc::clone(&success.audio),
            duration: success.new_duration,
            source_offset: success.new_source_offset,
            loop_start: success.new_loop_start,
            loop_end: success.new_loop_end,
        });
        if let Some(track) = self.state.find_track_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                clip.audio = Arc::clone(&success.audio);
                clip.duration = success.new_duration;
                clip.source_offset = success.new_source_offset;
                clip.loop_start = success.new_loop_start;
                clip.loop_end = success.new_loop_end;
                clip.original_bpm = Some(success.detected_bpm);
                clip.warped = true;
                clip.warped_to_bpm = Some(success.warped_to_bpm);
                clip.original_audio = Some(Arc::clone(&success.original_audio));
            }
        }
        self.state.status_text = format!("Warped to {:.0} BPM", success.warped_to_bpm);
        self.state.project_dirty = true;
    }

    /// If auto-warp-on-import is enabled, return a background task
    /// that detects the imported clip's BPM and warps it to the
    /// project tempo. Call this right after a clip is inserted into
    /// state / the engine. The caller propagates the Task to the
    /// iced runtime (helpers return it up through
    /// `apply_browser_sample_decoded`).
    fn schedule_auto_warp_if_enabled(
        &self,
        track_id: TrackId,
        clip_id: ClipId,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    ) -> Task<Message> {
        if !self.state.auto_warp_on_import || self.state.bpm <= 0.0 || self.state.sample_rate == 0 {
            return Task::none();
        }
        let input = AutoWarpInput {
            audio,
            sample_rate: self.state.sample_rate,
            project_bpm: self.state.bpm,
            confidence_threshold: self.state.warp_confidence_threshold,
        };
        Task::perform(auto_warp_clip_async(input), move |outcome| {
            Message::ClipAutoWarpReady {
                track_id,
                clip_id,
                outcome,
            }
        })
    }

    fn apply_auto_warp_outcome(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        outcome: crate::message::AutoWarpOutcome,
    ) {
        use crate::message::AutoWarpOutcome;
        match outcome {
            AutoWarpOutcome::NotDetected => {
                // Nothing to apply. Point the user at the manual
                // workflow in the clip detail panel.
                self.state.status_text =
                    "Auto-warp: could not detect BPM. Select the clip and type the source \
                     BPM in the Warp row, then press Enter and click Warp."
                        .to_string();
            }
            AutoWarpOutcome::DetectedOnly { bpm, confidence } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.original_bpm = Some(bpm);
                    }
                }
                self.state.status_text = format!(
                    "Detected {:.1} BPM (confidence {:.2}, below warp threshold)",
                    bpm, confidence
                );
                self.state.project_dirty = true;
            }
            AutoWarpOutcome::Warped { success, .. } => {
                self.apply_clip_warp_success(track_id, clip_id, success);
            }
        }
    }

    fn apply_clear_clip_warp(&mut self, track_id: TrackId, clip_id: ClipId) {
        let restore = self
            .state
            .find_track(track_id)
            .and_then(|track| track.clips.iter().find(|c| c.id == clip_id))
            .and_then(|clip| clip.original_audio.as_ref().map(Arc::clone));
        if let Some(original) = restore {
            let original_frames = original.num_frames() as u64;
            self.send_command(EngineCommand::ReplaceClipAudio {
                track_id,
                clip_id,
                audio: Arc::clone(&original),
                duration: original_frames,
                source_offset: 0,
                loop_start: 0,
                loop_end: 0,
            });
            if let Some(track) = self.state.find_track_mut(track_id) {
                if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                    clip.audio = original;
                    clip.duration = original_frames;
                    clip.source_offset = 0;
                    clip.loop_start = 0;
                    clip.loop_end = 0;
                    clip.warped = false;
                    clip.warped_to_bpm = None;
                    clip.original_audio = None;
                }
            }
        } else if let Some(track) = self.state.find_track_mut(track_id) {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                clip.warped = false;
                clip.warped_to_bpm = None;
            }
        }
        self.state.status_text = "Cleared clip warp".to_string();
        self.state.project_dirty = true;
    }

    fn quantize_note_clip(&mut self, track_id: TrackId, clip_id: ClipId) {
        let grid = self.state.snap_grid;
        let mut changes: Vec<(usize, MidiNote)> = Vec::new();
        let Some(track) = self.state.find_track_mut(track_id) else {
            return;
        };
        let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) else {
            return;
        };
        for (idx, note) in clip.notes.iter_mut().enumerate() {
            let snapped = grid.snap_beat(note.start_beat).max(0.0);
            if (snapped - note.start_beat).abs() > f64::EPSILON {
                note.start_beat = snapped;
                changes.push((idx, *note));
            }
        }
        let count = changes.len();
        for (idx, note) in changes {
            self.send_command(EngineCommand::EditNote {
                track_id,
                clip_id,
                note_index: idx,
                note,
            });
        }
        self.mark_project_dirty();
        self.state.status_text = format!("Quantized {count} note(s) to {}", grid.label());
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
                    | Message::WindowResized(_, _)
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
                | Message::DrumRackPadSampleDecoded(..)
                | Message::ClearDrumRackPad(..)
                | Message::SetDrumPadParam { .. }
                | Message::SetDrumPadOneShot { .. }
                | Message::SetDrumPadChokeGroup { .. }
                | Message::BrowserSampleDecoded(..)
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
                | Message::QuantizeNoteClip { .. }
                | Message::AudioQuantizeReady { .. }
                | Message::SetClipNominalBpm { .. }
                | Message::SubmitClipBpm { .. }
                | Message::ClipWarpReady { .. }
                | Message::ClearClipWarp { .. }
                | Message::ClipAutoWarpReady { .. }
        );
        if should_mark_dirty {
            self.push_undo_snapshot();
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
                self.state.time_selection_track = None;
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
                self.poll_midi_input();
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
                        let overshoot = ((self.state.cursor_x - (right_boundary - edge_zone))
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
                let track_num = self.next_unique_track_number("Track");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number = track_num + 1;
                let id = TrackId::new();
                let name = format!("Track {track_num}");

                self.send_command(EngineCommand::AddTrack(id, name.clone()));
                self.state.tracks.push(UiTrack::new(id, name, color_index));
                self.state.selected_track = Some(id);
                self.state.status_text = format!("{} tracks", self.state.tracks.len());
            }
            Message::RemoveTrack(track_id) => {
                // Capture identity before mutating so we can report exactly
                // which track was removed. Helps diagnose the "deleted the
                // wrong track" reports.
                let removed_name = self
                    .state
                    .find_track(track_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| format!("{track_id}"));

                // Close all plugin GUI windows for this track
                if let Some(ref mut mgr) = self.plugin_window_manager {
                    mgr.close_track_effects(track_id);
                }
                self.plugin_gui_raw_ptrs.retain(|k, _| match k {
                    PluginGuiKey::Effect { track_id: tid, .. } => *tid != track_id,
                    PluginGuiKey::Instrument { track_id: tid } => *tid != track_id,
                });
                self.plugin_state_ptrs.retain(|k, _| match k {
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
                self.state.status_text = format!(
                    "Removed {removed_name}. {} track(s) remain.",
                    self.state.tracks.len()
                );
            }
            Message::AuditionNote {
                track_id,
                pitch,
                on,
            } => {
                self.send_command(EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity: 100,
                    on,
                });
            }
            Message::DeleteKeyPressed => {
                // Never delete anything while a text field is being
                // edited; backspace belongs to the text there.
                if self.state.editing_track_name.is_some() || self.state.editing_clip_name.is_some()
                {
                    return Task::none();
                }
                // Priority 1: selected notes in the open piano roll.
                if let Some((track_id, clip_id)) = self.state.selected_note_clip {
                    let has_selection = self
                        .state
                        .find_track(track_id)
                        .and_then(|t| t.note_clips.iter().find(|c| c.id == clip_id))
                        .is_some_and(|c| !c.selected_notes.is_empty());
                    if has_selection {
                        return self.update(Message::RemoveSelectedNotes(track_id, clip_id));
                    }
                }
                // Priority 2: selected arrangement clips.
                if !self.state.selected_clips.is_empty() {
                    return self.update(Message::DeleteSelectedClip);
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
                        original_bpm: None,
                        warped: false,
                        warped_to_bpm: None,
                        original_audio: None,
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
                        plugin_ref: None,
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
                self.plugin_state_ptrs.remove(&gui_key);

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
                let track_num = self.next_unique_track_number("MIDI");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number = track_num + 1;
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

                    return Task::perform(decode_file_async(path), move |result| match result {
                        Ok(audio) => Message::SamplerSampleDecoded(
                            track_id,
                            Arc::new(audio),
                            file_name.clone(),
                            source.clone(),
                        ),
                        Err(e) => Message::SamplerDecodeError(track_id, e),
                    });
                }
            }
            Message::SamplerSampleDecoded(track_id, audio, name, source) => {
                self.apply_sampler_sample_loaded(track_id, audio, name, source);
            }
            Message::SamplerDecodeError(track_id, err) => {
                self.state.selected_track = Some(track_id);
                self.state.status_text = format!("Sample load error: {err}");
            }
            Message::LoadDrumRackPadSample(track_id, pad_index) => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Load Drum Pad Sample")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    move |path| Message::DrumRackPadFileSelected(track_id, pad_index, path),
                );
            }
            Message::DrumRackPadFileSelected(track_id, pad_index, path) => {
                if let Some(path) = path {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.state.status_text = format!("Loading {file_name}...");
                    let source = MediaSourceRef::LocalFile { path: path.clone() };

                    return Task::perform(decode_file_async(path), move |result| match result {
                        Ok(audio) => Message::DrumRackPadSampleDecoded(
                            track_id,
                            pad_index,
                            Arc::new(audio),
                            file_name.clone(),
                            source.clone(),
                        ),
                        Err(e) => Message::DrumRackPadDecodeError(track_id, pad_index, e),
                    });
                }
            }
            Message::DrumRackPadSampleDecoded(track_id, pad_index, audio, name, source) => {
                self.apply_drum_rack_pad_loaded(track_id, pad_index, audio, name, source);
            }
            Message::DrumRackPadDecodeError(track_id, _pad_index, err) => {
                self.state.selected_track = Some(track_id);
                self.state.status_text = format!("Drum pad load error: {err}");
            }
            Message::ClearDrumRackPad(track_id, pad_index) => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        *pad = UiDrumPad::default();
                    }
                }
                self.sync_drum_rack_pad_state(track_id, pad_index);
                self.send_command(EngineCommand::ClearDrumRackPad {
                    track_id,
                    pad_index,
                });
                self.state.status_text = format!("Cleared pad {}", pad_index + 1);
            }
            Message::SelectDrumRackPad(track_id, pad_index) => {
                // Audition the pad like Ableton: hear it on click.
                let pitch = 36 + pad_index.min(127) as u8;
                self.send_command(EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity: 100,
                    on: true,
                });
                self.send_command(EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity: 100,
                    on: false,
                });
                if let Some(track) = self.state.find_track_mut(track_id) {
                    let max_index = track.drum_rack_pads.len().saturating_sub(1);
                    track.selected_drum_pad = pad_index.min(max_index);
                }
                self.state.selected_track = Some(track_id);
            }
            Message::SetDrumPadParam {
                track_id,
                pad_index,
                param,
                value,
            } => {
                let mut changed = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        match param {
                            DrumPadParam::Gain => pad.gain = value.clamp(0.0, 2.0),
                            DrumPadParam::Pan => pad.pan = value.clamp(-1.0, 1.0),
                            DrumPadParam::Start => pad.start = value.clamp(0.0, 1.0),
                            DrumPadParam::End => pad.end = value.clamp(0.0, 1.0),
                            DrumPadParam::CoarseTune => {
                                pad.coarse_tune = value.clamp(-24.0, 24.0).round() as i8;
                            }
                            DrumPadParam::FineTune => pad.fine_tune = value.clamp(-100.0, 100.0),
                        }
                        changed = true;
                    }
                }
                if changed {
                    self.sync_drum_rack_pad_state(track_id, pad_index);
                    self.state.status_text = format!("Pad {} updated", pad_index + 1);
                }
            }
            Message::SetDrumPadOneShot {
                track_id,
                pad_index,
                one_shot,
            } => {
                let mut changed = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        pad.one_shot = one_shot;
                        changed = true;
                    }
                }
                if changed {
                    self.sync_drum_rack_pad_state(track_id, pad_index);
                    self.state.status_text = format!("Pad {} updated", pad_index + 1);
                }
            }
            Message::SetDrumPadChokeGroup {
                track_id,
                pad_index,
                choke_group,
            } => {
                let mut changed = false;
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        pad.choke_group = choke_group;
                        changed = true;
                    }
                }
                if changed {
                    self.sync_drum_rack_pad_state(track_id, pad_index);
                    self.state.status_text = format!("Pad {} updated", pad_index + 1);
                }
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
                        // Default the loop region whenever the stored
                        // one is unusable: never set, inverted, or
                        // stale from before a resize. Ableton
                        // semantics: the loop region covers the
                        // CONTENT (rounded up to whole bars), so a
                        // 1-bar pattern inside a longer clip repeats
                        // bar by bar instead of playing once followed
                        // by silence. Bug #3 in the dogfood log.
                        let invalid = clip.loop_end_beats <= clip.loop_start_beats
                            || clip.loop_end_beats > clip.duration_beats;
                        if clip.loop_enabled && invalid {
                            clip.loop_start_beats = 0.0;
                            clip.loop_end_beats =
                                default_loop_end(&clip.notes, clip.duration_beats);
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
                self.state.selected_track = Some(track_id);
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
                            clip.selected_notes = clip
                                .selected_notes
                                .iter()
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
                let indices_to_remove: Vec<usize> =
                    if let Some(track) = self.state.find_track(track_id) {
                        if let Some(clip) = track.note_clips.iter().find(|c| c.id == clip_id) {
                            let mut indices: Vec<usize> = clip
                                .selected_notes
                                .iter()
                                .copied()
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
            Message::NudgeSelectedNotes {
                track_id,
                clip_id,
                delta_beats,
                delta_semitones,
            } => {
                let mut updates: Vec<(usize, MidiNote)> = Vec::new();
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.note_clips.iter_mut().find(|c| c.id == clip_id) {
                        let indices: Vec<usize> = clip
                            .selected_notes
                            .iter()
                            .copied()
                            .filter(|&i| i < clip.notes.len())
                            .collect();
                        for &idx in &indices {
                            let note = &mut clip.notes[idx];
                            note.start_beat = (note.start_beat + delta_beats).max(0.0);
                            note.pitch =
                                (note.pitch as i16 + delta_semitones as i16).clamp(0, 127) as u8;
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

            Message::MoveNotesAbsolute {
                track_id,
                clip_id,
                moves,
            } => {
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
                        let was_full_clip_loop = clip.loop_enabled
                            && clip.loop_start_beats == 0.0
                            && (clip.loop_end_beats - clip.duration_beats).abs() < 1e-9;
                        clip.duration_beats *= 2.0;
                        if was_full_clip_loop {
                            clip.loop_end_beats = clip.duration_beats;
                        }
                        let new_duration = clip.duration_beats;
                        let loop_sync = (
                            clip.loop_enabled,
                            clip.loop_start_beats,
                            clip.loop_end_beats,
                        );

                        // Send new notes to engine
                        for note in &cloned_notes {
                            self.send_command(EngineCommand::AddNote {
                                track_id,
                                clip_id,
                                note: *note,
                            });
                        }
                        // The engine clip must grow too, or playback
                        // still ends at the old boundary and the
                        // duplicated notes never sound.
                        self.send_command(EngineCommand::SetNoteClipDuration {
                            track_id,
                            clip_id,
                            duration_beats: new_duration,
                        });
                        self.send_command(EngineCommand::SetNoteClipLoop {
                            track_id,
                            clip_id,
                            enabled: loop_sync.0,
                            loop_start_beats: loop_sync.1,
                            loop_end_beats: loop_sync.2,
                        });
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
                        clip_end_beat = Some((clip.position + clip.duration) as f64 / spb);
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

                        // Keep the loop region inside the clip when
                        // shrinking. Extending leaves the region
                        // untouched so the looped pattern repeats to
                        // fill the new length (the whole point of
                        // stretching a looped clip).
                        if clip.loop_enabled && clip.loop_end_beats > new_duration_beats {
                            clip.loop_end_beats = new_duration_beats;
                            if clip.loop_start_beats >= clip.loop_end_beats {
                                clip.loop_start_beats = 0.0;
                            }
                        }

                        // Auto-enable loop when extending past note content
                        // Only if the clip actually has notes — empty clips don't loop
                        if !clip.notes.is_empty() && !clip.loop_enabled {
                            let loop_end = default_loop_end(&clip.notes, new_duration_beats);
                            if loop_end > 0.0 && new_duration_beats > loop_end {
                                clip.loop_enabled = true;
                                clip.loop_start_beats = 0.0;
                                clip.loop_end_beats = loop_end;
                            }
                        }

                        clip_end_beat = Some(clip.position_beats + clip.duration_beats);
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
                            original_bpm: None,
                            warped: false,
                            warped_to_bpm: None,
                            original_audio: None,
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
                            original_bpm: None,
                            warped: false,
                            warped_to_bpm: None,
                            original_audio: None,
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
                                if let Some((
                                    audio,
                                    name,
                                    source,
                                    position,
                                    source_offset,
                                    duration,
                                )) = dup_data
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
                                            original_bpm: None,
                                            warped: false,
                                            warped_to_bpm: None,
                                            original_audio: None,
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
                        track_id: self.state.time_selection_track,
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
                track_id,
            } => {
                self.state.selection_start_beats = start_beats;
                self.state.selection_end_beats = end_beats;
                self.state.time_selection_active = true;
                self.state.time_selection_track = track_id;
                if let Some(tid) = track_id {
                    self.state.selected_track = Some(tid);
                }
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
                if !active {
                    self.state.time_selection_track = None;
                }
            }
            Message::CursorMoved(x, y) => {
                self.state.cursor_x = x;
                self.state.cursor_y = y;
            }
            Message::WindowResized(w, h) => {
                self.state.window_width = w;
                self.state.window_height = h;
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
                track_id: target_track,
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
                    if let Some(tid) = target_track {
                        if track.id != tid {
                            continue;
                        }
                    }
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
                track_id: target_track,
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

                    // Collect audio splits for this boundary, limited to
                    // the originating track when the selection was drawn
                    // on a single lane.
                    let audio_hits: Vec<(TrackId, ClipId)> = if spb > 0.0 {
                        self.state
                            .tracks
                            .iter()
                            .filter(|t| target_track.is_none_or(|tid| t.id == tid))
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
                        .filter(|t| target_track.is_none_or(|tid| t.id == tid))
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
                self.state
                    .selected_clips
                    .insert(ArrangementSelection::NoteClip { track_id, clip_id });
                self.state.status_text = "Created note clip from selection".to_string();
            }

            // -- Track reordering --
            Message::MoveTrackUp(track_id) => {
                if let Some(idx) = self.state.tracks.iter().position(|t| t.id == track_id) {
                    if idx > 0 {
                        self.state.tracks.swap(idx, idx - 1);
                        let order: Vec<TrackId> = self.state.tracks.iter().map(|t| t.id).collect();
                        self.send_command(EngineCommand::ReorderTracks(order));
                    }
                }
            }
            Message::MoveTrackDown(track_id) => {
                if let Some(idx) = self.state.tracks.iter().position(|t| t.id == track_id) {
                    if idx + 1 < self.state.tracks.len() {
                        self.state.tracks.swap(idx, idx + 1);
                        let order: Vec<TrackId> = self.state.tracks.iter().map(|t| t.id).collect();
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
                let track_num = self.next_unique_track_number("MIDI");
                let color_index = (track_num.wrapping_sub(1) % 8) as u8;
                self.state.next_track_number = track_num + 1;
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
                    track.sample_audio = None;
                    track.instrument_params = instrument_params.clone();
                    track.drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
                    track.selected_drum_pad = 0;
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
                self.plugin_state_ptrs.remove(&gui_key);

                if let Some(track) = self.state.find_track_mut(track_id) {
                    track.has_instrument = false;
                    track.instrument_kind = None;
                    track.sample_name = None;
                    track.sample_source = None;
                    track.instrument_params.clear();
                    track.drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
                    track.selected_drum_pad = 0;
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

            // -- Sample browser --
            Message::ToggleSampleBrowser => {
                self.state.sample_browser_open = !self.state.sample_browser_open;
                self.persist_ui_settings();
            }
            Message::ToggleAutoWarpOnImport => {
                self.state.auto_warp_on_import = !self.state.auto_warp_on_import;
                self.persist_ui_settings();
            }
            Message::SetWarpConfidenceThreshold(v) => {
                self.state.warp_confidence_threshold = v.clamp(0.0, 1.0);
                self.persist_ui_settings();
            }
            Message::RescanMidiInputs => {
                match vibez_audio_io::midi_input::list_midi_input_ports() {
                    Ok(ports) => {
                        self.midi_input_ports = ports;
                        self.state.status_text =
                            format!("Found {} MIDI input port(s)", self.midi_input_ports.len());
                    }
                    Err(err) => {
                        self.midi_input_ports.clear();
                        self.state.status_text = format!("MIDI scan error: {err}");
                    }
                }
            }
            Message::OpenMidiInput(name) => {
                match vibez_audio_io::midi_input::open_midi_input(&name) {
                    Ok(handle) => {
                        self.state.status_text = format!("MIDI input: {}", handle.port_name);
                        self.midi_input = Some(handle);
                        self.persist_ui_settings();
                    }
                    Err(err) => {
                        self.state.status_text = format!("MIDI open error: {err}");
                    }
                }
            }
            Message::CloseMidiInput => {
                self.midi_input = None;
                self.persist_ui_settings();
                self.state.status_text = "MIDI input disconnected".to_string();
            }
            Message::RewarpAllClips => {
                // Collect targets first so we don't hold a borrow across dispatch.
                let targets: Vec<(TrackId, ClipId)> = self
                    .state
                    .tracks
                    .iter()
                    .flat_map(|track| {
                        track
                            .clips
                            .iter()
                            .filter(|c| c.warped && c.original_bpm.is_some())
                            .map(move |c| (track.id, c.id))
                    })
                    .collect();
                if targets.is_empty() {
                    self.state.status_text = "Re-warp all: no warped clips to re-warp".to_string();
                    return Task::none();
                }
                let count = targets.len();
                let tasks: Vec<Task<Message>> = targets
                    .into_iter()
                    .map(|(tid, cid)| self.dispatch_warp_clip_to_project(tid, cid))
                    .collect();
                self.state.status_text =
                    format!("Re-warping {count} clip(s) to {:.0} BPM", self.state.bpm);
                return Task::batch(tasks);
            }
            Message::AddSampleLibraryRoot => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Add Sample Library Root")
                            .pick_folder()
                            .await;
                        handle.map(|folder| folder.path().to_path_buf())
                    },
                    Message::SampleLibraryRootSelected,
                );
            }
            Message::SampleLibraryRootSelected(path) => {
                if let Some(path) = path {
                    if !self
                        .state
                        .sample_browser_roots
                        .iter()
                        .any(|root| root == &path)
                    {
                        self.state.sample_browser_roots.push(path.clone());
                        self.state.sample_browser_roots.sort();
                        self.persist_ui_settings();
                    }
                    self.state.sample_browser_scan_in_progress = true;
                    self.state.status_text = format!("Scanning {}...", path.display());
                    return Task::perform(
                        scan_sample_library_async(self.state.sample_browser_roots.clone()),
                        Message::SampleLibraryScanned,
                    );
                }
            }
            Message::RemoveSampleLibraryRoot(path) => {
                self.state.sample_browser_roots.retain(|root| root != &path);
                if self
                    .state
                    .sample_browser_root_filter
                    .as_ref()
                    .is_some_and(|root| root == &path)
                {
                    self.state.sample_browser_root_filter = None;
                }
                self.state
                    .sample_browser_entries
                    .retain(|entry| entry.root_path != path);
                if self
                    .state
                    .sample_browser_selected_source
                    .as_ref()
                    .and_then(|selected| {
                        self.state
                            .sample_browser_entries
                            .iter()
                            .find(|entry| &entry.source == selected)
                    })
                    .is_none()
                {
                    self.state.sample_browser_selected_source = None;
                }
                self.persist_ui_settings();
                self.state.status_text = "Removed sample root".to_string();
            }
            Message::RescanSampleLibrary => {
                self.state.sample_browser_scan_in_progress = true;
                self.state.status_text = "Rescanning sample library...".to_string();
                return Task::perform(
                    scan_sample_library_async(self.state.sample_browser_roots.clone()),
                    Message::SampleLibraryScanned,
                );
            }
            Message::SampleLibraryScanned(result) => {
                self.state.sample_browser_scan_in_progress = false;
                match result {
                    Ok(scan) => {
                        self.state.sample_browser_entries = scan.entries;
                        if self
                            .state
                            .sample_browser_selected_source
                            .as_ref()
                            .and_then(|selected| {
                                self.state
                                    .sample_browser_entries
                                    .iter()
                                    .find(|entry| &entry.source == selected)
                            })
                            .is_none()
                        {
                            self.state.sample_browser_selected_source = self
                                .state
                                .sample_browser_entries
                                .first()
                                .map(|entry| entry.source.clone());
                        }
                        self.state.status_text = if scan.warnings.is_empty() {
                            format!(
                                "Indexed {} samples",
                                self.state.sample_browser_entries.len()
                            )
                        } else {
                            format!(
                                "Indexed {} samples with {} warning(s)",
                                self.state.sample_browser_entries.len(),
                                scan.warnings.len()
                            )
                        };
                    }
                    Err(err) => {
                        self.state.status_text = format!("Sample scan error: {err}");
                    }
                }
            }
            Message::SampleBrowserSearchChanged(query) => {
                self.state.sample_browser_search = query;
            }
            Message::SelectSampleBrowserRoot(root) => {
                self.state.sample_browser_root_filter = root;
            }
            Message::SelectSampleBrowserEntry(source) => {
                self.state.sample_browser_selected_source = Some(source);
            }
            Message::ClickLocalBrowserEntry(source) => {
                // Previously auto-previewed on click; now click only
                // selects. Preview fires via the speaker icon (see
                // `Message::PreviewLocalEntry`).
                self.state.sample_browser_selected_source = Some(source);
            }
            Message::PreviewLocalEntry(source) => {
                self.state.sample_browser_selected_source = Some(source.clone());
                if let MediaSourceRef::LocalFile { path } = source {
                    self.state.status_text = "Previewing...".to_string();
                    return Task::perform(
                        decode_local_for_preview_async(path),
                        Message::LocalSamplePreviewReady,
                    );
                }
            }
            // -- Drag-and-drop from sample browser --
            Message::StartDragSample { source, label } => {
                self.state.status_text = format!("Dragging {label} - drop on a lane or drum pad");
                self.state.drag_source = Some(source);
                self.state.drag_label = Some(label);
                self.state.drag_hover_track = None;
                self.state.drag_hover_beat = 0.0;
            }
            Message::DragHoverTrack { track_id, beat } => {
                self.state.drag_hover_track = Some(track_id);
                self.state.drag_hover_beat = beat;
            }
            Message::EndDragSample => {
                if let Some(source) = self.state.drag_source.take() {
                    self.state.drag_label = None;
                    // If a drop target was hovered recently, route the drop
                    // there instead of cancelling. Protects against
                    // sub-pixel release-outside-bounds misses.
                    if let Some(track_id) = self.state.drag_hover_track.take() {
                        let position_samples =
                            self.state.beats_to_samples(self.state.drag_hover_beat);
                        self.state.drag_hover_beat = 0.0;
                        return self.dispatch_drop_on_arrangement(
                            track_id,
                            position_samples,
                            source,
                        );
                    }
                    self.state.drag_hover_beat = 0.0;
                    self.state.status_text = "Drag cancelled".to_string();
                }
            }
            Message::DropSampleOnArrangement {
                track_id,
                position_samples,
            } => {
                let Some(source) = self.state.drag_source.take() else {
                    return Task::none();
                };
                self.state.drag_label = None;
                return self.dispatch_drop_on_arrangement(track_id, position_samples, source);
            }
            Message::DropSampleOnDrumPad {
                track_id,
                pad_index,
            } => {
                match self.state.drag_source.take() {
                    Some(source) => {
                        self.state.drag_label = None;
                        return self.dispatch_drop_for_target(
                            source,
                            BrowserImportTarget::DrumRackPad {
                                track_id,
                                pad_index,
                            },
                        );
                    }
                    None => {
                        // No active drag: treat release as a click.
                        // Select the pad AND audition its loaded sample
                        // via the engine's preview channel (bypasses
                        // transport + mute + solo; one-shot). This is
                        // the fastest way to hear what's on a pad
                        // without drawing notes into the piano roll.
                        let audition = self
                            .state
                            .find_track(track_id)
                            .and_then(|track| track.drum_rack_pads.get(pad_index))
                            .and_then(|pad| {
                                pad.audio.as_ref().map(|audio| {
                                    (
                                        Arc::clone(audio),
                                        pad.name.clone().unwrap_or_else(|| "sample".into()),
                                    )
                                })
                            });
                        if let Some((audio, name)) = audition {
                            self.send_command(EngineCommand::StartPreview(audio));
                            self.state.status_text = format!("Pad {}: {}", pad_index + 1, name);
                        }
                        return self.update(Message::SelectDrumRackPad(track_id, pad_index));
                    }
                }
            }
            Message::DropSampleOnSampler { track_id } => {
                let Some(source) = self.state.drag_source.take() else {
                    return Task::none();
                };
                self.state.drag_label = None;
                return self
                    .dispatch_drop_for_target(source, BrowserImportTarget::Sampler(track_id));
            }
            Message::LocalSamplePreviewReady(Ok(audio)) => {
                self.send_command(EngineCommand::StartPreview(audio));
                self.state.status_text = "Preview playing".to_string();
            }
            Message::LocalSamplePreviewReady(Err(err)) => {
                self.state.status_text = format!("Preview error: {err}");
            }
            Message::ImportSelectedBrowserSampleToArrangement => {
                if let Some(entry) = self.selected_sample_browser_entry().cloned() {
                    let target = BrowserImportTarget::ArrangementClip(
                        self.state.selected_track.filter(|track_id| {
                            self.state
                                .find_track(*track_id)
                                .is_some_and(|track| matches!(track.kind, TrackKind::Audio))
                        }),
                    );
                    if let MediaSourceRef::LocalFile { path } = &entry.source {
                        let source = entry.source.clone();
                        let name = entry.name.clone();
                        self.state.status_text = format!("Loading {name}...");
                        return Task::perform(decode_file_async(path.clone()), move |result| {
                            match result {
                                Ok(audio) => Message::BrowserSampleDecoded(
                                    target.clone(),
                                    Arc::new(audio),
                                    name.clone(),
                                    source.clone(),
                                ),
                                Err(err) => Message::BrowserSampleDecodeError(err),
                            }
                        });
                    }
                }
            }
            Message::LoadSelectedBrowserSampleToDevice => {
                let Some(entry) = self.selected_sample_browser_entry().cloned() else {
                    return Task::none();
                };
                let Some(target) = self.selected_browser_device_target() else {
                    self.state.status_text =
                        "Select a sampler or drum rack track to load from the browser".to_string();
                    return Task::none();
                };
                if let MediaSourceRef::LocalFile { path } = &entry.source {
                    let source = entry.source.clone();
                    let name = entry.name.clone();
                    self.state.status_text = format!("Loading {name}...");
                    return Task::perform(
                        decode_file_async(path.clone()),
                        move |result| match result {
                            Ok(audio) => Message::BrowserSampleDecoded(
                                target.clone(),
                                Arc::new(audio),
                                name.clone(),
                                source.clone(),
                            ),
                            Err(err) => Message::BrowserSampleDecodeError(err),
                        },
                    );
                }
            }
            Message::BrowserSampleDecoded(target, audio, name, source) => {
                return self.apply_browser_sample_decoded(target, audio, name, source);
            }
            Message::ClipAutoWarpReady {
                track_id,
                clip_id,
                outcome,
            } => {
                self.apply_auto_warp_outcome(track_id, clip_id, outcome);
            }
            Message::BrowserSampleDecodeError(err) => {
                self.state.status_text = format!("Browser load error: {err}");
            }

            // -- File menu --
            Message::NewProject => {
                self.state.file_menu_open = false;
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
                    let dropbox = self
                        .dropbox_client
                        .clone()
                        .map(|client| (client, self.dropbox_cache.clone()));
                    return Task::perform(
                        load_project_async(path, dropbox),
                        Message::ProjectLoaded,
                    );
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
                                vibez_plugin_host::scan_plugins_sandboxed(&settings)
                            })
                            .await
                            .unwrap_or_default()
                        },
                        Message::ScanPluginsComplete,
                    );
                }
            }
            Message::ScanPluginsComplete(report) => {
                let count = report.plugins.len();
                self.state.plugin_settings.cache = report.plugins;
                self.state.plugin_scan_in_progress = false;
                self.state.plugin_scan_status = if report.failed.is_empty() {
                    format!("Found {count} plugins")
                } else {
                    for (path, reason) in &report.failed {
                        eprintln!("vibez: plugin skipped: {path:?}: {reason}");
                    }
                    let names: Vec<String> = report
                        .failed
                        .iter()
                        .filter_map(|(p, _)| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .collect();
                    format!(
                        "Found {count} plugins ({} skipped: {})",
                        report.failed.len(),
                        names.join(", ")
                    )
                };
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
                            match load_plugin_instrument_bg(&info, sample_rate, None) {
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
                            match load_plugin_effect_bg(&info, sample_rate, None) {
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
                            self.state.status_text = "Failed to open plugin GUI".to_string();
                        }
                    } else {
                        self.state.status_text =
                            "No X11 display — plugin GUI unavailable".to_string();
                    }
                } else {
                    self.state.status_text = "Plugin GUI handle not available".to_string();
                }
            }
            Message::ClosePluginGui(key) => {
                if let Some(ref mut mgr) = self.plugin_window_manager {
                    mgr.close(key);
                }
            }

            // -- Bounce / resample --
            Message::BounceSelectionToAudio => {
                self.state.context_menu = None;
                if !self.state.time_selection_active
                    || self.state.selection_end_beats <= self.state.selection_start_beats
                {
                    self.state.status_text = "No time selection active".to_string();
                    return Task::none();
                }
                let start = self
                    .state
                    .beats_to_samples(self.state.selection_start_beats);
                let end = self.state.beats_to_samples(self.state.selection_end_beats);
                return self.dispatch_bounce(
                    vibez_engine::render::BounceMode::Master,
                    (start, end),
                    start,
                    format!(
                        "Selection {:.2}–{:.2}",
                        self.state.selection_start_beats, self.state.selection_end_beats
                    ),
                );
            }
            Message::BounceClipToAudio {
                track_id,
                clip_id,
                is_note_clip,
            } => {
                self.state.context_menu = None;
                let (range, insert_pos, name) = if is_note_clip {
                    let spb = self.state.sample_rate as f64 * 60.0 / self.state.bpm;
                    let track_opt = self.state.find_track(track_id);
                    let nc = track_opt.and_then(|t| t.note_clips.iter().find(|c| c.id == clip_id));
                    match nc {
                        Some(nc) => {
                            let start = (nc.position_beats * spb) as u64;
                            let end = ((nc.position_beats + nc.duration_beats) * spb) as u64;
                            (Some((start, end)), start, nc.name.clone())
                        }
                        None => (None, 0, String::new()),
                    }
                } else {
                    let track_opt = self.state.find_track(track_id);
                    let ac = track_opt.and_then(|t| t.clips.iter().find(|c| c.id == clip_id));
                    match ac {
                        Some(ac) => (
                            Some((ac.position, ac.position + ac.duration)),
                            ac.position,
                            ac.name.clone(),
                        ),
                        None => (None, 0, String::new()),
                    }
                };
                let Some(range) = range else {
                    self.state.status_text = "Clip not found".to_string();
                    return Task::none();
                };
                return self.dispatch_bounce(
                    vibez_engine::render::BounceMode::Clip {
                        track_id,
                        clip_id,
                        is_note_clip,
                    },
                    range,
                    insert_pos,
                    name,
                );
            }
            Message::BounceComplete(Ok(outcome)) => {
                self.finalize_bounce(outcome);
            }
            Message::BounceComplete(Err(err)) => {
                self.state.status_text = format!("Bounce error: {err}");
            }

            // -- Quantize --
            Message::QuantizeNoteClip { track_id, clip_id } => {
                self.state.context_menu = None;
                self.quantize_note_clip(track_id, clip_id);
            }
            Message::QuantizeAudioClip { track_id, clip_id } => {
                self.state.context_menu = None;
                let grid = self.state.snap_grid;
                return self.dispatch_audio_quantize(track_id, clip_id, grid);
            }
            Message::QuantizeAudioClipAt {
                track_id,
                clip_id,
                grid,
            } => {
                return self.dispatch_audio_quantize(track_id, clip_id, grid);
            }
            Message::AudioQuantizeReady {
                track_id,
                old_clip_id,
                result,
            } => match result {
                Ok(success) => self.apply_audio_quantize_success(track_id, old_clip_id, success),
                Err(err) => {
                    self.state.status_text = format!("Quantize failed: {err}");
                }
            },

            // -- Warping --
            Message::DetectClipBpm { track_id, clip_id } => {
                return self.dispatch_detect_clip_bpm(track_id, clip_id);
            }
            Message::ClipBpmDetected {
                track_id,
                clip_id,
                bpm,
                confidence,
            } => match bpm {
                Some(b) => {
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.original_bpm = Some(b);
                        }
                    }
                    self.state.clip_bpm_edit.remove(&clip_id);
                    self.state.status_text =
                        format!("Detected {:.1} BPM (confidence {:.2})", b, confidence);
                    self.state.project_dirty = true;
                }
                None => {
                    self.state.status_text =
                        "Could not detect BPM. Type the source BPM in the Warp row and \
                         press Enter, then click Warp."
                            .to_string();
                }
            },
            Message::ClipBpmInputChanged {
                track_id: _,
                clip_id,
                text,
            } => {
                self.state.clip_bpm_edit.insert(clip_id, text);
            }
            Message::SubmitClipBpm { track_id, clip_id } => {
                let parsed = self
                    .state
                    .clip_bpm_edit
                    .remove(&clip_id)
                    .and_then(|t| t.parse::<f64>().ok())
                    .filter(|b| *b > 0.0 && *b < 1_000.0);
                if let Some(bpm) = parsed {
                    if let Some(track) = self.state.find_track_mut(track_id) {
                        if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                            clip.original_bpm = Some(bpm);
                        }
                    }
                    self.state.status_text = format!("Clip BPM set to {:.1}", bpm);
                    self.state.project_dirty = true;
                }
            }
            Message::SetClipNominalBpm {
                track_id,
                clip_id,
                bpm,
            } => {
                if let Some(track) = self.state.find_track_mut(track_id) {
                    if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                        clip.original_bpm = Some(bpm);
                    }
                }
                self.state.status_text = format!("Clip BPM set to {:.1}", bpm);
                self.state.project_dirty = true;
            }
            Message::WarpClipToProject { track_id, clip_id } => {
                return self.dispatch_warp_clip_to_project(track_id, clip_id);
            }
            Message::ClipWarpReady {
                track_id,
                clip_id,
                result,
            } => match result {
                Ok(success) => self.apply_clip_warp_success(track_id, clip_id, success),
                Err(err) => {
                    self.state.status_text = format!("Warp failed: {err}");
                }
            },
            Message::ClearClipWarp { track_id, clip_id } => {
                self.apply_clear_clip_warp(track_id, clip_id);
            }

            // -- Undo / redo --
            Message::Undo => {
                self.undo();
            }
            Message::Redo => {
                self.redo();
            }

            // -- Export --
            Message::ExportProject => {
                self.state.file_menu_open = false;
                let default_name = self
                    .state
                    .current_project_path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|n| format!("{}.wav", n.to_string_lossy()))
                    .unwrap_or_else(|| "vibez-export.wav".to_string());
                return Task::perform(
                    async move {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Export to WAV")
                            .set_file_name(&default_name)
                            .add_filter("WAV", &["wav"])
                            .save_file()
                            .await;
                        handle.map(|file| file.path().to_path_buf())
                    },
                    Message::ExportPathSelected,
                );
            }
            Message::ExportPathSelected(path) => {
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
                let sample_rate = self.state.sample_rate;
                let bpm = self.state.bpm;
                let request = vibez_engine::render::BounceRequest {
                    tracks: project.tracks,
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
                return Task::perform(export_async(request, path), Message::ExportComplete);
            }
            Message::ExportComplete(Ok(path)) => {
                self.state.status_text = format!("Exported: {}", path.display());
            }
            Message::ExportComplete(Err(err)) => {
                self.state.status_text = format!("Export error: {err}");
            }

            // -- Sample browser mode --
            Message::SetSampleBrowserMode(mode) => {
                self.state.sample_browser_mode = mode;
                if mode == crate::state::SampleBrowserMode::Dropbox
                    && self.dropbox_client.is_some()
                    && !self.state.dropbox.folders.contains_key("")
                    && !self.state.dropbox.listing_in_progress.contains("")
                {
                    return self.update(Message::DropboxExpandFolder(String::new()));
                }
            }

            // -- Dropbox --
            Message::SetDropboxAppKey(key) => {
                self.state.dropbox.app_key_input = key;
            }
            Message::SaveDropboxAppKey => {
                let value = self.state.dropbox.app_key_input.trim().to_string();
                self.dropbox_settings.app_key = if value.is_empty() { None } else { Some(value) };
                if let Err(err) = self.dropbox_settings.save() {
                    self.state.dropbox.last_error = Some(format!("save settings: {err}"));
                }
                self.state.dropbox.has_app_key =
                    load_app_key_with_env_override(&self.dropbox_settings).is_some();
                self.state.status_text = "Dropbox app key saved".to_string();
            }
            Message::ConnectDropbox => {
                let Some(app_key) = load_app_key_with_env_override(&self.dropbox_settings) else {
                    self.state.dropbox.last_error = Some(
                        "No Dropbox app key set. Register an app at dropbox.com/developers/apps \
                        and paste the App key above."
                            .into(),
                    );
                    return Task::none();
                };
                if self.state.dropbox.auth_in_progress {
                    return Task::none();
                }
                self.state.dropbox.auth_in_progress = true;
                self.state.dropbox.last_error = None;
                self.state.status_text = "Opening Dropbox authorisation...".to_string();
                return Task::perform(connect_dropbox_async(app_key), |result| {
                    Message::DropboxConnected(result.map(|(info, tokens)| {
                        crate::message::DropboxConnectOutcome { info, tokens }
                    }))
                });
            }
            Message::DropboxConnected(Ok(outcome)) => {
                self.state.dropbox.auth_in_progress = false;
                if let Some(app_key) = load_app_key_with_env_override(&self.dropbox_settings) {
                    let client = DropboxClient::new(app_key, outcome.tokens.clone());
                    self.dropbox_client = Some(Arc::new(client));
                }
                self.dropbox_settings.tokens = Some(outcome.tokens.clone());
                self.dropbox_settings.account_email = Some(outcome.info.email.clone());
                if let Err(err) = self.dropbox_settings.save() {
                    self.state.dropbox.last_error = Some(format!("save settings: {err}"));
                }
                self.state.dropbox.connected = true;
                self.state.dropbox.account_email = Some(outcome.info.email.clone());
                self.state.status_text = format!("Dropbox connected: {}", outcome.info.email);
            }
            Message::DropboxConnected(Err(err)) => {
                self.state.dropbox.auth_in_progress = false;
                self.state.dropbox.last_error = Some(err.clone());
                self.state.status_text = format!("Dropbox connect failed: {err}");
            }
            Message::DisconnectDropbox => {
                self.dropbox_client = None;
                self.dropbox_settings.clear_tokens();
                let _ = self.dropbox_settings.save();
                self.state.dropbox = crate::state::DropboxUiState {
                    app_key_input: self.state.dropbox.app_key_input.clone(),
                    has_app_key: self.state.dropbox.has_app_key,
                    ..Default::default()
                };
                self.state.status_text = "Dropbox disconnected".to_string();
            }
            Message::DropboxExpandFolder(path) => {
                self.state.dropbox.expanded.insert(path.clone());
                if self.state.dropbox.folders.contains_key(&path)
                    || self.state.dropbox.listing_in_progress.contains(&path)
                {
                    return Task::none();
                }
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.dropbox.last_error = Some("Not connected to Dropbox".into());
                    return Task::none();
                };
                self.state.dropbox.listing_in_progress.insert(path.clone());
                return Task::perform(
                    list_dropbox_folder_async(client, path),
                    |result| match result {
                        Ok((path, entries)) => Message::DropboxFolderListed {
                            path,
                            result: Ok(entries),
                        },
                        Err(err) => Message::DropboxFolderListed {
                            path: String::new(),
                            result: Err(err),
                        },
                    },
                );
            }
            Message::DropboxCollapseFolder(path) => {
                self.state.dropbox.expanded.remove(&path);
            }
            Message::DropboxFolderListed { path, result } => {
                self.state.dropbox.listing_in_progress.remove(&path);
                match result {
                    Ok(entries) => {
                        self.state.dropbox.folders.insert(path, entries);
                    }
                    Err(err) => {
                        self.state.dropbox.last_error = Some(err.clone());
                        self.state.status_text = format!("Dropbox error: {err}");
                    }
                }
            }
            Message::DropboxSelectEntry(entry) => {
                self.state.dropbox.selected_path = Some(entry.path_lower.clone());
                self.state.sample_browser_selected_source = Some(MediaSourceRef::DropboxFile {
                    path_lower: entry.path_lower,
                    display_path: entry.path_display,
                    rev: entry.rev,
                });
            }
            Message::DropboxPreview(entry) => {
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.dropbox.last_error = Some("Not connected to Dropbox".into());
                    return Task::none();
                };
                let cache = self.dropbox_cache.clone();
                self.state.dropbox.preview_in_progress = true;
                self.state.status_text = format!("Fetching preview: {}", entry.name);
                return Task::perform(fetch_dropbox_sample_async(client, cache, entry), |result| {
                    Message::DropboxPreviewReady(result.map(|(audio, _, _)| audio))
                });
            }
            Message::DropboxPreviewReady(Ok(audio)) => {
                self.state.dropbox.preview_in_progress = false;
                self.send_command(EngineCommand::StartPreview(audio));
                self.state.status_text = "Preview playing".to_string();
            }
            Message::DropboxPreviewReady(Err(err)) => {
                self.state.dropbox.preview_in_progress = false;
                self.state.dropbox.last_error = Some(err.clone());
                self.state.status_text = format!("Preview error: {err}");
            }
            Message::DropboxImportToArrangement(entry) => {
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.dropbox.last_error = Some("Not connected to Dropbox".into());
                    return Task::none();
                };
                let cache = self.dropbox_cache.clone();
                let target = BrowserImportTarget::ArrangementClip(self.state.selected_track);
                self.state.status_text = format!("Importing {}...", entry.name);
                return Task::perform(
                    fetch_dropbox_sample_async(client, cache, entry),
                    move |result| match result {
                        Ok((audio, name, source)) => {
                            Message::BrowserSampleDecoded(target.clone(), audio, name, source)
                        }
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                );
            }
            Message::DropboxImportToDevice(entry) => {
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.dropbox.last_error = Some("Not connected to Dropbox".into());
                    return Task::none();
                };
                let Some(target) = self.selected_browser_device_target() else {
                    self.state.status_text = "Select a Sampler or Drum Pad track first".into();
                    return Task::none();
                };
                let cache = self.dropbox_cache.clone();
                self.state.status_text = format!("Importing {}...", entry.name);
                return Task::perform(
                    fetch_dropbox_sample_async(client, cache, entry),
                    move |result| match result {
                        Ok((audio, name, source)) => {
                            Message::BrowserSampleDecoded(target.clone(), audio, name, source)
                        }
                        Err(err) => Message::BrowserSampleDecodeError(err),
                    },
                );
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
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
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
        while let Ok(mut result) = self.plugin_effect_rx.try_recv() {
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
                    Ok(mut clap_inst) => {
                        if let Some(ref data) = result.pending_state {
                            use vibez_plugin_host::PluginInstance;
                            if !clap_inst.load_state(data) {
                                eprintln!("vibez: {plugin_name} rejected saved state");
                            }
                        }
                        let raw_ptr = Some(PluginRawPtr::Clap(
                            clap_inst.plugin_ptr() as *const std::ffi::c_void,
                        ));
                        result.state_ptr = Some(vibez_plugin_host::PluginStatePtr::Clap(
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
            } else if let Some(partial) = result.vst3_partial.take() {
                match vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::init_on_main_thread(
                    partial,
                    result.sample_rate,
                    4096,
                ) {
                    Ok(mut vst3_inst) => {
                        if let Some(ref data) = result.pending_state {
                            use vibez_plugin_host::PluginInstance;
                            if !vst3_inst.load_state(data) {
                                eprintln!("vibez: {plugin_name} rejected saved state");
                            }
                        }
                        let ctrl = vst3_inst.controller_ptr();
                        let raw_ptr = if ctrl.is_null() {
                            None
                        } else {
                            Some(PluginRawPtr::Vst3(ctrl))
                        };
                        result.state_ptr =
                            Some(vibez_plugin_host::PluginStatePtr::Vst3Component(
                                vst3_inst.component_ptr(),
                            ));
                        let wrapper =
                            vibez_plugin_host::PluginEffectWrapper::new(Box::new(vst3_inst));
                        (Box::new(wrapper), raw_ptr)
                    }
                    Err(e) => {
                        eprintln!("vibez: VST3 init failed on UI thread: {e}");
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
            if let Some(state_ptr) = result.state_ptr {
                let key = PluginGuiKey::Effect {
                    track_id,
                    effect_id,
                };
                self.plugin_state_ptrs.insert(key, state_ptr);
            }

            if let Some(track) = self.state.find_track_mut(track_id) {
                let descriptors: &'static [vibez_core::effect::ParamDescriptor] =
                    Box::leak(Vec::new().into_boxed_slice());
                let ui_effect = UiEffect {
                    id: effect_id,
                    effect_type: EffectType::Gain,
                    bypass: false,
                    params: Vec::new(),
                    descriptors,
                    plugin_name: Some(plugin_name.clone()),
                    has_plugin_gui: has_gui,
                    plugin_ref: Some(result.device_ref.clone()),
                };
                match result.position {
                    Some(pos) if pos < track.effects.len() => track.effects.insert(pos, ui_effect),
                    _ => track.effects.push(ui_effect),
                }
            }
            self.send_command(EngineCommand::AddPluginEffect {
                track_id,
                effect_id,
                effect,
                position: result.position,
            });
            self.state.status_text = format!("Loaded {plugin_name}");
        }

        // Poll for loaded plugin instruments
        while let Ok(mut result) = self.plugin_instrument_rx.try_recv() {
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
                    Ok(mut clap_inst) => {
                        if let Some(ref data) = result.pending_state {
                            use vibez_plugin_host::PluginInstance;
                            if !clap_inst.load_state(data) {
                                eprintln!("vibez: {plugin_name} rejected saved state");
                            }
                        }
                        let raw_ptr = Some(PluginRawPtr::Clap(
                            clap_inst.plugin_ptr() as *const std::ffi::c_void,
                        ));
                        result.state_ptr = Some(vibez_plugin_host::PluginStatePtr::Clap(
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
            } else if let Some(partial) = result.vst3_partial.take() {
                match vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::init_on_main_thread(
                    partial,
                    result.sample_rate,
                    4096,
                ) {
                    Ok(mut vst3_inst) => {
                        if let Some(ref data) = result.pending_state {
                            use vibez_plugin_host::PluginInstance;
                            if !vst3_inst.load_state(data) {
                                eprintln!("vibez: {plugin_name} rejected saved state");
                            }
                        }
                        let ctrl = vst3_inst.controller_ptr();
                        let raw_ptr = if ctrl.is_null() {
                            None
                        } else {
                            Some(PluginRawPtr::Vst3(ctrl))
                        };
                        result.state_ptr =
                            Some(vibez_plugin_host::PluginStatePtr::Vst3Component(
                                vst3_inst.component_ptr(),
                            ));
                        let wrapper =
                            vibez_plugin_host::PluginInstrumentWrapper::new(Box::new(vst3_inst));
                        (Box::new(wrapper), raw_ptr)
                    }
                    Err(e) => {
                        eprintln!("vibez: VST3 instrument init failed on UI thread: {e}");
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
            if let Some(state_ptr) = result.state_ptr {
                let key = PluginGuiKey::Instrument { track_id };
                self.plugin_state_ptrs.insert(key, state_ptr);
            }

            if let Some(track) = self.state.find_track_mut(track_id) {
                track.has_instrument = true;
                track.plugin_instrument_name = Some(plugin_name.clone());
                track.plugin_instrument_ref = Some(result.device_ref.clone());
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

    /// Drain pending MIDI events from the external input port and
    /// forward them to the engine. Events route to the currently-
    /// selected track's instrument; if nothing is selected or the
    /// track has no instrument attached, events are dropped (no
    /// passthrough). Called on every UI tick.
    fn poll_midi_input(&mut self) {
        let Some(handle) = self.midi_input.as_ref() else {
            return;
        };
        let mut events = Vec::new();
        while let Ok(event) = handle.rx.try_recv() {
            events.push(event);
        }
        if events.is_empty() {
            return;
        }
        let Some(track_id) = self.state.selected_track else {
            return;
        };
        let has_instrument = self
            .state
            .find_track(track_id)
            .map(|t| t.instrument_kind.is_some())
            .unwrap_or(false);
        if !has_instrument {
            return;
        }
        for event in events {
            match event {
                vibez_audio_io::midi_input::MidiEvent::NoteOn { pitch, velocity } => {
                    self.send_command(EngineCommand::ExternalNoteOn {
                        track_id,
                        pitch,
                        velocity,
                    });
                }
                vibez_audio_io::midi_input::MidiEvent::NoteOff { pitch } => {
                    self.send_command(EngineCommand::ExternalNoteOff { track_id, pitch });
                }
                vibez_audio_io::midi_input::MidiEvent::ControlChange { .. } => {
                    // CC mapping not wired yet.
                }
            }
        }
    }

    fn poll_engine_events(&mut self) {
        if let Some(ref mut rx) = self.event_rx {
            while let Ok(event) = rx.pop() {
                match event {
                    EngineEvent::DisposeEffect(cell) => {
                        // Plugin teardown on the UI thread: dlclose,
                        // COM release, JUCE MessageManager access.
                        drop(cell.take());
                    }
                    EngineEvent::DisposeInstrument(cell) => {
                        drop(cell.take());
                    }
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

        let workspace_content = match self.state.workspace {
            Workspace::Arrange => self.view_arrangement(),
            Workspace::Mix => self.view_mixer(),
        };
        let content: Element<'_, Message> =
            if self.state.workspace == Workspace::Arrange && self.state.sample_browser_open {
                row![self.view_sample_browser_panel(), workspace_content]
                    .height(Length::FillPortion(5))
                    .into()
            } else {
                workspace_content
            };

        let detail_panel = self.view_detail_panel();
        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![header, transport_bar, content, detail_panel, status_bar];

        let layout_container = container(layout).width(Length::Fill).height(Length::Fill);
        // Outer mouse_area cancels an active sample-drag on any release
        // that wasn't captured by a drop target (clip canvas, drum pad).
        let base_layout: Element<'_, Message> = mouse_area(layout_container)
            .on_release(Message::EndDragSample)
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
        let make_menu_btn = |label: &'static str, icon: char, msg: Message| {
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
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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

        let new_btn = make_menu_btn("New (Empty)", icons::PLUS, Message::NewProject);
        let export_btn = make_menu_btn(
            "Export to WAV...",
            icons::AUDIO_WAVEFORM,
            Message::ExportProject,
        );

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
                icons::icon(icons::SLIDERS_VERTICAL)
                    .size(12)
                    .color(th::TEXT),
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
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT,
                border: iced::Border::default(),
                ..Default::default()
            }
        });

        let menu_content = column![new_btn]
            .spacing(2)
            .push(open_btn)
            .push(save_btn)
            .push(save_as_btn)
            .push(export_btn)
            .push(settings_btn)
            .padding(4)
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

        // Position below the header, near the File button
        let padded = column![
            vertical_space().height(Length::Fixed(42.0)),
            row![horizontal_space().width(Length::Fixed(60.0)), menu_card,]
        ];

        mouse_area(container(padded).width(Length::Fill).height(Length::Fill))
            .on_press(Message::DismissFileMenu)
            .into()
    }

    fn view_settings_modal(&self) -> Element<'_, Message> {
        let title = text("Settings").size(18).color(th::ACCENT);
        let close_btn = button(icons::icon(icons::X).size(14).color(th::TEXT_DIM))
            .on_press(Message::CloseSettings)
            .padding([4, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        let make_tab_btn = |label: &'static str, tab: SettingsTab, is_active: bool| {
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
            make_tab_btn(
                "Plugins",
                SettingsTab::Plugins,
                active == SettingsTab::Plugins
            ),
            make_tab_btn(
                "Dropbox",
                SettingsTab::Dropbox,
                active == SettingsTab::Dropbox
            ),
            make_tab_btn(
                "Warping",
                SettingsTab::Warping,
                active == SettingsTab::Warping
            ),
        ]
        .spacing(0);

        // -- Tab body --
        let tab_body: Element<'_, Message> = match self.state.settings_tab {
            SettingsTab::Audio => self.view_settings_audio_tab(),
            SettingsTab::Plugins => self.view_settings_plugins_tab(),
            SettingsTab::Dropbox => self.view_settings_dropbox_tab(),
            SettingsTab::Warping => self.view_settings_warping_tab(),
        };

        let content = column![
            header,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            tab_bar,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
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
            container(center(dialog).width(Length::Fill).height(Length::Fill))
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

        // ---- MIDI input picker ----
        let midi_label = text("MIDI Input").size(14).color(th::TEXT);
        let midi_hint = text(
            "External MIDI routes to the currently selected instrument track. \
             Plug your keyboard or Push in, hit Rescan, then pick the port.",
        )
        .size(11)
        .color(th::TEXT_DIM);

        let current_port_line: Element<'_, Message> = match self.midi_input.as_ref() {
            Some(h) => text(format!("Connected: {}", h.port_name))
                .size(12)
                .color(th::ACCENT)
                .into(),
            None => text("Not connected").size(12).color(th::TEXT_DIM).into(),
        };

        let rescan_btn = button(text("Rescan ports").size(11).color(th::TEXT))
            .on_press(Message::RescanMidiInputs)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
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

        let disconnect_btn = button(text("Disconnect").size(11).color(th::TEXT_DIM))
            .on_press(Message::CloseMidiInput)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
            });

        let midi_actions = row![rescan_btn, disconnect_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        let mut port_list = column![].spacing(3);
        for name in &self.midi_input_ports {
            let is_current = self
                .midi_input
                .as_ref()
                .map(|h| h.port_name == *name)
                .unwrap_or(false);
            let label = name.clone();
            let port_btn = button(
                text(if is_current {
                    format!("● {name}")
                } else {
                    name.clone()
                })
                .size(11)
                .color(if is_current { th::ACCENT } else { th::TEXT }),
            )
            .on_press(Message::OpenMidiInput(label))
            .padding([4, 10])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if is_current {
                    Some(th::BG_HOVER.into())
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
                    text_color: if is_current { th::ACCENT } else { th::TEXT },
                    border: iced::Border {
                        color: if is_current {
                            th::ACCENT_DIM
                        } else {
                            th::BORDER
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
            port_list = port_list.push(port_btn);
        }

        column![
            buf_label,
            buf_hint,
            buf_row,
            sr_label,
            sr_value,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            midi_label,
            midi_hint,
            current_port_line,
            midi_actions,
            port_list,
        ]
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
        for (i, path) in self
            .state
            .plugin_settings
            .extra_scan_paths
            .iter()
            .enumerate()
        {
            let path_text = text(path.display().to_string())
                .size(11)
                .color(th::TEXT_DIM);
            let remove_btn = button(icons::icon(icons::X).size(10).color(th::DANGER))
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
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        let scan_btn = button(text(scan_label).size(12).color(th::TEXT))
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
            text(&self.state.plugin_scan_status)
                .size(11)
                .color(th::TEXT_DIM)
        } else {
            text(format!("{cache_count} plugins cached"))
                .size(11)
                .color(th::TEXT_DIM)
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

    fn view_settings_dropbox_tab(&self) -> Element<'_, Message> {
        let title = text("Dropbox").size(14).color(th::TEXT);
        let hint = text(
            "Register an app at https://www.dropbox.com/developers/apps \
            (Scoped access, Full Dropbox). Paste the App key below.",
        )
        .size(11)
        .color(th::TEXT_DIM);

        let app_key_input = text_input("App key", &self.state.dropbox.app_key_input)
            .on_input(Message::SetDropboxAppKey)
            .on_submit(Message::SaveDropboxAppKey)
            .size(13)
            .width(Length::Fill);
        let save_key_btn = button(text("Save").size(12).color(th::TEXT))
            .on_press(Message::SaveDropboxAppKey)
            .padding([6, 12])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT,
                    border: iced::Border {
                        color: th::ACCENT_DIM,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let key_row = row![app_key_input, save_key_btn]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let account_line: Element<'_, Message> = if self.state.dropbox.connected {
            let email = self
                .state
                .dropbox
                .account_email
                .clone()
                .unwrap_or_else(|| "connected".into());
            text(format!("Connected: {email}"))
                .size(12)
                .color(th::ACCENT)
                .into()
        } else if self.state.dropbox.auth_in_progress {
            text("Waiting for browser authorisation...")
                .size(12)
                .color(th::TEXT_DIM)
                .into()
        } else {
            text("Not connected").size(12).color(th::TEXT_DIM).into()
        };

        let can_connect = self.state.dropbox.has_app_key && !self.state.dropbox.auth_in_progress;
        let connect_label = if self.state.dropbox.auth_in_progress {
            "Connecting..."
        } else if self.state.dropbox.connected {
            "Reconnect"
        } else {
            "Connect"
        };
        let connect_btn = {
            let mut btn = button(text(connect_label).size(12).color(th::ACCENT));
            if can_connect {
                btn = btn.on_press(Message::ConnectDropbox);
            }
            btn.padding([6, 12]).style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
            })
        };

        let disconnect_btn: Element<'_, Message> = if self.state.dropbox.connected {
            button(text("Disconnect").size(12).color(th::TEXT_DIM))
                .on_press(Message::DisconnectDropbox)
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
                        text_color: th::TEXT_DIM,
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
                .into()
        } else {
            horizontal_space().width(Length::Shrink).into()
        };

        let error_line: Element<'_, Message> =
            if let Some(err) = self.state.dropbox.last_error.clone() {
                text(err).size(11).color(th::DANGER).into()
            } else {
                horizontal_space().width(Length::Shrink).into()
            };

        column![
            title,
            hint,
            key_row,
            account_line,
            row![connect_btn, disconnect_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            error_line,
        ]
        .spacing(10)
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

        mouse_area(centered).on_press(Message::CancelEditing).into()
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

                // Bounce to audio
                col = col.push(menu_btn(
                    icons::AUDIO_WAVEFORM,
                    "Bounce to Audio".into(),
                    Message::BounceClipToAudio {
                        track_id,
                        clip_id,
                        is_note_clip,
                    },
                ));

                // Quantize (grid follows the snap setting)
                if is_note_clip {
                    col = col.push(menu_btn(
                        icons::CIRCLE_DOT,
                        format!("Quantize ({})", self.state.snap_grid.label()),
                        Message::QuantizeNoteClip { track_id, clip_id },
                    ));
                } else {
                    col = col.push(menu_btn(
                        icons::CIRCLE_DOT,
                        format!("Quantize ({})", self.state.snap_grid.label()),
                        Message::QuantizeAudioClip { track_id, clip_id },
                    ));
                }

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
                        track_id: *track_id,
                    },
                ));
                col = col.push(menu_btn(
                    icons::TRASH_2,
                    "Delete Clips in Region".into(),
                    Message::DeleteClipsInRegion {
                        start_beats: start,
                        end_beats: end,
                        track_id: *track_id,
                    },
                ));
                col = col.push(menu_btn(
                    icons::REPEAT,
                    "Set as Loop Region".into(),
                    Message::SetSelectionAsLoop,
                ));
                col = col.push(menu_btn(
                    icons::AUDIO_WAVEFORM,
                    "Bounce Selection".into(),
                    Message::BounceSelectionToAudio,
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

        let file_btn = button(text("File").size(13).color(th::TEXT_DIM))
            .on_press(Message::ToggleFileMenu)
            .padding([6, 14])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::TEXT_DIM,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        let browser_active = self.state.sample_browser_open;
        let browser_btn = button(
            row![
                icons::icon(icons::AUDIO_WAVEFORM)
                    .size(13)
                    .color(if browser_active {
                        th::ACCENT
                    } else {
                        th::TEXT_DIM
                    }),
                text("Browser").size(13).color(if browser_active {
                    th::ACCENT
                } else {
                    th::TEXT_DIM
                })
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleSampleBrowser)
        .padding([6, 14])
        .style(move |_theme: &Theme, status| {
            let bg = if browser_active {
                Some(th::BG_ELEVATED.into())
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => None,
                }
            };
            button::Style {
                background: bg,
                text_color: if browser_active {
                    th::ACCENT
                } else {
                    th::TEXT_DIM
                },
                border: iced::Border {
                    color: if browser_active {
                        th::ACCENT_DIM
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if browser_active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        let header_row = row![title, file_btn, browser_btn, tabs, horizontal_space()].spacing(8);

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

    fn view_sample_browser_panel(&self) -> Element<'_, Message> {
        let tab_bar = {
            let local_active = matches!(
                self.state.sample_browser_mode,
                crate::state::SampleBrowserMode::Local
            );
            let dropbox_active = !local_active;
            let tab_btn = |label: &'static str, active: bool, mode| {
                button(
                    text(label)
                        .size(11)
                        .color(if active { th::ACCENT } else { th::TEXT_DIM }),
                )
                .on_press(Message::SetSampleBrowserMode(mode))
                .padding([4, 12])
                .style(move |_theme: &Theme, status| {
                    let bg = if active {
                        Some(th::ACCENT_DIM.into())
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
                        text_color: if active { th::ACCENT } else { th::TEXT_DIM },
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
            };
            row![
                tab_btn(
                    "Local",
                    local_active,
                    crate::state::SampleBrowserMode::Local
                ),
                tab_btn(
                    "Dropbox",
                    dropbox_active,
                    crate::state::SampleBrowserMode::Dropbox,
                ),
            ]
            .spacing(0)
        };

        let body: Element<'_, Message> = match self.state.sample_browser_mode {
            crate::state::SampleBrowserMode::Local => self.view_local_sample_browser(),
            crate::state::SampleBrowserMode::Dropbox => self.view_dropbox_browser(),
        };

        container(
            column![tab_bar, body]
                .spacing(4)
                .padding([4, 0])
                .height(Length::Fill),
        )
        .width(Length::Fixed(320.0))
        .height(Length::Fill)
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

    fn view_settings_warping_tab(&self) -> Element<'_, Message> {
        let title = text("Sample Warping").size(14).color(th::TEXT);
        let hint = text(
            "Auto-warp detects BPM of each dropped sample and time-stretches it to \
             the project tempo, preserving pitch. Turn this off to keep samples at their \
             original speed.",
        )
        .size(11)
        .color(th::TEXT_DIM);

        let toggle_icon = if self.state.auto_warp_on_import {
            icons::icon(icons::CIRCLE_DOT).size(12).color(th::ACCENT)
        } else {
            icons::icon(icons::CIRCLE).size(12).color(th::TEXT_DIM)
        };
        let toggle_btn = button(
            row![
                toggle_icon,
                text("Auto-warp samples on import").size(12).color(th::TEXT)
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::ToggleAutoWarpOnImport)
        .padding([4, 8])
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            text_color: th::TEXT,
            border: iced::Border::default(),
            ..Default::default()
        });

        let conf = self.state.warp_confidence_threshold;
        let conf_label = text("Detection confidence threshold")
            .size(12)
            .color(th::TEXT);
        let conf_value = text(format!("{:.2}", conf)).size(12).color(th::TEXT_DIM);
        let conf_hint = text(
            "Higher = only warp when the detector is very sure. \
             Lower = warp even ambiguous clips.",
        )
        .size(11)
        .color(th::TEXT_DIM);
        let conf_slider = slider(0.0..=1.0, conf, Message::SetWarpConfidenceThreshold).step(0.05);

        let rewarp_btn = button(
            text("Re-warp all clips to project tempo")
                .size(12)
                .color(th::TEXT),
        )
        .on_press(Message::RewarpAllClips)
        .padding([6, 12])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::TEXT,
                border: iced::Border {
                    color: th::ACCENT_DIM,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        column![
            title,
            hint,
            toggle_btn,
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            conf_label,
            conf_hint,
            row![conf_slider, conf_value]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            container(column![].height(Length::Fixed(1.0)).width(Length::Fill)).style(
                |_theme: &Theme| container::Style {
                    background: Some(th::BORDER.into()),
                    ..Default::default()
                }
            ),
            rewarp_btn,
        ]
        .spacing(10)
        .into()
    }

    fn view_local_sample_browser(&self) -> Element<'_, Message> {
        let title = text("Sample Browser").size(14).color(th::ACCENT);
        let mut add_root_btn = button(text("Add Root").size(11).color(th::TEXT))
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        add_root_btn = add_root_btn.on_press(Message::AddSampleLibraryRoot);

        let mut rescan_btn = button(text("Rescan").size(11).color(th::TEXT))
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        if !self.state.sample_browser_roots.is_empty()
            && !self.state.sample_browser_scan_in_progress
        {
            rescan_btn = rescan_btn.on_press(Message::RescanSampleLibrary);
        }

        let header = row![title, horizontal_space(), add_root_btn, rescan_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        if self.state.sample_browser_roots.is_empty() {
            let empty = column![
                header,
                text("Add a root folder to index your sample library.")
                    .size(12)
                    .color(th::TEXT_DIM)
            ]
            .spacing(10)
            .padding(10);
            return container(empty)
                .width(Length::Fixed(320.0))
                .height(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::BG_SURFACE.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                })
                .into();
        }

        let root_label = |path: &PathBuf| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string())
        };

        let mut roots_col = column![].spacing(4);
        let all_active = self.state.sample_browser_root_filter.is_none();
        let mut all_btn = button(text("All Roots").size(11).color(if all_active {
            th::ACCENT
        } else {
            th::TEXT_DIM
        }))
        .padding([4, 8])
        .style(move |_theme: &Theme, status| {
            let bg = if all_active {
                Some(th::ACCENT_DIM.into())
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => Some(th::BG_ELEVATED.into()),
                }
            };
            button::Style {
                background: bg,
                text_color: if all_active { th::ACCENT } else { th::TEXT_DIM },
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });
        all_btn = all_btn.on_press(Message::SelectSampleBrowserRoot(None));
        roots_col = roots_col.push(all_btn);

        for root in &self.state.sample_browser_roots {
            let active = self
                .state
                .sample_browser_root_filter
                .as_ref()
                .is_some_and(|selected| selected == root);
            let mut filter_btn = button(text(root_label(root)).size(11).color(if active {
                th::ACCENT
            } else {
                th::TEXT
            }))
            .padding([4, 8])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if active {
                    Some(th::ACCENT_DIM.into())
                } else {
                    match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => Some(th::BG_ELEVATED.into()),
                    }
                };
                button::Style {
                    background: bg,
                    text_color: if active { th::ACCENT } else { th::TEXT },
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
            filter_btn = filter_btn.on_press(Message::SelectSampleBrowserRoot(Some(root.clone())));

            let remove_btn = button(icons::icon(icons::X).size(10).color(th::DANGER))
                .on_press(Message::RemoveSampleLibraryRoot(root.clone()))
                .padding([3, 6])
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

            roots_col = roots_col.push(
                row![filter_btn, remove_btn]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            );
        }

        let search = text_input("Search samples...", &self.state.sample_browser_search)
            .on_input(Message::SampleBrowserSearchChanged)
            .size(12)
            .width(Length::Fill);

        let search_lower = self.state.sample_browser_search.to_lowercase();
        let mut filtered_entries: Vec<&SampleBrowserEntry> = self
            .state
            .sample_browser_entries
            .iter()
            .filter(|entry| {
                self.state
                    .sample_browser_root_filter
                    .as_ref()
                    .is_none_or(|root| &entry.root_path == root)
            })
            .filter(|entry| search_lower.is_empty() || entry.search_text.contains(&search_lower))
            .collect();
        filtered_entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        let selected_source = self.state.sample_browser_selected_source.as_ref();
        let selected_entry = self.selected_sample_browser_entry();
        let selected_target = self.selected_browser_device_target();

        let mut entries_col = column![].spacing(2);
        for entry in filtered_entries.iter().take(400) {
            let selected = selected_source.is_some_and(|source| &entry.source == source);
            // mouse_area returns early if its child captures the event, so
            // iced Button underneath would swallow press events. Use a
            // plain container as the click target instead.
            let entry_body = container(
                column![
                    text(entry.name.as_str()).size(12).color(if selected {
                        th::ACCENT
                    } else {
                        th::TEXT
                    }),
                    text(entry.relative_path.display().to_string())
                        .size(10)
                        .color(th::TEXT_DIM)
                ]
                .spacing(2)
                .width(Length::Fill),
            )
            .padding([6, 8])
            .width(Length::Fill)
            .style(move |_theme: &Theme| container::Style {
                background: Some(
                    if selected {
                        th::ACCENT_DIM
                    } else {
                        th::BG_ELEVATED
                    }
                    .into(),
                ),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });
            let entry_dragger: Element<'_, Message> = mouse_area(entry_body)
                .on_press(Message::StartDragSample {
                    source: entry.source.clone(),
                    label: entry.name.clone(),
                })
                .on_release(Message::ClickLocalBrowserEntry(entry.source.clone()))
                .into();
            let preview_btn = button(icons::icon(icons::VOLUME_2).size(12).color(th::TEXT_DIM))
                .on_press(Message::PreviewLocalEntry(entry.source.clone()))
                .padding([6, 8])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
                        }
                        _ => Some(th::BG_ELEVATED.into()),
                    };
                    button::Style {
                        background: bg,
                        text_color: th::ACCENT,
                        border: iced::Border {
                            color: th::BORDER,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                });
            entries_col = entries_col.push(
                row![entry_dragger, preview_btn]
                    .spacing(4)
                    .align_y(iced::Alignment::Center),
            );
        }

        if filtered_entries.is_empty() {
            entries_col = entries_col.push(
                container(
                    text("No samples match the current filters")
                        .size(11)
                        .color(th::TEXT_DIM),
                )
                .padding([8, 4]),
            );
        }

        let count_label = text(format!(
            "{} shown / {} indexed{}",
            filtered_entries.len().min(400),
            self.state.sample_browser_entries.len(),
            if self.state.sample_browser_scan_in_progress {
                " (scanning...)"
            } else {
                ""
            }
        ))
        .size(10)
        .color(th::TEXT_DIM);

        let selected_text = selected_entry
            .map(|entry| entry.relative_path.display().to_string())
            .unwrap_or_else(|| "Select a sample".to_string());
        let selected_hint = match selected_target {
            Some(BrowserImportTarget::Sampler(track_id)) => self
                .state
                .find_track(track_id)
                .map(|track| format!("Load to {}", track.name))
                .unwrap_or_else(|| "Load to sampler".to_string()),
            Some(BrowserImportTarget::DrumRackPad {
                track_id,
                pad_index,
            }) => self
                .state
                .find_track(track_id)
                .map(|track| format!("Load to {} pad {}", track.name, pad_index + 1))
                .unwrap_or_else(|| "Load to drum rack".to_string()),
            _ => "No sampler or drum rack selected".to_string(),
        };

        let mut add_clip_btn = button(text("Add Clip").size(11).color(th::TEXT))
            .padding([6, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        if selected_entry.is_some() {
            add_clip_btn = add_clip_btn.on_press(Message::ImportSelectedBrowserSampleToArrangement);
        }

        let mut load_device_btn = button(text("Load Device").size(11).color(th::TEXT))
            .padding([6, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        if selected_entry.is_some() && selected_target.is_some() {
            load_device_btn = load_device_btn.on_press(Message::LoadSelectedBrowserSampleToDevice);
        }

        let footer = column![
            text(selected_text).size(11).color(th::TEXT),
            text(selected_hint).size(10).color(th::TEXT_DIM),
            row![add_clip_btn, load_device_btn]
                .spacing(6)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(6);

        column![
            header,
            roots_col,
            search,
            count_label,
            scrollable(entries_col).height(Length::Fill).direction(
                scrollable::Direction::Vertical(scrollable::Scrollbar::default())
            ),
            footer
        ]
        .spacing(8)
        .padding(10)
        .height(Length::Fill)
        .into()
    }

    fn view_dropbox_browser(&self) -> Element<'_, Message> {
        let title = text("Dropbox").size(14).color(th::ACCENT);

        if !self.state.dropbox.connected {
            let hint = if self.state.dropbox.auth_in_progress {
                "Waiting for browser authorisation..."
            } else {
                "Connect in Settings > Dropbox to browse your library."
            };
            return column![title, text(hint).size(12).color(th::TEXT_DIM)]
                .spacing(10)
                .padding(10)
                .height(Length::Fill)
                .into();
        }

        let account = self.state.dropbox.account_email.clone().unwrap_or_default();
        let header = column![title, text(account).size(11).color(th::TEXT_DIM),].spacing(2);

        let mut rows: Vec<Element<'_, Message>> = Vec::new();
        self.render_dropbox_tree(String::new(), 0, &mut rows);
        if rows.is_empty() {
            let msg = if self.state.dropbox.listing_in_progress.contains("") {
                "Listing your Dropbox..."
            } else {
                "Empty (or still fetching)."
            };
            rows.push(text(msg).size(11).color(th::TEXT_DIM).into());
        }
        let mut entries_col = column![].spacing(2);
        for row in rows {
            entries_col = entries_col.push(row);
        }

        let selected_entry = self.selected_dropbox_entry();
        let selected_label = selected_entry
            .as_ref()
            .map(|e| e.path_display.clone())
            .unwrap_or_else(|| "Select a file".to_string());

        // Preview is triggered by click-to-audition on the tree row itself;
        // no dedicated button here.

        let add_clip_btn: Element<'_, Message> = {
            let mut btn = button(text("Add Clip").size(11).color(th::TEXT))
                .padding([6, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
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
            if let Some(entry) = selected_entry.as_ref().filter(|e| e.is_supported_audio()) {
                btn = btn.on_press(Message::DropboxImportToArrangement(entry.clone()));
            }
            btn.into()
        };

        let load_device_btn: Element<'_, Message> = {
            let mut btn = button(text("Load Device").size(11).color(th::TEXT))
                .padding([6, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
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
            if let (Some(entry), Some(_)) = (
                selected_entry.as_ref().filter(|e| e.is_supported_audio()),
                self.selected_browser_device_target(),
            ) {
                btn = btn.on_press(Message::DropboxImportToDevice(entry.clone()));
            }
            btn.into()
        };

        let error_line: Element<'_, Message> =
            if let Some(err) = self.state.dropbox.last_error.clone() {
                text(err).size(10).color(th::DANGER).into()
            } else {
                horizontal_space().width(Length::Shrink).into()
            };

        let footer = column![
            text(selected_label).size(11).color(th::TEXT),
            row![add_clip_btn, load_device_btn].spacing(6),
            error_line,
        ]
        .spacing(6);

        column![
            header,
            scrollable(entries_col).height(Length::Fill).direction(
                scrollable::Direction::Vertical(scrollable::Scrollbar::default())
            ),
            footer,
        ]
        .spacing(8)
        .padding(10)
        .height(Length::Fill)
        .into()
    }

    fn render_dropbox_tree(
        &self,
        path: String,
        depth: usize,
        rows: &mut Vec<Element<'_, Message>>,
    ) {
        let Some(entries) = self.state.dropbox.folders.get(&path) else {
            return;
        };
        let mut sorted: Vec<&DropboxEntry> = entries.iter().collect();
        sorted.sort_by(|a, b| {
            (!a.is_folder, a.name.to_lowercase()).cmp(&(!b.is_folder, b.name.to_lowercase()))
        });
        for entry in sorted {
            let expanded = self.state.dropbox.expanded.contains(&entry.path_lower);
            let selected = self.state.dropbox.selected_path.as_deref() == Some(&entry.path_lower);

            let prefix = if entry.is_folder {
                if expanded {
                    "v "
                } else {
                    "> "
                }
            } else if entry.is_supported_audio() {
                "· "
            } else {
                "  "
            };
            let indent = "  ".repeat(depth);
            let label = format!("{indent}{prefix}{}", entry.name);
            let msg = if entry.is_folder {
                if expanded {
                    Message::DropboxCollapseFolder(entry.path_lower.clone())
                } else {
                    Message::DropboxExpandFolder(entry.path_lower.clone())
                }
            } else {
                Message::DropboxSelectEntry(entry.clone())
            };
            if entry.is_supported_audio() {
                // Audio rows use a container + mouse_area so press events
                // reach us (iced Button captures ButtonPressed, which would
                // hide the drag from mouse_area).
                let text_color = if selected { th::ACCENT } else { th::TEXT };
                let row_body = container(text(label).size(11).color(text_color))
                    .padding([3, 6])
                    .width(Length::Fill)
                    .style(move |_theme: &Theme| container::Style {
                        background: Some(
                            if selected {
                                th::ACCENT_DIM
                            } else {
                                th::BG_ELEVATED
                            }
                            .into(),
                        ),
                        border: iced::Border::default(),
                        ..Default::default()
                    });
                let source = MediaSourceRef::DropboxFile {
                    path_lower: entry.path_lower.clone(),
                    display_path: entry.path_display.clone(),
                    rev: entry.rev.clone(),
                };
                let dragger: Element<'_, Message> = mouse_area(row_body)
                    .on_press(Message::StartDragSample {
                        source,
                        label: entry.name.clone(),
                    })
                    .on_release(msg)
                    .into();
                let speaker = button(icons::icon(icons::VOLUME_2).size(11).color(th::ACCENT))
                    .on_press(Message::DropboxPreview(entry.clone()))
                    .padding([3, 6])
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
                            border: iced::Border::default(),
                            ..Default::default()
                        }
                    });
                rows.push(
                    row![dragger, speaker]
                        .spacing(2)
                        .align_y(iced::Alignment::Center)
                        .into(),
                );
            } else {
                // Folders + non-audio entries keep the button path since they
                // don't participate in drag.
                let btn = button(text(label).size(11).color(if selected {
                    th::ACCENT
                } else if entry.is_folder {
                    th::TEXT
                } else {
                    th::TEXT_DIM
                }))
                .on_press(msg)
                .padding([3, 6])
                .width(Length::Fill)
                .style(move |_theme: &Theme, status| {
                    let bg = if selected {
                        Some(th::ACCENT_DIM.into())
                    } else {
                        match status {
                            button::Status::Hovered | button::Status::Pressed => {
                                Some(th::BG_HOVER.into())
                            }
                            _ => Some(th::BG_ELEVATED.into()),
                        }
                    };
                    button::Style {
                        background: bg,
                        text_color: if selected { th::ACCENT } else { th::TEXT },
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                });
                rows.push(btn.into());
            }

            if entry.is_folder && expanded {
                self.render_dropbox_tree(entry.path_lower.clone(), depth + 1, rows);
            }
        }
    }

    fn selected_dropbox_entry(&self) -> Option<DropboxEntry> {
        let selected = self.state.dropbox.selected_path.as_ref()?;
        for entries in self.state.dropbox.folders.values() {
            if let Some(entry) = entries.iter().find(|e| &e.path_lower == selected) {
                return Some(entry.clone());
            }
        }
        None
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
        let track_kinds: Vec<bool> = self.state.tracks.iter().map(|t| t.kind.is_midi()).collect();
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
                self.state.time_selection_track,
                self.state.drag_source.is_some(),
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
                                self.view_audio_clip_panel(track_id, clip, track_color)
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

        let hint_label = text("Right-click to add").size(10).color(th::TEXT_MUTED);

        let header = row![track_label, horizontal_space(), hint_label]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        // Device cards
        let mut devices_row = row![].spacing(6);

        // Instrument device card (branched by kind)
        if track.has_instrument {
            if track.plugin_instrument_name.is_some() {
                // External plugin instrument — clickable card
                let card = self.view_plugin_instrument_device(track_id, track, track_color);
                devices_row = devices_row.push(card);
            } else {
                match track.instrument_kind {
                    Some(vibez_core::midi::InstrumentKind::Sampler) => {
                        let card = self.view_sampler_device(track_id, track, track_color);
                        devices_row = devices_row.push(card);
                    }
                    Some(vibez_core::midi::InstrumentKind::DrumRack) => {
                        let card = self.view_drum_rack_device(track_id, track, track_color);
                        devices_row = devices_row.push(card);
                    }
                    _ => {
                        let synth_card = self.view_synth_device(track_id, track, track_color);
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
        let plugin_name = track.plugin_instrument_name.as_deref().unwrap_or("Plugin");

        let name_section =
            container(text(plugin_name).size(11).color(th::TEXT)).width(Length::Fill);

        // Edit button for plugins with a native GUI
        let edit_btn: Option<iced::widget::Button<'_, Message>> = if track.has_plugin_instrument_gui
        {
            let gui_key = PluginGuiKey::Instrument { track_id };
            Some(
                button(text("Edit").size(9).color(th::TEXT_DIM))
                    .on_press(Message::OpenPluginGui(gui_key))
                    .padding([2, 5])
                    .style(|_theme: &Theme, status| {
                        let (bg, tc) = match status {
                            button::Status::Hovered => (Some(th::BG_HOVER.into()), th::ACCENT),
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
    fn view_synth_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::EffectKnobWidget;
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text("Synth").size(11).color(th::TEXT);

        let title =
            Self::device_title_bar(row![dot, name].spacing(4).align_y(iced::Alignment::Center));

        let descriptors = vibez_instruments::synth::SYNTH_PARAMS;

        // Param 0 is the waveform: a selector reads better than a knob.
        let wave_value = track
            .instrument_params
            .first()
            .copied()
            .unwrap_or(descriptors[0].default)
            .round() as usize;
        let mut wave_row = row![].spacing(2);
        for (i, label) in ["Sin", "Saw", "Sqr", "Tri"].iter().enumerate() {
            let active = wave_value == i;
            let btn =
                button(
                    text(*label)
                        .size(9)
                        .color(if active { th::ACCENT } else { th::TEXT_DIM }),
                )
                .on_press(Message::SetInstrumentParam(track_id, 0, i as f32))
                .padding([2, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(if active { th::ACCENT_DIM } else { th::BG_DARK }.into()),
                    text_color: if active { th::ACCENT } else { th::TEXT_DIM },
                    border: iced::Border {
                        color: if active { th::ACCENT_DIM } else { th::BORDER },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            wave_row = wave_row.push(btn);
        }

        // Remaining params get real knobs, four per row.
        let mut param_rows = column![].spacing(6);
        let mut current_row = row![].spacing(8);
        let mut count = 0;
        for (i, descriptor) in descriptors.iter().enumerate().skip(1) {
            let value = track
                .instrument_params
                .get(i)
                .copied()
                .unwrap_or(descriptor.default);
            let knob = EffectKnobWidget::for_instrument(
                track_id,
                i,
                value,
                descriptor.min,
                descriptor.max,
                descriptor.default,
                track_color,
            );
            let knob_canvas: Element<'a, Message> = canvas(knob)
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .into();
            let label = text(descriptor.name).size(9).color(th::TEXT_DIM);
            let value_label = text(format!("{value:.2}{}", descriptor.unit))
                .size(8)
                .color(th::TEXT_MUTED);
            let param_col = column![knob_canvas, label, value_label]
                .spacing(1)
                .align_x(iced::Alignment::Center);
            current_row = current_row.push(param_col);
            count += 1;
            if count % 4 == 0 {
                param_rows = param_rows.push(current_row);
                current_row = row![].spacing(8);
            }
        }
        if count % 4 != 0 {
            param_rows = param_rows.push(current_row);
        }

        let body = container(column![wave_row, param_rows].spacing(6))
            .padding([6, 7])
            .width(Length::Fill);

        Self::device_card(column![title, body].width(Length::Fixed(260.0)))
    }

    /// Sampler device card.
    fn view_sampler_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::EffectKnobWidget;
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text("Sampler").size(11).color(th::TEXT);

        let title =
            Self::device_title_bar(row![dot, name].spacing(4).align_y(iced::Alignment::Center));

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

        let descriptors = vibez_instruments::sampler::SAMPLER_PARAMS;
        let mut param_rows = column![].spacing(6);
        let mut current_row = row![].spacing(8);
        let mut count = 0;
        for (i, descriptor) in descriptors.iter().enumerate() {
            let value = track
                .instrument_params
                .get(i)
                .copied()
                .unwrap_or(descriptor.default);
            let knob = EffectKnobWidget::for_instrument(
                track_id,
                i,
                value,
                descriptor.min,
                descriptor.max,
                descriptor.default,
                track_color,
            );
            let knob_canvas: Element<'a, Message> = canvas(knob)
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .into();
            let label = text(descriptor.name).size(9).color(th::TEXT_DIM);
            let value_label = text(format!("{value:.2}{}", descriptor.unit))
                .size(8)
                .color(th::TEXT_MUTED);
            let param_col = column![knob_canvas, label, value_label]
                .spacing(1)
                .align_x(iced::Alignment::Center);
            current_row = current_row.push(param_col);
            count += 1;
            if count % 4 == 0 {
                param_rows = param_rows.push(current_row);
                current_row = row![].spacing(8);
            }
        }
        if count % 4 != 0 {
            param_rows = param_rows.push(current_row);
        }

        let body = container(column![sample_row, param_rows].spacing(6))
            .padding([6, 7])
            .width(Length::Fill);

        Self::device_card(column![title, body].width(Length::Fixed(260.0)))
    }

    fn view_drum_rack_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::EffectKnobWidget;
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text("Drum Rack").size(11).color(th::TEXT);
        let selected_pad = track
            .selected_drum_pad
            .min(track.drum_rack_pads.len().saturating_sub(1));

        let remove: Element<'a, Message> = Self::device_icon_btn(
            icons::X,
            th::TEXT_DIM,
            th::DANGER,
            Message::RemoveTrackInstrument(track_id),
        )
        .into();

        let title = Self::device_title_bar(
            row![dot, name, horizontal_space(), remove]
                .spacing(4)
                .align_y(iced::Alignment::Center),
        );

        let mut grid = column![].spacing(4);
        for row_index in 0..4 {
            let mut pad_row = row![].spacing(4);
            for col_index in 0..4 {
                let pad_index = row_index * 4 + col_index;
                let pad = &track.drum_rack_pads[pad_index];
                let active = selected_pad == pad_index;
                let label = pad
                    .name
                    .as_deref()
                    .map(|name| {
                        if name.len() > 10 {
                            format!("{}...", &name[..10])
                        } else {
                            name.to_string()
                        }
                    })
                    .unwrap_or_else(|| format!("Pad {}", pad_index + 1));
                // Use container + mouse_area so press events reach us and
                // drag-drop works. iced Button would capture ButtonPressed
                // and hide it from mouse_area.
                let pad_note = crate::widgets::piano_roll::pitch_name(36 + pad_index as u8);
                let pad_body = container(
                    column![
                        text(format!("{:02}  {pad_note}", pad_index + 1))
                            .size(9)
                            .color(if active { th::ACCENT } else { th::TEXT_DIM }),
                        text(label)
                            .size(10)
                            .color(if active { th::ACCENT } else { th::TEXT })
                    ]
                    .spacing(2)
                    .align_x(iced::Alignment::Center),
                )
                .padding([8, 6])
                .width(Length::Fixed(60.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(if active { th::ACCENT_DIM } else { th::BG_DARK }.into()),
                    text_color: Some(if active { th::ACCENT } else { th::TEXT }),
                    border: iced::Border {
                        color: if active { th::ACCENT_DIM } else { th::BORDER },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
                // on_release handler selects the pad when no drag is active,
                // otherwise routes through DropSampleOnDrumPad.
                let pad_cell: Element<'a, Message> = mouse_area(pad_body)
                    .on_release(Message::DropSampleOnDrumPad {
                        track_id,
                        pad_index,
                    })
                    .into();
                pad_row = pad_row.push(pad_cell);
            }
            grid = grid.push(pad_row);
        }

        let selected_name = track.drum_rack_pads[selected_pad]
            .name
            .clone()
            .unwrap_or_else(|| "No sample loaded".to_string());
        let source_hint = track.drum_rack_pads[selected_pad]
            .source
            .as_ref()
            .map(MediaSourceRef::display_name)
            .unwrap_or_else(|| "Use the browser or Load".to_string());
        let selected_pad_state = &track.drum_rack_pads[selected_pad];

        let load_btn = button(text("Load").size(9).color(th::TEXT))
            .on_press(Message::LoadDrumRackPadSample(track_id, selected_pad))
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

        let clear_btn = button(text("Clear").size(9).color(th::TEXT_DIM))
            .on_press(Message::ClearDrumRackPad(track_id, selected_pad))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => Some(th::BG_DARK.into()),
                };
                button::Style {
                    background: bg,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::TEXT_DIM,
                    ..Default::default()
                }
            });

        let footer = column![
            text(selected_name).size(10).color(th::TEXT),
            text(source_hint).size(9).color(th::TEXT_DIM),
            row![load_btn, clear_btn]
                .spacing(6)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(4);

        let drum_params = [
            (
                "Gain",
                format!("{:.2}", selected_pad_state.gain),
                selected_pad_state.gain,
                0.0,
                2.0,
                1.0,
                DrumPadParam::Gain,
            ),
            (
                "Pan",
                format!("{:.2}", selected_pad_state.pan),
                selected_pad_state.pan,
                -1.0,
                1.0,
                0.0,
                DrumPadParam::Pan,
            ),
            (
                "Start",
                format!("{:.0}%", selected_pad_state.start * 100.0),
                selected_pad_state.start,
                0.0,
                1.0,
                0.0,
                DrumPadParam::Start,
            ),
            (
                "End",
                format!("{:.0}%", selected_pad_state.end * 100.0),
                selected_pad_state.end,
                0.0,
                1.0,
                1.0,
                DrumPadParam::End,
            ),
            (
                "Coarse",
                format!("{}st", selected_pad_state.coarse_tune),
                selected_pad_state.coarse_tune as f32,
                -24.0,
                24.0,
                0.0,
                DrumPadParam::CoarseTune,
            ),
            (
                "Fine",
                format!("{:.0}ct", selected_pad_state.fine_tune),
                selected_pad_state.fine_tune,
                -100.0,
                100.0,
                0.0,
                DrumPadParam::FineTune,
            ),
        ];

        let mut param_rows = column![].spacing(6);
        let mut current_row = row![].spacing(8);
        for (i, (label_text, value_text, value, min, max, default, param)) in
            drum_params.iter().enumerate()
        {
            let knob = EffectKnobWidget::for_drum_pad(
                track_id,
                selected_pad,
                *param,
                *value,
                *min,
                *max,
                *default,
                track_color,
            );
            let knob_canvas: Element<'a, Message> = canvas(knob)
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .into();
            let label = text(*label_text).size(9).color(th::TEXT_DIM);
            let value_label = text(value_text.clone()).size(8).color(th::TEXT_MUTED);
            let param_col = column![knob_canvas, label, value_label]
                .spacing(1)
                .align_x(iced::Alignment::Center);
            current_row = current_row.push(param_col);
            if (i + 1) % 3 == 0 {
                param_rows = param_rows.push(current_row);
                current_row = row![].spacing(8);
            }
        }

        let one_shot_active = selected_pad_state.one_shot;
        let one_shot_btn = button(text("One-shot").size(9).color(if one_shot_active {
            th::ACCENT
        } else {
            th::TEXT_DIM
        }))
        .on_press(Message::SetDrumPadOneShot {
            track_id,
            pad_index: selected_pad,
            one_shot: !one_shot_active,
        })
        .padding([2, 6])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(
                if one_shot_active {
                    th::ACCENT_DIM
                } else {
                    th::BG_DARK
                }
                .into(),
            ),
            text_color: if one_shot_active {
                th::ACCENT
            } else {
                th::TEXT_DIM
            },
            border: iced::Border {
                color: if one_shot_active {
                    th::ACCENT_DIM
                } else {
                    th::BORDER
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

        let mut choke_row = row![text("Choke").size(9).color(th::TEXT_DIM)]
            .spacing(2)
            .align_y(iced::Alignment::Center);
        for (group, label) in [
            (None, "Off"),
            (Some(1), "1"),
            (Some(2), "2"),
            (Some(3), "3"),
            (Some(4), "4"),
        ] {
            let active = selected_pad_state.choke_group == group;
            let btn =
                button(
                    text(label)
                        .size(9)
                        .color(if active { th::ACCENT } else { th::TEXT_DIM }),
                )
                .on_press(Message::SetDrumPadChokeGroup {
                    track_id,
                    pad_index: selected_pad,
                    choke_group: group,
                })
                .padding([2, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(if active { th::ACCENT_DIM } else { th::BG_DARK }.into()),
                    text_color: if active { th::ACCENT } else { th::TEXT_DIM },
                    border: iced::Border {
                        color: if active { th::ACCENT_DIM } else { th::BORDER },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            choke_row = choke_row.push(btn);
        }

        let editor = column![
            param_rows,
            row![one_shot_btn, choke_row]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(6);

        let body = container(column![grid, footer, editor].spacing(8))
            .padding([6, 6])
            .width(Length::Fill);

        Self::device_card(column![title, body].width(Length::Fixed(300.0)))
    }

    /// Placeholder card for MIDI tracks with no instrument attached.
    fn view_add_instrument_placeholder(&self) -> Element<'_, Message> {
        let title = Self::device_title_bar(text("No Instrument").size(11).color(th::TEXT_DIM));
        let body = container(text("Right-click to add").size(9).color(th::TEXT_MUTED))
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
                .on_press(Message::SetDeviceMenuCategory(
                    DeviceMenuCategory::Instruments,
                ))
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
        const PLUGIN_GRID_COLS: usize = 4;
        const PLUGIN_GRID_COL_W: f32 = 150.0;
        let mut items_col = column![].spacing(2);
        let search_lower = menu.search.to_lowercase();
        // Estimated visible rows, used to size and clamp the popup.
        let mut est_rows: usize = 0;
        let mut is_grid = false;

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
                    est_rows += 1;
                }
            }
            Some(DeviceMenuCategory::Plugins) => {
                is_grid = true;
                if self.state.plugin_settings.cache.is_empty() {
                    items_col = items_col.push(
                        text("No plugins scanned yet.\nUse File → Settings to scan.")
                            .size(11)
                            .color(th::TEXT_DIM),
                    );
                    est_rows = 2;
                } else {
                    let mut filtered: Vec<&vibez_plugin_host::PluginInfo> = self
                        .state
                        .plugin_settings
                        .cache
                        .iter()
                        .filter(|p| {
                            search_lower.is_empty()
                                || p.name.to_lowercase().contains(&search_lower)
                                || p.vendor.to_lowercase().contains(&search_lower)
                        })
                        .collect();
                    filtered.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                    est_rows = filtered.len().div_ceil(PLUGIN_GRID_COLS);
                    for chunk in filtered.chunks(PLUGIN_GRID_COLS) {
                        let mut grid_row = row![].spacing(2);
                        for plugin in chunk {
                            let format_badge = match plugin.format {
                                PluginFormat::Clap => "CLAP",
                                PluginFormat::Vst3 => "VST3",
                            };
                            let cat_label = match plugin.category {
                                PluginCategory::Effect => "fx",
                                PluginCategory::Instrument => "inst",
                                PluginCategory::Both => "fx+inst",
                            };
                            let plugin_id = plugin.id.clone();
                            // Full name, wrapping inside the fixed
                            // cell width: truncated names made the
                            // LSP suite indistinguishable.
                            let cell = column![
                                text(plugin.name.clone()).size(11).color(th::TEXT),
                                text(format!("{format_badge} {cat_label}"))
                                    .size(9)
                                    .color(th::TEXT_DIM),
                            ]
                            .spacing(1);
                            let btn = button(cell)
                                .on_press(Message::AddPluginToTrack(track_id, plugin_id))
                                .padding([4, 8])
                                .width(Length::Fixed(PLUGIN_GRID_COL_W))
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
                            grid_row = grid_row.push(btn);
                        }
                        items_col = items_col.push(grid_row);
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
                    est_rows += 1;
                }
            }
        }

        // Cap the list height and scroll it: a full plugin library is
        // hundreds of entries, which would otherwise render past the
        // bottom of the window and look like an empty menu. The
        // plugins tab uses a 4-column grid to spend the space on
        // breadth instead of one skinny endless column.
        const MENU_LIST_MAX_H: f32 = 380.0;
        let (menu_w, row_h) = if is_grid {
            (PLUGIN_GRID_COL_W * PLUGIN_GRID_COLS as f32 + 30.0, 38.0)
        } else {
            (220.0, 29.0)
        };
        let est_list_h = (est_rows.max(1) as f32 * row_h).min(MENU_LIST_MAX_H);
        let items_scroll = container(scrollable(items_col).width(Length::Fill).direction(
            scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ),
        ))
        .max_height(MENU_LIST_MAX_H);

        let menu_content = column![tabs_row, search_input, items_scroll]
            .spacing(6)
            .padding(8)
            .width(Length::Fixed(menu_w));

        let menu_card = container(menu_content).style(|_theme: &Theme| container::Style {
            background: Some(th::BG_SURFACE.into()),
            border: iced::Border {
                color: th::BORDER,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        });

        // Position the menu near where it was triggered, clamped just
        // enough that the estimated content stays on-screen (the
        // devices panel lives at the bottom of the window).
        let est_h = est_list_h + 90.0;
        let menu_y = menu.y.min(self.state.window_height - est_h).max(0.0);
        let menu_x = menu.x.min(self.state.window_width - menu_w - 16.0).max(0.0);
        let padded = column![
            vertical_space().height(Length::Fixed(menu_y)),
            row![horizontal_space().width(Length::Fixed(menu_x)), menu_card,]
        ];

        mouse_area(container(padded).width(Length::Fill).height(Length::Fill))
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
            let loop_icon_color = if clip_loop { th::ACCENT } else { th::TEXT_DIM };
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
                clip_name, pos_label, dur_label, loop_btn, dup_btn, double_btn, halve_btn,
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
        let header_row = row![label, mode_row, horizontal_space(), snap_label, snap_row]
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
    fn view_audio_clip_panel(
        &self,
        track_id: TrackId,
        clip: &UiClip,
        track_color: Color,
    ) -> Element<'_, Message> {
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
            "{}: {:.1}s",
            clip.name,
            clip.duration as f64 / self.state.sample_rate as f64
        ))
        .size(10)
        .color(th::TEXT_MUTED);

        let header_row = row![label, horizontal_space(), clip_info]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        let quantize_row = self.view_audio_quantize_row(track_id, clip.id);
        let warp_row = self.view_audio_warp_row(track_id, clip);

        let content = column![header_row, quantize_row, warp_row, waveform_canvas]
            .spacing(6)
            .padding(4);

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

    fn view_audio_warp_row(&self, track_id: TrackId, clip: &UiClip) -> Element<'_, Message> {
        let clip_id = clip.id;
        let label = text("Warp").size(11).color(th::TEXT_DIM);

        let default_text = clip
            .original_bpm
            .map(|bpm| format!("{:.1}", bpm))
            .unwrap_or_default();
        let text_value = self
            .state
            .clip_bpm_edit
            .get(&clip_id)
            .cloned()
            .unwrap_or(default_text);

        let bpm_input = text_input("BPM", &text_value)
            .on_input(move |t| Message::ClipBpmInputChanged {
                track_id,
                clip_id,
                text: t,
            })
            .on_submit(Message::SubmitClipBpm { track_id, clip_id })
            .size(11)
            .width(Length::Fixed(70.0));

        let button_style = |_theme: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
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
        };

        let detect_btn = button(text("Detect").size(11).color(th::TEXT))
            .on_press(Message::DetectClipBpm { track_id, clip_id })
            .padding([4, 10])
            .style(button_style);

        let warp_btn = button(
            text(format!("Warp → {:.0} BPM", self.state.bpm))
                .size(11)
                .color(th::TEXT),
        )
        .on_press(Message::WarpClipToProject { track_id, clip_id })
        .padding([4, 10])
        .style(button_style);

        let mut row_widgets = row![label, bpm_input, detect_btn, warp_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        if clip.warped {
            let clear_btn = button(text("Clear warp").size(11).color(th::TEXT_DIM))
                .on_press(Message::ClearClipWarp { track_id, clip_id })
                .padding([4, 10])
                .style(button_style);
            row_widgets = row_widgets.push(clear_btn);

            if let Some(warped_to) = clip.warped_to_bpm {
                let stale = (warped_to - self.state.bpm).abs() > 0.01;
                if stale {
                    row_widgets = row_widgets.push(
                        text(format!("(was {:.0})", warped_to))
                            .size(10)
                            .color(th::METER_YELLOW),
                    );
                }
            }
        }

        row_widgets.into()
    }

    fn view_audio_quantize_row(&self, track_id: TrackId, clip_id: ClipId) -> Element<'_, Message> {
        let label = text("Quantize").size(11).color(th::TEXT_DIM);
        let grid_btn = |grid: crate::state::SnapGrid| -> Element<'_, Message> {
            button(text(grid.label()).size(11).color(th::TEXT))
                .on_press(Message::QuantizeAudioClipAt {
                    track_id,
                    clip_id,
                    grid,
                })
                .padding([4, 10])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::BG_HOVER.into())
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
                })
                .into()
        };

        row![
            label,
            grid_btn(crate::state::SnapGrid::Quarter),
            grid_btn(crate::state::SnapGrid::Eighth),
            grid_btn(crate::state::SnapGrid::Sixteenth),
            grid_btn(crate::state::SnapGrid::ThirtySecond),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
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
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size.width, size.height))
                }
                _ => None,
            }),
        ])
    }
}

/// Default clip loop region end: the note content's span rounded up
/// to whole bars (4/4), capped at the clip duration. This is the
/// Ableton behavior: loop the PATTERN, not the whole clip, so a
/// 1-bar pattern in a longer clip repeats bar by bar.
fn default_loop_end(notes: &[MidiNote], duration_beats: f64) -> f64 {
    const BEATS_PER_BAR: f64 = 4.0;
    let content_end = notes
        .iter()
        .map(|n| n.start_beat + n.duration_beats)
        .fold(0.0_f64, f64::max);
    if content_end <= 0.0 {
        return duration_beats;
    }
    let bars = (content_end / BEATS_PER_BAR).ceil().max(1.0);
    (bars * BEATS_PER_BAR).min(duration_beats)
}

/// Persistent identity for a plugin device, built from scan info.
fn plugin_device_ref(info: &PluginInfo) -> vibez_core::effect::PluginDeviceInfo {
    vibez_core::effect::PluginDeviceInfo {
        format: match info.format {
            PluginFormat::Clap => "clap".to_string(),
            PluginFormat::Vst3 => "vst3".to_string(),
        },
        uid: info.id.uid.clone(),
        path: info.path.clone(),
        name: info.name.clone(),
        state_b64: None,
    }
}

/// Phase 1 of plugin loading (runs on background thread).
/// For CLAP: only loads the DSO — NO CLAP API calls (not even create_plugin).
/// For VST3: fully loads (VST3 doesn't have JUCE MessageManager issues).
fn load_plugin_effect_bg(
    info: &PluginInfo,
    sample_rate: f64,
    saved_state: Option<Vec<u8>>,
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
                vst3_partial: None,
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                // CLAP state must be applied after init_on_main_thread.
                pending_state: saved_state,
                position: None,
            })
        }
        PluginFormat::Vst3 => {
            let partial = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                false,
            )?;
            Ok(PluginLoadResult {
                track_id: TrackId::default(),
                effect_id: EffectId::new(),
                plugin_name: info.name.clone(),
                effect: None,
                gui_raw_ptr: None,
                clap_partial: None,
                vst3_partial: Some(partial),
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                pending_state: saved_state,
                position: None,
            })
        }
    }
}

/// Phase 1 of instrument loading (runs on background thread).
fn load_plugin_instrument_bg(
    info: &PluginInfo,
    sample_rate: f64,
    saved_state: Option<Vec<u8>>,
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
                vst3_partial: None,
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                pending_state: saved_state,
            })
        }
        PluginFormat::Vst3 => {
            let partial = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                true,
            )?;
            Ok(PluginInstrumentLoadResult {
                track_id: TrackId::default(),
                plugin_name: info.name.clone(),
                instrument: None,
                gui_raw_ptr: None,
                clap_partial: None,
                vst3_partial: Some(partial),
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                pending_state: saved_state,
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

    // Delete/Backspace: context-resolved in update() (selected notes
    // first, then selected clips) and ignored while renaming.
    if !modifiers.control()
        && matches!(
            key,
            iced::keyboard::Key::Named(Named::Delete)
                | iced::keyboard::Key::Named(Named::Backspace)
        )
    {
        return Some(Message::DeleteKeyPressed);
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
            "z" | "Z" => {
                if modifiers.shift() {
                    Some(Message::Redo)
                } else {
                    Some(Message::Undo)
                }
            }
            "y" | "Y" => Some(Message::Redo),
            _ => None,
        },
        _ => None,
    }
}

async fn decode_local_for_preview_async(
    path: PathBuf,
) -> Result<Arc<vibez_core::audio_buffer::DecodedAudio>, String> {
    let audio = decode_file_async(path).await?;
    Ok(Arc::new(audio))
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

struct QuantizeInput {
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    bpm: f64,
    sample_rate: u32,
    grid: crate::state::SnapGrid,
    clip_position: u64,
    clip_source_offset: u64,
    clip_duration: u64,
    original_name: String,
    new_clip_id: ClipId,
}

async fn quantize_audio_clip_async(
    input: QuantizeInput,
) -> Result<crate::message::AudioQuantizeSuccess, String> {
    tokio::task::spawn_blocking(move || compute_audio_quantize(input))
        .await
        .map_err(|e| format!("quantize task failed: {e}"))?
}

/// Attempt to auto-open an external MIDI input port at startup. If
/// `preferred` matches a visible port by name, open that; otherwise
/// open the first port in the list. Silent on failure so an
/// unavailable MIDI system doesn't block the UI.
fn auto_open_midi_input(
    preferred: Option<&str>,
) -> Option<vibez_audio_io::midi_input::MidiInputHandle> {
    let ports = vibez_audio_io::midi_input::list_midi_input_ports().ok()?;
    let target = preferred
        .and_then(|name| ports.iter().find(|p| p.as_str() == name).cloned())
        .or_else(|| ports.into_iter().next())?;
    vibez_audio_io::midi_input::open_midi_input(&target).ok()
}

async fn detect_clip_bpm_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    sample_rate: u32,
) -> Option<vibez_core::onset::BpmEstimate> {
    tokio::task::spawn_blocking(move || vibez_core::onset::detect_bpm(&audio, sample_rate))
        .await
        .unwrap_or(None)
}

struct AutoWarpInput {
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    sample_rate: u32,
    project_bpm: f64,
    confidence_threshold: f32,
}

async fn auto_warp_clip_async(input: AutoWarpInput) -> crate::message::AutoWarpOutcome {
    use crate::message::AutoWarpOutcome;
    let audio_for_detect = Arc::clone(&input.audio);
    let sample_rate = input.sample_rate;
    let estimate = tokio::task::spawn_blocking(move || {
        vibez_core::onset::detect_bpm(&audio_for_detect, sample_rate)
    })
    .await
    .unwrap_or(None);
    let Some(est) = estimate else {
        return AutoWarpOutcome::NotDetected;
    };
    if est.confidence < input.confidence_threshold || est.bpm <= 0.0 {
        return AutoWarpOutcome::DetectedOnly {
            bpm: est.bpm,
            confidence: est.confidence,
        };
    }
    let num_frames = input.audio.num_frames();
    let warp_input = crate::warp::WarpClipInput {
        audio: input.audio,
        fields_frames: num_frames as u64,
        source_offset: 0,
        duration: num_frames as u64,
        loop_start: 0,
        loop_end: 0,
        clip_bpm: est.bpm,
        project_bpm: input.project_bpm,
    };
    match crate::warp::warp_clip_async(warp_input).await {
        Ok(success) => AutoWarpOutcome::Warped {
            confidence: est.confidence,
            success,
        },
        Err(_) => AutoWarpOutcome::DetectedOnly {
            bpm: est.bpm,
            confidence: est.confidence,
        },
    }
}

fn compute_audio_quantize(
    input: QuantizeInput,
) -> Result<crate::message::AudioQuantizeSuccess, String> {
    const DEFAULT_SENSITIVITY: f32 = 1.5;
    const MIN_SLICE_FRAMES: usize = 64;

    let QuantizeInput {
        audio,
        bpm,
        sample_rate,
        grid,
        clip_position,
        clip_source_offset,
        clip_duration,
        original_name,
        new_clip_id,
    } = input;

    let sr = sample_rate as f64;
    let clip_end_src = clip_source_offset.saturating_add(clip_duration);
    let onsets: Vec<u64> = vibez_core::onset::detect_onsets(&audio, DEFAULT_SENSITIVITY)
        .into_iter()
        .filter(|&o| o >= clip_source_offset && o < clip_end_src)
        .collect();
    if onsets.is_empty() {
        return Err("No transients detected in clip".into());
    }

    let samples_to_beats = |s: u64| -> f64 { s as f64 * bpm / (sr * 60.0) };
    let beats_to_samples = |beats: f64| -> u64 {
        if bpm > 0.0 && sr > 0.0 {
            (beats * sr * 60.0 / bpm) as u64
        } else {
            0
        }
    };

    let mut source_bounds: Vec<u64> = onsets.clone();
    source_bounds.push(clip_end_src);

    let mut target_positions: Vec<u64> = Vec::with_capacity(source_bounds.len());
    for &b in &source_bounds {
        let original_timeline_pos = clip_position.saturating_add(b - clip_source_offset);
        let snapped_beats = grid
            .snap_beat(samples_to_beats(original_timeline_pos))
            .max(0.0);
        target_positions.push(beats_to_samples(snapped_beats));
    }
    for i in 1..target_positions.len() {
        if target_positions[i] < target_positions[i - 1] {
            target_positions[i] = target_positions[i - 1];
        }
    }

    let channel_count = audio.channels.len().max(1);
    let mut output_channels: Vec<Vec<f32>> = (0..channel_count).map(|_| Vec::new()).collect();
    let mut total_out_len: usize = 0;
    let mut stretched_slices = 0usize;

    for i in 0..onsets.len() {
        let src_start = source_bounds[i] as usize;
        let src_end = source_bounds[i + 1] as usize;
        let src_len = src_end.saturating_sub(src_start);
        let target_len = target_positions[i + 1].saturating_sub(target_positions[i]) as usize;
        if src_len < MIN_SLICE_FRAMES || target_len == 0 {
            continue;
        }
        // Build a per-slice DecodedAudio so pitch_preserving_stretch
        // can run a single shared analysis pass across channels.
        // Extreme ratios (very short/long snap targets) route through
        // the linear resampler automatically; typical ratios go
        // through WSOLA and preserve pitch on the bass-loop case the
        // user reported.
        let slice_channels: Vec<Vec<f32>> = audio
            .channels
            .iter()
            .map(|c| {
                let start = src_start.min(c.len());
                let end = src_end.min(c.len());
                c[start..end].to_vec()
            })
            .collect();
        let slice_audio = vibez_core::audio_buffer::DecodedAudio {
            channels: slice_channels,
            sample_rate,
        };
        let stretched_slice =
            vibez_dsp::time_stretch::pitch_preserving_stretch(&slice_audio, target_len);
        for (ch, out) in output_channels.iter_mut().enumerate() {
            if let Some(s) = stretched_slice.channels.get(ch) {
                out.extend_from_slice(s);
            }
        }
        total_out_len += target_len;
        stretched_slices += 1;
    }

    if stretched_slices == 0 || total_out_len == 0 {
        return Err("All slices collapsed to zero length after snapping".into());
    }

    for ch in output_channels.iter_mut() {
        if ch.len() < total_out_len {
            ch.resize(total_out_len, 0.0);
        } else {
            ch.truncate(total_out_len);
        }
    }

    let new_audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
        channels: output_channels,
        sample_rate,
    });

    Ok(crate::message::AudioQuantizeSuccess {
        new_clip_id,
        new_audio,
        new_name: format!("{original_name} (Q {})", grid.label()),
        new_position: target_positions[0],
        new_duration: total_out_len as u64,
        slice_count: stretched_slices,
        grid_label: grid.label().to_string(),
    })
}

async fn connect_dropbox_async(
    app_key: String,
) -> Result<(vibez_dropbox::AccountInfo, vibez_dropbox::Tokens), String> {
    let opener: Arc<dyn vibez_dropbox::BrowserOpener> =
        Arc::new(vibez_dropbox::SystemBrowserOpener);
    let tokens = vibez_dropbox::run_oauth_flow(&app_key, opener)
        .await
        .map_err(|e| e.to_string())?;
    let client = DropboxClient::new(app_key, tokens);
    let info = client.current_account().await.map_err(|e| e.to_string())?;
    let tokens = client.tokens().await;
    Ok((info, tokens))
}

async fn list_dropbox_folder_async(
    client: Arc<DropboxClient>,
    path: String,
) -> Result<(String, Vec<DropboxEntry>), String> {
    let entries = client.list_folder(&path).await.map_err(|e| e.to_string())?;
    Ok((path, entries))
}

async fn fetch_dropbox_sample_async(
    client: Arc<DropboxClient>,
    cache: DropboxCache,
    entry: DropboxEntry,
) -> Result<
    (
        Arc<vibez_core::audio_buffer::DecodedAudio>,
        String,
        MediaSourceRef,
    ),
    String,
> {
    let local = client
        .download_to_cache(&entry, &cache)
        .await
        .map_err(|e| e.to_string())?;
    let decoded = tokio::task::spawn_blocking(move || {
        vibez_audio_io::file_io::decode_audio_file(&local).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))??;
    let source = MediaSourceRef::DropboxFile {
        path_lower: entry.path_lower.clone(),
        display_path: entry.path_display.clone(),
        rev: entry.rev.clone(),
    };
    Ok((Arc::new(decoded), entry.name, source))
}

async fn export_async(
    request: vibez_engine::render::BounceRequest,
    wav_path: PathBuf,
) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        let result = vibez_engine::render::render_offline(&request);
        vibez_audio_io::file_io::write_wav_file(&wav_path, &result.audio)
            .map_err(|e| format!("WAV write error: {e}"))?;
        Ok(wav_path)
    })
    .await
    .map_err(|err| format!("export task failed: {err}"))?
}

async fn bounce_async(
    request: vibez_engine::render::BounceRequest,
    wav_path: PathBuf,
    clip_name: String,
    insert_position_samples: u64,
) -> Result<crate::message::BounceOutcome, String> {
    tokio::task::spawn_blocking(move || {
        let result = vibez_engine::render::render_offline(&request);
        vibez_audio_io::file_io::write_wav_file(&wav_path, &result.audio)
            .map_err(|e| format!("WAV write error: {e}"))?;
        Ok(crate::message::BounceOutcome {
            audio: Arc::new(result.audio),
            source: MediaSourceRef::LocalFile {
                path: wav_path.clone(),
            },
            path: wav_path,
            clip_name,
            insert_position_samples,
            warnings: result.warnings,
        })
    })
    .await
    .map_err(|err| format!("bounce task failed: {err}"))?
}

/// Finish a decoded clip for project load. The project file stores
/// the raw source reference, but a warped clip's geometry (duration /
/// offsets / loop bounds) is saved in warped-sample units, so the
/// deterministic stretch is re-applied here; otherwise every warped
/// clip reloads at its raw tempo and the whole project plays out of
/// sync. The stretch runs on a blocking thread (WSOLA over a whole
/// clip is CPU-heavy).
async fn finish_loaded_clip(
    info: ClipInfo,
    raw: Arc<vibez_core::audio_buffer::DecodedAudio>,
) -> LoadedClipData {
    if info.warped {
        if let (Some(clip_bpm), Some(warped_to_bpm)) = (info.original_bpm, info.warped_to_bpm) {
            let stretch_src = Arc::clone(&raw);
            let warped = tokio::task::spawn_blocking(move || {
                crate::warp::rewarp_for_load(&stretch_src, clip_bpm, warped_to_bpm)
            })
            .await
            .unwrap_or(None);
            if let Some(warped) = warped {
                return LoadedClipData {
                    info,
                    audio: warped,
                    original_audio: Some(raw),
                };
            }
        }
    }
    LoadedClipData {
        info,
        audio: raw,
        original_audio: None,
    }
}

async fn load_project_async(
    path: PathBuf,
    dropbox: Option<(Arc<DropboxClient>, DropboxCache)>,
) -> Result<ProjectLoadResult, String> {
    let load_path = path.clone();
    let project = tokio::task::spawn_blocking(move || {
        Project::load_from_file(&load_path).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("load task failed: {err}"))??;

    let mut clips = Vec::new();
    let mut sampler_samples = Vec::new();
    let mut drum_rack_pad_samples = Vec::new();
    let mut warnings = Vec::new();

    for clip in &project.clips {
        match clip.resolved_source().cloned() {
            Some(MediaSourceRef::LocalFile { path: clip_path }) => {
                match decode_blocking(clip_path).await {
                    Ok(audio) => {
                        clips.push(finish_loaded_clip(clip.clone(), Arc::new(audio)).await)
                    }
                    Err(err) => warnings.push(format!("Skipped clip '{}' ({})", clip.name, err)),
                }
            }
            Some(source @ MediaSourceRef::DropboxFile { .. }) => {
                match hydrate_dropbox_source(dropbox.as_ref(), &source, &clip.name).await {
                    Ok(audio) => {
                        clips.push(finish_loaded_clip(clip.clone(), Arc::new(audio)).await)
                    }
                    Err(err) => warnings.push(err),
                }
            }
            None => warnings.push(format!(
                "Skipped clip '{}' (missing source reference)",
                clip.name
            )),
        }
    }

    for track in &project.tracks {
        if let Some(native) = &track.native_instrument {
            match native {
                InstrumentStateInfo::Sampler {
                    source: Some(source),
                    ..
                } => match source {
                    MediaSourceRef::LocalFile { path: sample_path } => {
                        match decode_blocking(sample_path.clone()).await {
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
                    MediaSourceRef::DropboxFile { .. } => {
                        match hydrate_dropbox_source(dropbox.as_ref(), source, &track.name).await {
                            Ok(audio) => sampler_samples.push(LoadedSamplerData {
                                track_id: track.id,
                                source: source.clone(),
                                audio: Arc::new(audio),
                                name: source.display_name(),
                            }),
                            Err(err) => warnings.push(err),
                        }
                    }
                },
                InstrumentStateInfo::DrumRack { pads } => {
                    for (pad_index, pad) in pads.iter().enumerate() {
                        let Some(source) = &pad.source else {
                            continue;
                        };
                        let label = format!("drum pad {} on '{}'", pad_index + 1, track.name);
                        match source {
                            MediaSourceRef::LocalFile { path: sample_path } => {
                                match decode_blocking(sample_path.clone()).await {
                                    Ok(audio) => {
                                        drum_rack_pad_samples.push(LoadedDrumRackPadData {
                                            track_id: track.id,
                                            pad_index,
                                            source: source.clone(),
                                            audio: Arc::new(audio),
                                            name: source.display_name(),
                                            state: pad.clone(),
                                        })
                                    }
                                    Err(err) => warnings.push(format!("Skipped {label} ({err})")),
                                }
                            }
                            MediaSourceRef::DropboxFile { .. } => {
                                match hydrate_dropbox_source(dropbox.as_ref(), source, &label).await
                                {
                                    Ok(audio) => {
                                        drum_rack_pad_samples.push(LoadedDrumRackPadData {
                                            track_id: track.id,
                                            pad_index,
                                            source: source.clone(),
                                            audio: Arc::new(audio),
                                            name: source.display_name(),
                                            state: pad.clone(),
                                        })
                                    }
                                    Err(err) => warnings.push(err),
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(ProjectLoadResult {
        path,
        project,
        clips,
        sampler_samples,
        drum_rack_pad_samples,
        warnings,
    })
}

async fn decode_blocking(path: PathBuf) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

async fn hydrate_dropbox_source(
    dropbox: Option<&(Arc<DropboxClient>, DropboxCache)>,
    source: &MediaSourceRef,
    label: &str,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    let MediaSourceRef::DropboxFile {
        path_lower,
        display_path,
        rev,
    } = source
    else {
        return Err(format!(
            "Skipped '{label}' (expected Dropbox source reference)"
        ));
    };
    let Some((client, cache)) = dropbox else {
        return Err(format!(
            "Skipped '{label}' (not connected to Dropbox - reconnect in Settings)"
        ));
    };
    let file_name = display_path
        .rsplit_once('/')
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| display_path.clone());
    let entry = DropboxEntry {
        path_lower: path_lower.clone(),
        path_display: display_path.clone(),
        name: file_name,
        is_folder: false,
        rev: rev.clone(),
        size: None,
    };
    let local_path = client
        .download_to_cache(&entry, cache)
        .await
        .map_err(|e| format!("Skipped '{label}' ({e})"))?;
    decode_blocking(local_path)
        .await
        .map_err(|e| format!("Skipped '{label}' ({e})"))
}

async fn scan_sample_library_async(roots: Vec<PathBuf>) -> Result<SampleLibraryScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        let mut warnings = Vec::new();

        for root in roots {
            if !root.exists() {
                warnings.push(format!("Missing root: {}", root.display()));
                continue;
            }
            scan_root_into(&root, &root, &mut entries, &mut warnings);
        }

        entries.sort_by(|a, b| {
            a.relative_path
                .cmp(&b.relative_path)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(SampleLibraryScanResult { entries, warnings })
    })
    .await
    .map_err(|err| format!("scan task failed: {err}"))?
}

fn scan_root_into(
    root: &PathBuf,
    dir: &PathBuf,
    entries: &mut Vec<SampleBrowserEntry>,
    warnings: &mut Vec<String>,
) {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            warnings.push(format!("Failed to read {} ({err})", dir.display()));
            return;
        }
    };

    for item in read_dir {
        let Ok(item) = item else {
            continue;
        };
        let path = item.path();
        if path.is_dir() {
            scan_root_into(root, &path, entries, warnings);
            continue;
        }
        if !is_supported_audio_file(&path) {
            continue;
        }
        let relative_path = path
            .strip_prefix(root)
            .map(|rel| rel.to_path_buf())
            .unwrap_or_else(|_| path.clone());
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let search_text = format!(
            "{} {} {}",
            name.to_lowercase(),
            relative_path.display().to_string().to_lowercase(),
            root.display().to_string().to_lowercase()
        );
        entries.push(SampleBrowserEntry {
            source: MediaSourceRef::LocalFile { path },
            name,
            root_path: root.clone(),
            relative_path,
            search_text,
        });
    }
}

fn is_supported_audio_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "wav" | "wave" | "mp3" | "flac" | "ogg" | "aac" | "m4a" | "aif" | "aiff"
            )
        })
        .unwrap_or(false)
}

fn default_instrument_params(kind: InstrumentKind, sample_rate: f32) -> Vec<f32> {
    let instrument = vibez_instruments::create_instrument(kind, sample_rate);
    instrument
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.default)
        .collect()
}
