//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use vibez_core::effect::EffectType;
use vibez_engine::commands::EngineCommand;
use vibez_engine::events::EngineEvent;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::message::Message;
use crate::plugin_window::PluginWindowEvent;
use crate::state::{ArrangementSelection, DetailPanelTab, UiEffect};

use super::*;

impl App {
    /// Route cross-domain effects requested by the arrangement domain.
    pub(super) fn apply_arrangement_action(
        &mut self,
        action: crate::domains::arrangement::ArrangementAction,
    ) -> Task<Message> {
        if action.close_context_menu {
            self.state.view.context_menu = None;
        }
        if action.focus_clip_tab {
            self.state.view.detail_panel_tab = DetailPanelTab::Clip;
        }
        if let Some(beat) = action.scroll_to_beat {
            self.auto_scroll_to_beat(beat);
        }
        if let Some((start, end)) = action.loop_from_selection {
            self.state.view.context_menu = None;
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            let _ = self.state.transport.update(
                crate::domains::transport::TransportMsg::SetArrangementLoopRegion {
                    start_beats: start,
                    end_beats: end,
                },
                &mut engine,
                crate::domains::transport::TransportCtx::default(),
            );
            if !self.state.transport.loop_enabled {
                let _ = self.state.transport.update(
                    crate::domains::transport::TransportMsg::ToggleArrangementLoop,
                    &mut engine,
                    crate::domains::transport::TransportCtx::default(),
                );
            }
        }
        if let Some(track_id) = action.close_track_guis {
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
        }
        if let Some(status) = action.status {
            self.state.status_text = status;
        }
        if action.mark_dirty {
            self.state.project.dirty = true;
        }
        Task::none()
    }

    /// Route cross-domain effects requested by the view domain.
    pub(super) fn apply_view_action(
        &mut self,
        action: crate::domains::view::ViewAction,
    ) -> Task<Message> {
        if let Some((track_id, clip_id, is_note_clip)) = action.select_clip {
            let selection = if is_note_clip {
                ArrangementSelection::NoteClip { track_id, clip_id }
            } else {
                ArrangementSelection::AudioClip { track_id, clip_id }
            };
            if !self.state.arrangement.selected_clips.contains(&selection) {
                self.state.arrangement.selected_clips.clear();
                self.state.arrangement.selected_clips.insert(selection);
            }
            self.state.arrangement.selected_track = Some(track_id);
        }
        if action.end_drag_resize {
            self.state.arrangement.drag_resize_active = false;
        }
        if action.close_device_menu {
            self.state.devices.context_menu = None;
        }
        if let Some(rename) = action.rename {
            use crate::domains::view::RenameRequest;
            return match rename {
                RenameRequest::Track(track_id, name) => {
                    self.update(Message::rename_track(track_id, name))
                }
                RenameRequest::Clip(track_id, clip_id, name) => {
                    self.update(Message::rename_clip(track_id, clip_id, name))
                }
            };
        }
        Task::none()
    }

    /// Route cross-domain effects requested by the browser domain.
    pub(super) fn apply_browser_action(
        &mut self,
        action: crate::domains::browser::BrowserAction,
    ) -> Task<Message> {
        if let Some(status) = action.status {
            self.state.status_text = status;
        }
        if action.persist_settings {
            self.persist_ui_settings();
        }
        if action.expand_dropbox_root && self.dropbox_client.is_some() {
            return self.update(Message::DropboxExpandFolder(String::new()));
        }
        if let Some((track_id, beat, source)) = action.drop_on_arrangement {
            let position_samples = self.state.beats_to_samples(beat);
            return self.dispatch_drop_on_arrangement(track_id, position_samples, source);
        }
        Task::none()
    }

    /// Route cross-domain effects requested by the piano roll domain.
    pub(super) fn apply_piano_roll_action(
        &mut self,
        action: crate::domains::piano_roll::PianoRollAction,
    ) {
        if action.close_context_menu {
            self.state.view.context_menu = None;
        }
        if let Some(sel) = action.select_note_clip {
            self.state.arrangement.selected_note_clip = Some(sel);
        }
        if let Some(track_id) = action.select_track {
            self.state.arrangement.selected_track = Some(track_id);
        }
        if let Some(beat) = action.scroll_to_beat {
            self.auto_scroll_to_beat(beat);
        }
        if action.drag_resize_active {
            self.state.arrangement.drag_resize_active = true;
        }
        if let Some(status) = action.status {
            self.state.status_text = status;
        }
    }

    /// Route cross-domain effects requested by the devices domain.
    pub(super) fn apply_devices_action(&mut self, action: crate::domains::devices::DevicesAction) {
        if let Some(key) = action.close_gui {
            if let Some(ref mut mgr) = self.plugin_window_manager {
                mgr.close(key);
            }
            self.plugin_gui_raw_ptrs.remove(&key);
            self.plugin_state_ptrs.remove(&key);
        }
        if let Some(track_id) = action.select_track {
            self.state.arrangement.selected_track = Some(track_id);
        }
        if let Some(status) = action.status {
            self.state.status_text = status;
        }
    }

