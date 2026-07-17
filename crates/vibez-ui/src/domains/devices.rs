//! Devices domain: the per-track device chain. Built-in effects
//! (add/remove/reorder/params/bypass), native instruments and their
//! params, drum pad editing, note audition, and the device context
//! menu.
//!
//! Follows the transport reference pattern with one addition: tracks
//! are the shared model (like database entities), so `update`
//! receives `&mut [ProjectTrack]` explicitly. The domain may touch the
//! DEVICE fields of a track and nothing else; every dependency is
//! visible in the signature, which is what keeps this testable with
//! plain `Vec<ProjectTrack>` fixtures.
//!
//! Deliberately NOT here (async/infrastructure, future services
//! tranche): plugin scanning and loading pipelines, plugin GUI window
//! management, and file dialogs. Their cross-domain needs surface as
//! [`DevicesAction`] fields instead.

use vibez_core::effect::EffectType;
use vibez_core::id::{EffectId, TrackId};
use vibez_core::midi::InstrumentKind;
use vibez_engine::commands::EngineCommand;
use vibez_plugin_host::gui::PluginGuiKey;

use super::EngineHandle;
use crate::message::DrumPadParam;
use crate::state::{DeviceContextMenu, DeviceMenuCategory, ProjectTrack, UiDrumPad, UiEffect};

/// Messages the devices domain handles.
#[derive(Debug, Clone)]
pub enum DevicesMsg {
    AddEffect(TrackId, EffectType),
    RemoveEffect(TrackId, EffectId),
    SetEffectParam(TrackId, EffectId, usize, f32),
    /// Set several parameters of one effect atomically (the EQ
    /// display drags frequency and gain together).
    SetEffectParams(TrackId, EffectId, Vec<(usize, f32)>),
    ToggleEffectBypass(TrackId, EffectId),
    MoveEffectUp(TrackId, EffectId),
    MoveEffectDown(TrackId, EffectId),
    SetTrackInstrument(TrackId, InstrumentKind),
    RemoveTrackInstrument(TrackId),
    SetInstrumentParam(TrackId, usize, f32),
    SelectDrumRackPad(TrackId, usize),
    ClearDrumRackPad(TrackId, usize),
    SetDrumPadParam {
        track_id: TrackId,
        pad_index: usize,
        param: DrumPadParam,
        value: f32,
    },
    SetDrumPadOneShot {
        track_id: TrackId,
        pad_index: usize,
        one_shot: bool,
    },
    SetDrumPadChokeGroup {
        track_id: TrackId,
        pad_index: usize,
        choke_group: Option<u8>,
    },
    /// Preview a pitch on a track's instrument. `on: false` releases.
    AuditionNote {
        track_id: TrackId,
        pitch: u8,
        on: bool,
    },
    ShowContextMenu {
        x: f32,
        y: f32,
        track_id: TrackId,
    },
    DismissContextMenu,
    SetMenuCategory(DeviceMenuCategory),
    MenuSearch(String),
}

impl DevicesMsg {
    /// Whether this message edits the project (drives the dirty flag).
    pub fn marks_dirty(&self) -> bool {
        !matches!(
            self,
            DevicesMsg::AuditionNote { .. }
                | DevicesMsg::SelectDrumRackPad(..)
                | DevicesMsg::ShowContextMenu { .. }
                | DevicesMsg::DismissContextMenu
                | DevicesMsg::SetMenuCategory(_)
                | DevicesMsg::MenuSearch(_)
        )
    }
}

/// Cross-domain effects requested by a devices update. A struct
/// rather than an enum because several can occur at once.
#[derive(Debug, Default, PartialEq)]
pub struct DevicesAction {
    /// A plugin GUI window (and its raw pointers) must be torn down.
    pub close_gui: Option<PluginGuiKey>,
    /// The interaction implies focusing this track.
    pub select_track: Option<TrackId>,
    /// Status bar text.
    pub status: Option<String>,
}

/// Devices domain state slice: currently just the context menu; the
/// device data itself lives on the shared track model.
#[derive(Debug, Default)]
pub struct DevicesState {
    pub context_menu: Option<DeviceContextMenu>,
}

/// Default parameter values for a freshly added native instrument.
pub fn default_instrument_params(kind: InstrumentKind, sample_rate: f32) -> Vec<f32> {
    let instrument = vibez_instruments::create_instrument(kind, sample_rate);
    instrument
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.default)
        .collect()
}

