//! Headless application construction for router regression tests.

use crate::state::AppState;

pub(super) fn app() -> super::App {
    let (plugin_effect_tx, plugin_effect_rx) = std::sync::mpsc::channel();
    let (plugin_instrument_tx, plugin_instrument_rx) = std::sync::mpsc::channel();

    super::App {
        state: AppState::default(),
        edge_shortcuts: Default::default(),
        cmd_tx: Default::default(),
        event_rx: None,
        spectrum_rx: None,
        spectrum_tap: None,
        _stream: None,
        input_bridge: std::sync::Arc::new(Default::default()),
        _input_stream: None,
        plugin_effect_rx,
        plugin_effect_tx,
        plugin_load_requests: Default::default(),
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
