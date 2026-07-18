//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use vibez_core::effect::EffectType;
use vibez_engine::commands::EngineCommand;
use vibez_engine::events::EngineEvent;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::message::Message;
use crate::plugin_window::PluginWindowEvent;
use crate::state::{ArrangementSelection, AuditionMode, DetailPanelTab, UiEffect};

use super::*;

fn apply_track_mute_request(
    project_tracks: &mut Arc<crate::state::ProjectTracksState>,
    history: &mut crate::state::UndoHistory,
    pre_edit_snapshot: crate::state::ProjectSnapshot,
    request: crate::domains::perform::TrackMuteRequest,
    engine: &mut impl crate::domains::EngineHandle,
) -> Option<String> {
    project_tracks.find(request.track_id)?;
    history.push_edit(pre_edit_snapshot, None);
    let track = Arc::make_mut(project_tracks).find_mut(request.track_id)?;
    track.mute = request.muted;
    let track_name = track.name.clone();
    engine.send(EngineCommand::SetTrackMute(request.track_id, request.muted));
    Some(track_name)
}

impl App {
    /// Apply cross-domain effects requested by Perform without giving the
    /// Perform interaction slice ownership of Project Track state.
    pub(super) fn apply_perform_action(&mut self, action: crate::domains::perform::PerformAction) {
        if action.persist_settings {
            self.persist_ui_settings();
            self.state.status_text = "Perform key mapping saved".into();
        }
        if let Some(request) = action.track_mute_request {
            let pre_edit_snapshot = self.take_snapshot();
            let changed = {
                let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                apply_track_mute_request(
                    &mut self.state.project_tracks,
                    &mut self.state.project.history,
                    pre_edit_snapshot,
                    request,
                    &mut engine,
                )
            };
            if let Some(track_name) = changed {
                self.mark_project_dirty();
                self.state.status_text = format!(
                    "{} {track_name}",
                    if request.muted { "Muted" } else { "Unmuted" }
                );
            }
        }
    }

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
        if !action.debounce_root_scans.is_empty() {
            return Task::batch(
                action
                    .debounce_root_scans
                    .into_iter()
                    .map(|(root, revision)| {
                        Task::perform(
                            async move {
                                tokio::time::sleep(std::time::Duration::from_millis(180)).await;
                                (root, revision)
                            },
                            |(root, revision)| {
                                Message::Browser(BrowserMsg::ReconcileLocalRoot { root, revision })
                            },
                        )
                    }),
            );
        }
        if let Some((root, revision)) = action.scan_root {
            return Task::perform(scan_sample_root_async(root.clone()), move |result| {
                Message::Browser(BrowserMsg::LocalRootCatalogReconciled {
                    root: root.clone(),
                    revision,
                    result,
                })
            });
        }
        if let Some(source) = action.load_waveform {
            self.state.browser.begin_waveform_load(&source);
            if let MediaSourceRef::LocalFile { path } = source.clone() {
                return Task::perform(decode_local_for_preview_async(path), move |result| {
                    Message::BrowserWaveformReady(source.clone(), result)
                });
            }
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
                        // The master strip meters the same summed mix.
                        let project_tracks = Arc::make_mut(&mut self.state.project_tracks);
                        project_tracks.master.peak_l = self.state.peak_l;
                        project_tracks.master.peak_r = self.state.peak_r;
                    }
                    EngineEvent::PlaybackStopped => {
                        self.state.transport.playing = false;
                    }
                    EngineEvent::PlaybackStarted => {
                        self.state.transport.playing = true;
                    }
                    EngineEvent::AuditionStopped => {
                        self.state.browser.stop_audition_state();
                        if matches!(
                            self.state.status_text.as_str(),
                            "RAW Audition playing" | "WARP Audition playing"
                        ) {
                            self.state.status_text = "Audition finished".into();
                        }
                    }
                    EngineEvent::AuditionQueued => {
                        self.state.browser.audition_loading = false;
                        self.state.browser.audition_playing = false;
                        self.state.browser.audition_queued = true;
                    }
                    EngineEvent::AuditionStarted => {
                        self.state.browser.audition_loading = false;
                        self.state.browser.audition_queued = false;
                        self.state.browser.audition_playing = true;
                        self.state.status_text = match self.state.browser.audition_mode {
                            AuditionMode::Raw => "RAW Audition playing".into(),
                            AuditionMode::Warp => "WARP Audition playing".into(),
                        };
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
                    EngineEvent::TrackMuteChanged {
                        track_id,
                        muted,
                        effective_at_samples: _,
                    } => {
                        if let Some(track) = self.state.find_track_mut(track_id) {
                            track.mute = muted;
                        }
                    }
                }
            }
        }
    }

    /// Keep the engine's spectrum tap on the selected track and pump
    /// drained samples through the analyser.
    fn poll_spectrum(&mut self) {
        let wanted = self.state.arrangement.selected_track;
        if wanted != self.spectrum_tap {
            self.send_command(EngineCommand::SetSpectrumTap(wanted));
            self.spectrum_tap = wanted;
            self.state.spectrum.reset();
        }
        if let Some(ref mut rx) = self.spectrum_rx {
            // Drain in slices; the ring holds well under a second.
            let mut chunk = [0.0f32; 512];
            loop {
                let mut n = 0;
                while n < chunk.len() {
                    match rx.pop() {
                        Ok(s) => {
                            chunk[n] = s;
                            n += 1;
                        }
                        Err(_) => break,
                    }
                }
                if n == 0 {
                    break;
                }
                self.state.spectrum.ingest(&chunk[..n]);
                if n < chunk.len() {
                    break;
                }
            }
        }
        self.state
            .spectrum
            .analyse(self.state.transport.sample_rate as f32);
    }

    /// One frame of the 60fps subscription: drain engine events and
    /// pump every background service.
    pub(super) fn handle_tick(&mut self) -> Task<Message> {
        self.poll_engine_events();
        self.poll_spectrum();
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

#[cfg(test)]
mod perform_action_tests {
    use super::*;
    use crate::domains::test_support::RecordingEngine;
    use crate::state::{AppState, ProjectSnapshot, ProjectTrack};
    use vibez_core::id::TrackId;

    fn snapshot(state: &AppState) -> ProjectSnapshot {
        ProjectSnapshot {
            project_tracks: Arc::clone(&state.project_tracks),
            arrange_timeline: Arc::clone(&state.arrangement.timeline),
            bpm: state.transport.bpm,
            bpm_text: state.transport.bpm_text.clone(),
            loop_enabled: state.transport.loop_enabled,
            loop_start_beats: state.transport.loop_start_beats,
            loop_end_beats: state.transport.loop_end_beats,
            selected_track: state.arrangement.selected_track,
            selected_clips: state.arrangement.selected_clips.clone(),
            selected_note_clip: state.arrangement.selected_note_clip,
        }
    }

    #[test]
    fn perform_mute_request_updates_the_shared_track_and_engine_together() {
        let track_id = TrackId::new();
        let mut state = AppState::default();
        Arc::make_mut(&mut state.project_tracks)
            .tracks
            .push(ProjectTrack::new(track_id, "Drums".into(), 0));
        let mut engine = RecordingEngine::default();
        let pre_edit_snapshot = snapshot(&state);

        let name = apply_track_mute_request(
            &mut state.project_tracks,
            &mut state.project.history,
            pre_edit_snapshot,
            crate::domains::perform::TrackMuteRequest {
                track_id,
                muted: true,
            },
            &mut engine,
        );

        assert_eq!(name.as_deref(), Some("Drums"));
        assert!(state.project_tracks.tracks[0].mute);
        assert!(matches!(
            engine.0.as_slice(),
            [EngineCommand::SetTrackMute(event_track, true)] if *event_track == track_id
        ));
        assert_eq!(state.project.history.undo.len(), 1);
        let before_mute = state.project.history.pop_undo().expect("mute undo step");
        assert!(!before_mute.project_tracks.tracks[0].mute);
    }

    #[test]
    fn missing_track_mute_request_does_not_create_an_undo_step() {
        let mut state = AppState::default();
        let mut engine = RecordingEngine::default();
        let pre_edit_snapshot = snapshot(&state);

        let name = apply_track_mute_request(
            &mut state.project_tracks,
            &mut state.project.history,
            pre_edit_snapshot,
            crate::domains::perform::TrackMuteRequest {
                track_id: TrackId::new(),
                muted: true,
            },
            &mut engine,
        );

        assert_eq!(name, None);
        assert!(state.project.history.undo.is_empty());
        assert!(engine.0.is_empty());
    }
}