fn find_track_mut<'a>(
    tracks: &'a mut [ProjectTrack],
    master: &'a mut ProjectTrack,
    buses: &'a mut [ProjectTrack],
    track_id: TrackId,
) -> Option<&'a mut ProjectTrack> {
    if track_id.is_master() {
        return Some(master);
    }
    tracks
        .iter_mut()
        .chain(buses.iter_mut())
        .find(|t| t.id == track_id)
}

/// Send the engine a pad's full state (single source of truth for
/// pad edits).
fn sync_pad(
    tracks: &[ProjectTrack],
    track_id: TrackId,
    pad_index: usize,
    engine: &mut impl EngineHandle,
) {
    let state = tracks
        .iter()
        .find(|t| t.id == track_id)
        .and_then(|track| track.drum_rack_pads.get(pad_index))
        .map(UiDrumPad::to_state);
    if let Some(state) = state {
        engine.send(EngineCommand::SetDrumRackPadState {
            track_id,
            pad_index,
            state,
        });
    }
}

impl DevicesState {
    pub fn update(
        &mut self,
        msg: DevicesMsg,
        engine: &mut impl EngineHandle,
        tracks: &mut [ProjectTrack],
        master: &mut ProjectTrack,
        buses: &mut [ProjectTrack],
        sample_rate: u32,
    ) -> DevicesAction {
        let mut action = DevicesAction::default();
        match msg {
            DevicesMsg::AddEffect(track_id, effect_type) => {
                let effect_id = EffectId::new();
                let fx = vibez_dsp::factory::create_effect(effect_type, sample_rate as f32);
                let descriptors = fx.param_descriptors();
                let params: Vec<f32> = descriptors.iter().map(|d| d.default).collect();

                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
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
                engine.send(EngineCommand::AddEffect {
                    track_id,
                    effect_id,
                    effect_type,
                    position: None,
                });
                self.context_menu = None;
                action.status = Some(format!("Added {} effect", effect_type.name()));
            }
            DevicesMsg::RemoveEffect(track_id, effect_id) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    track.effects.retain(|e| e.id != effect_id);
                }
                engine.send(EngineCommand::RemoveEffect(track_id, effect_id));
                action.close_gui = Some(PluginGuiKey::Effect {
                    track_id,
                    effect_id,
                });
                action.status = Some("Removed effect".to_string());
            }
            DevicesMsg::SetEffectParam(track_id, effect_id, param_index, value) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        if param_index < effect.params.len() {
                            let desc = &effect.descriptors[param_index];
                            let clamped = value.clamp(desc.min, desc.max);
                            effect.params[param_index] = clamped;
                            engine.send(EngineCommand::SetEffectParam {
                                track_id,
                                effect_id,
                                param_index,
                                value: clamped,
                            });
                        }
                    }
                }
            }
            DevicesMsg::SetEffectParams(track_id, effect_id, updates) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        for (param_index, value) in updates {
                            if param_index < effect.params.len() {
                                let desc = &effect.descriptors[param_index];
                                let clamped = value.clamp(desc.min, desc.max);
                                effect.params[param_index] = clamped;
                                engine.send(EngineCommand::SetEffectParam {
                                    track_id,
                                    effect_id,
                                    param_index,
                                    value: clamped,
                                });
                            }
                        }
                    }
                }
            }
            DevicesMsg::ToggleEffectBypass(track_id, effect_id) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(effect) = track.effects.iter_mut().find(|e| e.id == effect_id) {
                        effect.bypass = !effect.bypass;
                        engine.send(EngineCommand::SetEffectBypass {
                            track_id,
                            effect_id,
                            bypass: effect.bypass,
                        });
                    }
                }
            }
            DevicesMsg::MoveEffectUp(track_id, effect_id) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx > 0 {
                            track.effects.swap(idx, idx - 1);
                            engine.send(EngineCommand::MoveEffect {
                                track_id,
                                effect_id,
                                new_index: idx - 1,
                            });
                        }
                    }
                }
            }
            DevicesMsg::MoveEffectDown(track_id, effect_id) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(idx) = track.effects.iter().position(|e| e.id == effect_id) {
                        if idx + 1 < track.effects.len() {
                            track.effects.swap(idx, idx + 1);
                            engine.send(EngineCommand::MoveEffect {
                                track_id,
                                effect_id,
                                new_index: idx + 1,
                            });
                        }
                    }
                }
            }
            DevicesMsg::SetTrackInstrument(track_id, instrument_kind) => {
                if track_id.is_master() || buses.iter().any(|b| b.id == track_id) {
                    // The master and buses host effects only.
                    return action;
                }
                let instrument_params =
                    default_instrument_params(instrument_kind, sample_rate as f32);
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    track.has_instrument = true;
                    track.instrument_kind = Some(instrument_kind);
                    track.sample_name = None;
                    track.sample_source = None;
                    track.sample_audio = None;
                    track.instrument_params = instrument_params.clone();
                    track.drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
                    track.selected_drum_pad = 0;
                }
                engine.send(EngineCommand::SetTrackInstrument(track_id, instrument_kind));
                for (param_index, value) in instrument_params.into_iter().enumerate() {
                    engine.send(EngineCommand::SetInstrumentParam {
                        track_id,
                        param_index,
                        value,
                    });
                }
                self.context_menu = None;
                action.status = Some(format!("Added {}", instrument_kind.name()));
            }
            DevicesMsg::RemoveTrackInstrument(track_id) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
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
                engine.send(EngineCommand::RemoveTrackInstrument(track_id));
                action.close_gui = Some(PluginGuiKey::Instrument { track_id });
                action.status = Some("Removed instrument".to_string());
            }
            DevicesMsg::SetInstrumentParam(track_id, param_index, value) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if param_index < track.instrument_params.len() {
                        track.instrument_params[param_index] = value;
                    }
                }
                engine.send(EngineCommand::SetInstrumentParam {
                    track_id,
                    param_index,
                    value,
                });
                action.status = Some(format!("Param {param_index} = {value:.2}"));
            }
            DevicesMsg::SelectDrumRackPad(track_id, pad_index) => {
                // Audition the pad like Ableton: hear it on click.
                let pitch = 36 + pad_index.min(127) as u8;
                engine.send(EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity: 100,
                    on: true,
                });
                engine.send(EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity: 100,
                    on: false,
                });
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    let max_index = track.drum_rack_pads.len().saturating_sub(1);
                    track.selected_drum_pad = pad_index.min(max_index);
                }
                action.select_track = Some(track_id);
            }
            DevicesMsg::ClearDrumRackPad(track_id, pad_index) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        *pad = UiDrumPad::default();
                    }
                }
                sync_pad(tracks, track_id, pad_index, engine);
                engine.send(EngineCommand::ClearDrumRackPad {
                    track_id,
                    pad_index,
                });
                action.status = Some(format!("Cleared pad {}", pad_index + 1));
            }
            DevicesMsg::SetDrumPadParam {
                track_id,
                pad_index,
                param,
                value,
            } => {
                let mut changed = false;
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        match param {
                            DrumPadParam::Gain => pad.gain = value.clamp(0.0, 2.0),
                            DrumPadParam::Pan => pad.pan = value.clamp(-1.0, 1.0),
                            DrumPadParam::Start => pad.start = value.clamp(0.0, 1.0),
                            DrumPadParam::End => pad.end = value.clamp(0.0, 1.0),
                            DrumPadParam::CoarseTune => {
                                pad.coarse_tune = value.clamp(-24.0, 24.0).round() as i8;
                            }
                            DrumPadParam::FineTune => {
                                pad.fine_tune = value.clamp(-100.0, 100.0);
                            }
                        }
                        changed = true;
                    }
                }
                if changed {
                    sync_pad(tracks, track_id, pad_index, engine);
                    action.status = Some(format!("Pad {} updated", pad_index + 1));
                }
            }
            DevicesMsg::SetDrumPadOneShot {
                track_id,
                pad_index,
                one_shot,
            } => {
                let mut changed = false;
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        pad.one_shot = one_shot;
                        changed = true;
                    }
                }
                if changed {
                    sync_pad(tracks, track_id, pad_index, engine);
                    action.status = Some(format!("Pad {} updated", pad_index + 1));
                }
            }
            DevicesMsg::SetDrumPadChokeGroup {
                track_id,
                pad_index,
                choke_group,
            } => {
                let mut changed = false;
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    if let Some(pad) = track.drum_rack_pads.get_mut(pad_index) {
                        pad.choke_group = choke_group;
                        changed = true;
                    }
                }
                if changed {
                    sync_pad(tracks, track_id, pad_index, engine);
                    action.status = Some(format!("Pad {} updated", pad_index + 1));
                }
            }
            DevicesMsg::AuditionNote {
                track_id,
                pitch,
                on,
            } => {
                engine.send(EngineCommand::AuditionNote {
                    track_id,
                    pitch,
                    velocity: 100,
                    on,
                });
            }
            DevicesMsg::ShowContextMenu { x, y, track_id } => {
                let is_midi = tracks
                    .iter()
                    .find(|t| t.id == track_id)
                    .is_some_and(|t| t.kind.is_midi());
                self.context_menu = Some(DeviceContextMenu {
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
            DevicesMsg::DismissContextMenu => {
                self.context_menu = None;
            }
            DevicesMsg::SetMenuCategory(category) => {
                if let Some(ref mut menu) = self.context_menu {
                    menu.category = Some(category);
                }
            }
            DevicesMsg::MenuSearch(query) => {
                if let Some(ref mut menu) = self.context_menu {
                    menu.search = query;
                }
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;

    fn midi_track_with_effect() -> (Vec<ProjectTrack>, TrackId, EffectId) {
        let track_id = TrackId::new();
        let mut track = ProjectTrack::new_instrument(
            track_id,
            "MIDI 1".to_string(),
            vibez_core::midi::TrackKind::Midi,
            0,
        );
        let effect_id = EffectId::new();
        track.effects.push(UiEffect {
            id: effect_id,
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![1.0],
            descriptors: vibez_dsp::factory::create_effect(EffectType::Gain, 44_100.0)
                .param_descriptors(),
            plugin_name: None,
            has_plugin_gui: false,
            plugin_ref: None,
        });
        (vec![track], track_id, effect_id)
    }

    #[test]
    fn remove_effect_requests_gui_teardown() {
        let (mut tracks, track_id, effect_id) = midi_track_with_effect();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        let action = devices.update(
            DevicesMsg::RemoveEffect(track_id, effect_id),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        assert!(tracks[0].effects.is_empty());
        assert_eq!(
            action.close_gui,
            Some(PluginGuiKey::Effect {
                track_id,
                effect_id
            })
        );
        assert!(matches!(engine.0[0], EngineCommand::RemoveEffect(..)));
    }

    #[test]
    fn effect_param_clamps_to_descriptor_range() {
        let (mut tracks, track_id, effect_id) = midi_track_with_effect();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        devices.update(
            DevicesMsg::SetEffectParam(track_id, effect_id, 0, 9999.0),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        let max = tracks[0].effects[0].descriptors[0].max;
        assert_eq!(tracks[0].effects[0].params[0], max);
    }

    #[test]
    fn move_effect_up_at_top_is_a_no_op() {
        let (mut tracks, track_id, effect_id) = midi_track_with_effect();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        devices.update(
            DevicesMsg::MoveEffectUp(track_id, effect_id),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        assert!(engine.0.is_empty());
    }

    #[test]
    fn pad_click_auditions_and_selects_track() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        let action = devices.update(
            DevicesMsg::SelectDrumRackPad(track_id, 3),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        assert_eq!(tracks[0].selected_drum_pad, 3);
        assert_eq!(action.select_track, Some(track_id));
        assert!(matches!(
            engine.0[0],
            EngineCommand::AuditionNote {
                pitch: 39,
                on: true,
                ..
            }
        ));
    }

    #[test]
    fn pad_param_edit_syncs_full_pad_state() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        devices.update(
            DevicesMsg::SetDrumPadParam {
                track_id,
                pad_index: 0,
                param: DrumPadParam::Gain,
                value: 5.0,
            },
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        assert_eq!(tracks[0].drum_rack_pads[0].gain, 2.0); // clamped
        assert!(matches!(
            engine.0[0],
            EngineCommand::SetDrumRackPadState { .. }
        ));
    }

    #[test]
    fn menu_lifecycle() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        devices.update(
            DevicesMsg::ShowContextMenu {
                x: 10.0,
                y: 20.0,
                track_id,
            },
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        // MIDI track opens on the Instruments tab.
        assert_eq!(
            devices.context_menu.as_ref().and_then(|m| m.category),
            Some(DeviceMenuCategory::Instruments)
        );
        devices.update(
            DevicesMsg::DismissContextMenu,
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            44_100,
        );
        assert!(devices.context_menu.is_none());
    }

    #[test]
    fn dirty_classification() {
        assert!(DevicesMsg::AddEffect(TrackId::new(), EffectType::Gain).marks_dirty());
        assert!(!DevicesMsg::DismissContextMenu.marks_dirty());
        assert!(!DevicesMsg::AuditionNote {
            track_id: TrackId::new(),
            pitch: 60,
            on: true
        }
        .marks_dirty());
    }
}
