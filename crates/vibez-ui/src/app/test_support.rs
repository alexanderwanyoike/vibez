//! Headless application construction for router regression tests.

use crate::state::AppState;

/// Escape as the window delivers it.
pub(super) fn escape_key() -> iced::keyboard::Event {
    use iced::keyboard::key::{Code, Named, Physical};
    iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Named(Named::Escape),
        modified_key: iced::keyboard::Key::Named(Named::Escape),
        physical_key: Physical::Code(Code::Escape),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::empty(),
        text: None,
    }
}

/// Hands a key press the widgets left with `status` to the app's global
/// shortcuts, as the window does; the keyboard router returns their
/// message as a task, which tests do not run.
pub(super) fn deliver_key(
    app: &mut super::App,
    event: iced::keyboard::Event,
    status: iced::event::Status,
) {
    if super::keyboard::keyboard_input_message(event.clone(), status).is_none() {
        return;
    }
    if let iced::keyboard::Event::KeyPressed { key, modifiers, .. } = event {
        if let Some(message) = super::keyboard::global_key_handler(key, modifiers) {
            let _ = app.update(message);
        }
    }
}

pub(super) fn app() -> super::App {
    let (plugin_effect_tx, plugin_effect_rx) = std::sync::mpsc::channel();
    let (plugin_instrument_tx, plugin_instrument_rx) = std::sync::mpsc::channel();

    super::App {
        state: AppState::default(),
        edge_shortcuts: Default::default(),
        cmd_tx: Default::default(),
        event_rx: None,
        _stream: None,
        sample_retirement: Default::default(),
        input_bridge: std::sync::Arc::new(Default::default()),
        _input_stream: None,
        plugin_effect_rx,
        plugin_effect_tx,
        plugin_load_requests: Default::default(),
        device_meters: Default::default(),
        compressor_devices: Default::default(),
        filter_panels: Default::default(),
        gate_panels: Default::default(),
        card_panels: Default::default(),
        instrument_panels: Default::default(),
        gain_devices: Default::default(),
        eq_devices: Default::default(),
        param_entry: None,
        device_presets: Default::default(),
        sidechain_sync_inputs: None,
        track_meter_peaks: Default::default(),
        plugin_instrument_rx,
        plugin_instrument_tx,
        plugin_window_manager: None,
        plugin_gui_raw_ptrs: Default::default(),
        plugin_state_ptrs: Default::default(),
        export_job: None,
        export_render_progress: None,
        export_plugin_return_rx: None,
        dropbox_settings: Default::default(),
        dropbox_cache: Default::default(),
        dropbox_client: None,
        remote_materialization_request: Default::default(),
        remote_import_request: Default::default(),
        remote_audition_cache_lease: None,
        pending_remote_audition: None,
        browser_import_request: Default::default(),
        remote_catalog_request: Default::default(),
        section_residency_request: Default::default(),
        remote_catalog_pending: Vec::new(),
        midi_input: None,
        midi_input_ports: Vec::new(),
        interface_scale: crate::ui_settings::INTERFACE_SCALE_DEFAULT,
        save_runtime: Default::default(),
    }
}

/// Delivers `message` and every message its Tasks produce, as the iced
/// runtime would, until nothing is left. Window, clipboard and other runtime
/// actions are dropped.
pub(super) async fn run_until_idle(app: &mut super::App, message: crate::message::Message) {
    use iced_runtime::futures::futures::StreamExt;

    const MAX_MESSAGES: usize = 1_000;
    let mut pending = std::collections::VecDeque::from([message]);
    let mut delivered = 0;
    while let Some(message) = pending.pop_front() {
        delivered += 1;
        assert!(delivered <= MAX_MESSAGES, "Tasks kept producing messages");
        if let Some(mut actions) = iced_runtime::task::into_stream(app.update(message)) {
            while let Some(action) = actions.next().await {
                if let iced_runtime::Action::Output(next) = action {
                    pending.push_back(next);
                }
            }
        }
    }
}

/// Joins the selected Audio Clips the way Ctrl+J does.
pub(super) async fn join_selected_audio_clips(app: &mut super::App) {
    run_until_idle(
        app,
        crate::message::Message::Arrangement(
            crate::domains::arrangement::ArrangementMsg::JoinSelectedClips,
        ),
    )
    .await;
}
