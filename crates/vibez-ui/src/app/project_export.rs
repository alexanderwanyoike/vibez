use super::*;
use vibez_core::id::{EffectId, TrackId};

pub(super) enum ExportPluginLoadEvent {
    Effect(crate::services::plugin_loader::PluginLoadResult),
    Instrument(crate::services::plugin_loader::PluginInstrumentLoadResult),
    FailedEffect(EffectId, String),
    FailedInstrument(TrackId, String),
    Finished,
}

type ExportEffectRequest = (TrackId, EffectId, vibez_core::effect::PluginDeviceInfo);
type ExportInstrumentRequest = (TrackId, vibez_core::effect::PluginDeviceInfo);

pub(super) struct ExportJob {
    request: Option<vibez_engine::render::BounceRequest>,
    path: PathBuf,
    bounce: Option<(String, u64)>,
    receiver: std::sync::mpsc::Receiver<ExportPluginLoadEvent>,
    plugins: vibez_engine::render::OfflinePlugins,
    expected_plugins: usize,
    prepared_plugins: usize,
    loader_finished: bool,
}

impl App {
    pub(super) fn handle_export_path_selected(&mut self, path: Option<PathBuf>) -> Task<Message> {
        if self.export_job.is_some() || self.export_render_progress.is_some() {
            self.state.status_text = "An export is already in progress".to_string();
            return Task::none();
        }
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
        let project = self.project_for_offline_render();
        let sample_rate = self.state.transport.sample_rate;
        let bpm = self.state.transport.bpm;
        let request = vibez_engine::render::BounceRequest {
            tracks: project.tracks,
            master: project.master,
            buses: project.buses,
            audio_clips: project.arrange.clips,
            note_clips: project.arrange.note_clips,
            clip_audio: assets.clips,
            sampler_audio: assets.samplers,
            drum_pad_audio: assets.pads,
            mode: vibez_engine::render::BounceMode::Master,
            range_samples: (0, total),
            bpm,
            sample_rate,
            swing: project.swing,
        };
        self.start_offline_render(request, path, None)
    }

    pub(super) fn start_offline_render(
        &mut self,
        request: vibez_engine::render::BounceRequest,
        path: PathBuf,
        bounce: Option<(String, u64)>,
    ) -> Task<Message> {
        if self.export_job.is_some() || self.export_render_progress.is_some() {
            self.state.status_text = "A render is already in progress".into();
            return Task::none();
        }
        let sample_rate = request.sample_rate;
        let (effect_requests, instrument_requests) = export_plugin_requests(&request);
        let expected_plugins = effect_requests.len() + instrument_requests.len();
        self.state.export_progress = Some(0);
        self.state.status_text = if expected_plugins == 0 {
            format!("Exporting… 0% · {}", path.display())
        } else {
            format!("Preparing {expected_plugins} plugin(s)… 0%")
        };

        let (sender, receiver) = std::sync::mpsc::channel();
        spawn_export_plugin_preflight(
            effect_requests,
            instrument_requests,
            sender,
            sample_rate as f64,
        );
        self.export_job = Some(ExportJob {
            request: Some(request),
            path,
            bounce,
            receiver,
            plugins: vibez_engine::render::OfflinePlugins::default(),
            expected_plugins,
            prepared_plugins: 0,
            loader_finished: false,
        });
        Task::none()
    }

    pub(super) fn poll_export(&mut self) -> Task<Message> {
        use std::sync::atomic::Ordering;

        if let Some(progress) = &self.export_render_progress {
            let percent = progress.load(Ordering::Relaxed).min(100);
            self.state.export_progress = Some(percent);
            self.state.status_text = format!("Exporting… {percent}%");
            return Task::none();
        }

        let Some(mut job) = self.export_job.take() else {
            return Task::none();
        };
        let mut failure = None;
        match job.receiver.try_recv() {
            Ok(ExportPluginLoadEvent::Effect(mut loaded)) => {
                let label = loaded.plugin_name.clone();
                match crate::services::plugin_loader::finish_effect_init_for_export(&mut loaded) {
                    Ok(Some(effect)) => {
                        job.plugins.effects.insert(loaded.effect_id, effect);
                        job.prepared_plugins += 1;
                    }
                    Ok(None) => {
                        job.plugins.failures.insert(
                            loaded.effect_id,
                            format!("{label} produced no effect instance"),
                        );
                        job.prepared_plugins += 1;
                    }
                    Err(error) => {
                        job.plugins
                            .failures
                            .insert(loaded.effect_id, format!("{label}: {error}"));
                        job.prepared_plugins += 1;
                    }
                }
            }
            Ok(ExportPluginLoadEvent::Instrument(mut loaded)) => {
                let label = loaded.plugin_name.clone();
                match crate::services::plugin_loader::finish_instrument_init_for_export(&mut loaded)
                {
                    Ok(Some(instrument)) => {
                        job.plugins.instruments.insert(loaded.track_id, instrument);
                        job.prepared_plugins += 1;
                    }
                    Ok(None) => {
                        job.plugins.instrument_failures.insert(
                            loaded.track_id,
                            format!("{label} produced no instrument instance"),
                        );
                        job.prepared_plugins += 1;
                    }
                    Err(error) => {
                        job.plugins
                            .instrument_failures
                            .insert(loaded.track_id, format!("{label}: {error}"));
                        job.prepared_plugins += 1;
                    }
                }
            }
            Ok(ExportPluginLoadEvent::FailedEffect(id, error)) => {
                job.plugins.failures.insert(id, error);
                job.prepared_plugins += 1;
            }
            Ok(ExportPluginLoadEvent::FailedInstrument(id, error)) => {
                job.plugins.instrument_failures.insert(id, error);
                job.prepared_plugins += 1;
            }
            Ok(ExportPluginLoadEvent::Finished) => {
                job.loader_finished = true;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if !job.loader_finished {
                    failure = Some("plugin preparation stopped unexpectedly".to_string());
                }
            }
        }

        if let Some(error) = failure {
            self.state.export_progress = None;
            self.state.status_text =
                format!("Render failed: {error}. No destination WAV was written.");
            return Task::none();
        }

        if !job.loader_finished || job.prepared_plugins != job.expected_plugins {
            let percent = job
                .prepared_plugins
                .saturating_mul(10)
                .checked_div(job.expected_plugins)
                .unwrap_or(0)
                .min(10) as u8;
            self.state.export_progress = Some(percent);
            self.state.status_text = format!(
                "Preparing plugins… {}/{} · {percent}%",
                job.prepared_plugins, job.expected_plugins
            );
            self.export_job = Some(job);
            return Task::none();
        }

        let progress = Arc::new(std::sync::atomic::AtomicU8::new(10));
        self.export_render_progress = Some(Arc::clone(&progress));
        self.state.export_progress = Some(10);
        self.state.status_text = "Exporting… 10%".to_string();
        let request = job.request.take().expect("export request is consumed once");
        let path = job.path;
        let (plugin_return, plugin_return_rx) = std::sync::mpsc::channel();
        self.export_plugin_return_rx = Some(plugin_return_rx);
        if let Some((clip_name, insert_position)) = job.bounce {
            Task::perform(
                bounce_async(
                    request,
                    job.plugins,
                    path,
                    clip_name,
                    insert_position,
                    progress,
                    plugin_return,
                ),
                Message::BounceComplete,
            )
        } else {
            Task::perform(
                export_async(request, job.plugins, path, progress, plugin_return),
                Message::ExportComplete,
            )
        }
    }

