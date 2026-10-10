//! Publishes current plugin completions after main-thread initialization.

use super::*;
use crate::state::UiEffect;
use vibez_core::effect::EffectType;
use vibez_core::id::{EffectId, TrackId};

impl App {
    pub(super) fn poll_plugin_loads(&mut self) {
        // Poll for loaded plugin effects
        while let Ok(mut result) = self.plugin_effect_rx.try_recv() {
            let track_id = result.track_id;
            let effect_id = result.effect_id;
            let plugin_name = result.plugin_name.clone();
            if !self.plugin_load_requests.finish(
                PluginGuiKey::Effect {
                    track_id,
                    effect_id,
                },
                result.load_token,
            ) {
                continue;
            }
            let track = self.state.find_track(track_id);
            if !crate::domains::project::accepts_effect_load(
                track,
                effect_id,
                result.position.is_some(),
                &result.device_ref,
            ) {
                continue;
            }
            if result.position.is_some() {
                result.position = track
                    .and_then(|track| track.effects.iter().position(|slot| slot.id == effect_id));
            }

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
                    reconfiguration_failed: false,
                    latency_samples: Some(effect.latency_samples()),
                    inactive_sidechains: track
                        .effects
                        .iter()
                        .find(|slot| slot.id == effect_id)
                        .map(|slot| slot.inactive_sidechains.clone())
                        .unwrap_or_default(),
                    sidechains: track
                        .effects
                        .iter()
                        .find(|slot| slot.id == effect_id)
                        .map(|slot| slot.sidechains.clone())
                        .unwrap_or_default(),
                    external_inputs: effect.external_inputs().to_vec(),

                    id: effect_id,
                    effect_type: EffectType::Gain,
                    bypass: track
                        .effects
                        .iter()
                        .find(|slot| slot.id == effect_id)
                        .is_some_and(|slot| slot.bypass),
                    params,
                    descriptors,
                    plugin_name: Some(plugin_name.clone()),
                    has_plugin_gui: has_gui,
                    plugin_ref: Some(result.device_ref.clone()),
                };
                if let Some(slot) = track.effects.iter_mut().find(|slot| slot.id == effect_id) {
                    *slot = ui_effect;
                } else {
                    match result.position {
                        Some(pos) if pos < track.effects.len() => {
                            track.effects.insert(pos, ui_effect)
                        }
                        _ => track.effects.push(ui_effect),
                    }
                }
            }
            self.send_command(EngineCommand::AddPluginEffect {
                track_id,
                effect_id,
                effect,
                position: result.position,
            });
            if let Some(bypass) = self
                .state
                .find_track(track_id)
                .and_then(|track| track.effects.iter().find(|slot| slot.id == effect_id))
                .map(|slot| slot.bypass)
            {
                self.send_command(EngineCommand::SetEffectBypass {
                    track_id,
                    effect_id,
                    bypass,
                });
            }
            self.state.status_text = format!("Loaded {plugin_name}");
        }

        // Poll for loaded plugin instruments
        while let Ok(mut result) = self.plugin_instrument_rx.try_recv() {
            let track_id = result.track_id;
            let plugin_name = result.plugin_name.clone();
            if !self
                .plugin_load_requests
                .finish(PluginGuiKey::Instrument { track_id }, result.load_token)
                || self.state.find_track(track_id).is_none()
            {
                continue;
            }

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
                track.instrument_kind = None;
                track.sample_name = None;
                track.sample_source = None;
                track.sample_audio = None;
                track.instrument_params.clear();
                track.drum_rack_pads = (0..vibez_core::track::DRUM_RACK_PAD_COUNT)
                    .map(|_| crate::state::UiDrumPad::default())
                    .collect();
                track.selected_drum_pad = 0;
                track.instrument_latency_samples = Some(instrument.latency_samples());
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
}

impl App {
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
        let effect_requests = effect_requests
            .into_iter()
            .map(|(track, effect, position, device)| {
                let token = self.plugin_load_requests.begin(PluginGuiKey::Effect {
                    track_id: track,
                    effect_id: effect,
                });
                (token, track, effect, position, device)
            })
            .collect();
        let instrument_requests = instrument_requests
            .into_iter()
            .map(|(track, device)| {
                let token = self
                    .plugin_load_requests
                    .begin(PluginGuiKey::Instrument { track_id: track });
                (token, track, device)
            })
            .collect();
        crate::services::plugin_loader::spawn_device_reloads(
            effect_requests,
            instrument_requests,
            self.plugin_effect_tx.clone(),
            self.plugin_instrument_tx.clone(),
            self.state.transport.sample_rate as f64,
        );
    }
}