    /// Route a cross-domain effect requested by the transport domain.
    pub(super) fn apply_transport_action(
        &mut self,
        action: crate::domains::transport::TransportAction,
    ) -> Task<Message> {
        use crate::domains::transport::TransportAction;
        match action {
            TransportAction::None => Task::none(),
            TransportAction::ClearTimeSelection => {
                self.state.arrangement.time_selection_active = false;
                self.state.arrangement.time_selection_track = None;
                Task::none()
            }
            TransportAction::TempoChanged { old_bpm, new_bpm } => {
                self.follow_tempo_change(old_bpm, new_bpm)
            }
        }
    }

    pub(super) fn poll_plugin_loads(&mut self) {
        // Poll for loaded plugin effects
        while let Ok(mut result) = self.plugin_effect_rx.try_recv() {
            let track_id = result.track_id;
            let effect_id = result.effect_id;
            let plugin_name = result.plugin_name.clone();

            // Phase 2 runs in the loader service: init on the UI thread
            // (JUCE binds its MessageManager here) + state restore.
            let (effect, gui_raw_ptr) =
                match crate::services::plugin_loader::finish_effect_init(&mut result) {
                    Ok(Some(pair)) => pair,
                    Ok(None) => continue,
                    Err(e) => {
                        eprintln!("vibez: {e}");
                        self.state.status_text = format!("Plugin init failed: {e}");
                        continue;
                    }
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
                // Real plugin parameters (already leaked 'static by the
                // wrapper): drives the knob strip and automation picker.
                let descriptors = effect.param_descriptors();
                let params: Vec<f32> = (0..descriptors.len())
                    .map(|i| effect.get_param(i))
                    .collect();
                let ui_effect = UiEffect {
                    id: effect_id,
                    effect_type: EffectType::Gain,
                    bypass: false,
                    params,
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

            // Phase 2 runs in the loader service.
            let (instrument, gui_raw_ptr) =
                match crate::services::plugin_loader::finish_instrument_init(&mut result) {
                    Ok(Some(pair)) => pair,
                    Ok(None) => continue,
                    Err(e) => {
                        eprintln!("vibez: {e}");
                        self.state.status_text = format!("Plugin init failed: {e}");
                        continue;
                    }
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
                track.plugin_instrument_descriptors = instrument.param_descriptors();
                track.has_plugin_instrument_gui = has_gui;
            }
            self.send_command(EngineCommand::SetPluginInstrument {
                track_id,
                instrument,
            });
            self.state.status_text = format!("Loaded {plugin_name}");
        }
    }

    pub(super) fn poll_plugin_windows(&mut self) {
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
    pub(super) fn poll_midi_input(&mut self) {
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
        let Some(track_id) = self.state.arrangement.selected_track else {
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

    pub(super) fn poll_engine_events(&mut self) {
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
                        self.state.transport.position_samples = pos;
                    }
                    EngineEvent::Metering { peak_l, peak_r, .. } => {
                        self.state.peak_l = peak_l.max(self.state.peak_l * 0.85);
                        self.state.peak_r = peak_r.max(self.state.peak_r * 0.85);
                    }
                    EngineEvent::PlaybackStopped => {
                        self.state.transport.playing = false;
                    }
                    EngineEvent::PlaybackStarted => {
                        self.state.transport.playing = true;
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

    /// One frame of the 60fps subscription: drain engine events and
    /// pump every background service.
    pub(super) fn handle_tick(&mut self) -> Task<Message> {
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
        if self.state.arrangement.drag_resize_active {
            let edge_zone = 60.0_f32;
            // Right edge: estimate window right ~= track header + canvas
            // Use cursor_x relative to a conservative right boundary
            let right_boundary = 1600.0_f32; // reasonable default
            if self.state.view.cursor_x > right_boundary - edge_zone {
                let overshoot = ((self.state.view.cursor_x - (right_boundary - edge_zone))
                    / edge_zone)
                    .clamp(0.0, 3.0) as f64;
                let delta = overshoot * 2.0;
                let total = self.state.total_beats();
                self.state.view.scroll_offset_beats =
                    (self.state.view.scroll_offset_beats + delta).clamp(0.0, total);
            }
            // Left edge
            let left_boundary = 230.0_f32; // ~track header width
            if self.state.view.cursor_x < left_boundary + edge_zone
                && self.state.view.scroll_offset_beats > 0.0
            {
                let overshoot = ((left_boundary + edge_zone - self.state.view.cursor_x) / edge_zone)
                    .clamp(0.0, 3.0) as f64;
                let delta = overshoot * 2.0;
                self.state.view.scroll_offset_beats =
                    (self.state.view.scroll_offset_beats - delta).max(0.0);
            }
        }
        Task::none()
    }
}