    pub(super) fn finish_export_runtime(&mut self) {
        self.export_render_progress = None;
        self.state.export_progress = None;
        if let Some(receiver) = self.export_plugin_return_rx.take() {
            // ExportTask has completed, so this is either immediately ready
            // or disconnected. Dropping the returned devices here satisfies
            // CLAP/VST3 main-thread teardown requirements.
            drop(receiver.recv().ok());
        }
    }
}

fn export_plugin_requests(
    request: &vibez_engine::render::BounceRequest,
) -> (Vec<ExportEffectRequest>, Vec<ExportInstrumentRequest>) {
    let mut effects = Vec::new();
    let mut instruments = Vec::new();
    let needed = vibez_engine::render::potential_dependency_ids(request);
    for track in &request.tracks {
        if !needed.contains(&track.id) {
            continue;
        }
        if let Some(device) = &track.plugin_instrument {
            instruments.push((track.id, device.clone()));
        }
        effects.extend(track.effects.iter().filter_map(|effect| {
            effect
                .plugin
                .clone()
                .map(|device| (track.id, effect.id, device))
        }));
    }
    for bus in &request.buses {
        if !needed.contains(&bus.id) {
            continue;
        }
        effects.extend(bus.effects.iter().filter_map(|effect| {
            effect
                .plugin
                .clone()
                .map(|device| (bus.id, effect.id, device))
        }));
    }
    if let Some(master) = request
        .master
        .as_ref()
        .filter(|_| needed.contains(&TrackId::MASTER))
    {
        effects.extend(master.effects.iter().filter_map(|effect| {
            effect
                .plugin
                .clone()
                .map(|device| (TrackId::MASTER, effect.id, device))
        }));
    }
    (effects, instruments)
}

fn spawn_export_plugin_preflight(
    effects: Vec<ExportEffectRequest>,
    instruments: Vec<ExportInstrumentRequest>,
    sender: std::sync::mpsc::Sender<ExportPluginLoadEvent>,
    sample_rate: f64,
) {
    std::thread::spawn(move || {
        for (track_id, effect_id, device) in effects {
            let prepared = (|| {
                let info = crate::services::plugin_loader::plugin_info_from_device(
                    &device,
                    vibez_plugin_host::PluginCategory::Effect,
                )?;
                let state = crate::services::plugin_loader::decode_plugin_state(&device)?;
                let mut loaded = crate::services::plugin_loader::load_plugin_effect_bg(
                    &info,
                    sample_rate,
                    state,
                )?;
                loaded.track_id = track_id;
                loaded.effect_id = effect_id;
                Ok::<_, String>(loaded)
            })();
            match prepared {
                Ok(loaded) => {
                    if sender.send(ExportPluginLoadEvent::Effect(loaded)).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ExportPluginLoadEvent::FailedEffect(
                        effect_id,
                        format!(
                            "{} effect '{}': {error}",
                            device.format.to_uppercase(),
                            device.name
                        ),
                    ));
                }
            }
        }
        for (track_id, device) in instruments {
            let prepared = (|| {
                let info = crate::services::plugin_loader::plugin_info_from_device(
                    &device,
                    vibez_plugin_host::PluginCategory::Instrument,
                )?;
                let state = crate::services::plugin_loader::decode_plugin_state(&device)?;
                let mut loaded = crate::services::plugin_loader::load_plugin_instrument_bg(
                    &info,
                    sample_rate,
                    state,
                )?;
                loaded.track_id = track_id;
                Ok::<_, String>(loaded)
            })();
            match prepared {
                Ok(loaded) => {
                    if sender
                        .send(ExportPluginLoadEvent::Instrument(loaded))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ExportPluginLoadEvent::FailedInstrument(
                        track_id,
                        format!(
                            "{} instrument '{}': {error}",
                            device.format.to_uppercase(),
                            device.name
                        ),
                    ));
                }
            }
        }
        let _ = sender.send(ExportPluginLoadEvent::Finished);
    });
}
