//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use crate::message::Message;

use super::*;

impl App {
    pub(super) fn handle_set_buffer_size(&mut self, size: u32) -> Task<Message> {
        self.state.settings_buffer_size = size;

        if let Some(stream) = self._stream.as_mut() {
            match stream.reconfigure(Some(size)) {
                Ok(()) => {
                    let sr = stream.sample_rate();
                    if let Err(e) = stream.play() {
                        eprintln!("vibez: failed to restart audio stream: {e}");
                    }
                    self.state.transport.sample_rate = sr;
                    self.state.status_text = format!("Audio restarted — buffer {size}, {sr} Hz");
                }
                Err(e) => {
                    eprintln!("vibez: failed to reconfigure audio stream: {e}");
                    self.state.status_text = format!("Audio error: {e}");
                }
            }
        } else {
            self.state.status_text = "No audio device — cannot change buffer size".to_string();
        }
        Task::none()
    }

    pub(super) fn handle_scan_plugins_complete(
        &mut self,
        report: vibez_plugin_host::ScanReport,
    ) -> Task<Message> {
        let count = report.plugins.len();
        self.state.plugin_settings.cache = report.plugins;
        self.state.plugin_settings.mark_cache_refreshed();
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
        Task::none()
    }

    pub(super) fn handle_scan_plugins(&mut self) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn handle_close_plugin_gui(&mut self, key: PluginGuiKey) -> Task<Message> {
        if let Some(ref mut mgr) = self.plugin_window_manager {
            mgr.close(key);
        }
        Task::none()
    }

    pub(super) fn handle_plugin_scan_path_selected(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            if !self.state.plugin_settings.extra_scan_paths.contains(&path) {
                self.state.plugin_settings.extra_scan_paths.push(path);
                let _ = self.state.plugin_settings.save();
            }
        }
        Task::none()
    }
}
