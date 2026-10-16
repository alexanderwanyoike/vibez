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

use crate::domains::device_params::DeviceSlot;
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
    SetSidechainSource {
        track_id: TrackId,
        effect_id: EffectId,
        input_id: vibez_core::routing::ExternalInputId,
        source: Option<TrackId>,
    },
    SetSidechainTap {
        track_id: TrackId,
        effect_id: EffectId,
        input_id: vibez_core::routing::ExternalInputId,
        tap: vibez_core::routing::SourceTap,
    },
    MoveEffectUp(TrackId, EffectId),
    MoveEffectDown(TrackId, EffectId),
    SetTrackInstrument(TrackId, InstrumentKind),
    RemoveTrackInstrument(TrackId),
    SetInstrumentParam(TrackId, usize, f32),
    /// Several instrument values as one edit, as a preset applies them.
    SetInstrumentParams(TrackId, Vec<(usize, f32)>),
    SelectDrumRackPad(TrackId, usize),
    SelectDrumRackBank(TrackId, usize),
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
    /// Several of a pad's Sampler values as one edit.
    SetDrumPadParams(TrackId, usize, Vec<(usize, f32)>),
    /// Names a pad; an empty name goes back to its sample's.
    RenameDrumPad(TrackId, usize, String),
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
                | DevicesMsg::SelectDrumRackBank(..)
                | DevicesMsg::ShowContextMenu { .. }
                | DevicesMsg::DismissContextMenu
                | DevicesMsg::SetMenuCategory(_)
                | DevicesMsg::MenuSearch(_)
        )
    }
}

/// The device and parameter values a parameter edit asks for.
pub struct ParamEdit {
    pub track_id: TrackId,
    pub slot: DeviceSlot,
    pub updates: Vec<(usize, f32)>,
}

impl DevicesMsg {
    pub fn param_edit(&self) -> Option<ParamEdit> {
        let (track_id, slot, updates) = match self {
            DevicesMsg::SetEffectParam(track, effect, index, value) => {
                (*track, DeviceSlot::Effect(*effect), vec![(*index, *value)])
            }
            DevicesMsg::SetEffectParams(track, effect, updates) => {
                (*track, DeviceSlot::Effect(*effect), updates.clone())
            }
            DevicesMsg::SetInstrumentParam(track, index, value) => {
                (*track, DeviceSlot::Instrument, vec![(*index, *value)])
            }
            DevicesMsg::SetInstrumentParams(track, updates) => {
                (*track, DeviceSlot::Instrument, updates.clone())
            }
            DevicesMsg::SetDrumPadParams(track, pad, updates) => {
                (*track, DeviceSlot::DrumPad(*pad), updates.clone())
            }
            _ => return None,
        };
        Some(ParamEdit {
            track_id,
            slot,
            updates,
        })
    }
}

/// An edit that leaves every value as it is once clamped to its range is no
/// change (PATTERNS §7). Checked before project state is copied for writing.
pub fn is_unchanged_param_edit(
    project: &crate::state::ProjectTracksState,
    message: &DevicesMsg,
) -> bool {
    let Some(edit) = message.param_edit() else {
        return false;
    };
    let Some(device) = project
        .find(edit.track_id)
        .and_then(|track| crate::domains::device_params::DeviceParams::find(track, edit.slot))
    else {
        return false;
    };
    edit.updates.iter().all(|&(index, value)| {
        device
            .descriptors
            .get(index)
            .zip(device.params.get(index))
            .is_some_and(|(descriptor, current)| {
                value.clamp(descriptor.min, descriptor.max) == *current
            })
    })
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
    pub routing_changed: Option<bool>,
}

/// Devices domain state slice: currently just the context menu; the
/// device data itself lives on the shared track model.
#[derive(Debug, Default)]
pub struct DevicesState {
    pub context_menu: Option<DeviceContextMenu>,
    pub last_routing: Option<Vec<vibez_core::routing::RoutingChannel>>,
    pub last_timing: Option<crate::domains::compensation::TimingSignature>,
    pub compensation_generation: u64,
    pub sidechain_choices: super::sidechain::SidechainChoiceCache,
    pub sidechain_meters:
        std::collections::HashMap<(EffectId, vibez_core::routing::ExternalInputId), (f32, f32)>,
}

/// Default parameter values for a freshly added native instrument.
/// Stores instrument values. A track saved before its instrument gained
/// controls holds fewer values; the missing ones take their defaults first,
/// so edits to the newer controls are kept.
fn store_instrument_values(track: &mut crate::state::ProjectTrack, updates: &[(usize, f32)]) {
    let descriptors = track
        .instrument_kind
        .map_or(&[][..], vibez_instruments::descriptors_for);
    for &(index, value) in updates {
        let stored = track.instrument_params.len();
        if index >= stored && index < descriptors.len() {
            track
                .instrument_params
                .extend(descriptors[stored..].iter().map(|d| d.default));
        }
        if let Some(slot) = track.instrument_params.get_mut(index) {
            *slot = value;
        }
    }
}

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

pub const DEVICE_CHAIN_FULL: &str = "This channel's device chain is full";

#[derive(Default)]
pub struct DevicesCtx<'a> {
    pub sample_rate: u32,
    pub routing: Option<&'a [vibez_core::routing::RoutingChannel]>,
}

impl DevicesState {
    pub fn update(
        &mut self,
        msg: DevicesMsg,
        engine: &mut impl EngineHandle,
        tracks: &mut [ProjectTrack],
        master: &mut ProjectTrack,
        buses: &mut [ProjectTrack],
        ctx: DevicesCtx<'_>,
    ) -> DevicesAction {
        let mut action = DevicesAction::default();
        let sample_rate = ctx.sample_rate;
        match msg {
            DevicesMsg::SetSidechainSource {
                track_id,
                effect_id,
                input_id,
                source,
            } => {
                action.routing_changed = Some(ctx.routing.is_some_and(|routing| {
                    super::sidechain::edit_source_with_model(
                        tracks,
                        master,
                        buses,
                        (track_id, effect_id, input_id),
                        source,
                        routing,
                    )
                }));
            }
            DevicesMsg::SetSidechainTap {
                track_id,
                effect_id,
                input_id,
                tap,
            } => {
                action.routing_changed = Some(ctx.routing.is_some_and(|routing| {
                    super::sidechain::edit_tap_with_model(
                        tracks,
                        master,
                        buses,
                        (track_id, effect_id, input_id),
                        tap,
                        routing,
                    )
                }));
            }
            DevicesMsg::AddEffect(track_id, effect_type) => {
                if find_track_mut(tracks, master, buses, track_id)
                    .is_some_and(|track| !track.has_device_room())
                {
                    self.context_menu = None;
                    action.status = Some(DEVICE_CHAIN_FULL.to_string());
                    return action;
                }
                let effect_id = EffectId::new();
                let fx = vibez_dsp::factory::create_effect(effect_type, sample_rate as f32);
                let descriptors = fx.param_descriptors();
                let params: Vec<f32> = descriptors.iter().map(|d| d.default).collect();

                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    track.effects.push(UiEffect {
                        reconfiguration_failed: false,
                        latency_samples: Some(fx.latency_samples()),
                        inactive_sidechains: Default::default(),
                        sidechains: Default::default(),
                        external_inputs: fx.external_inputs().to_vec(),

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
                engine.insert_built_in_effect(track_id, effect_id, effect_type);
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
                let mut replaced_plugin = false;
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    replaced_plugin = track.plugin_instrument_ref.is_some()
                        || track.plugin_instrument_name.is_some()
                        || !track.plugin_instrument_descriptors.is_empty()
                        || track.has_plugin_instrument_gui;
                    track.has_instrument = true;
                    track.instrument_latency_samples = Some(0);
                    track.instrument_kind = Some(instrument_kind);
                    track.sample_name = None;
                    track.sample_source = None;
                    track.sample_audio = None;
                    track.instrument_params = instrument_params.clone();
                    track.drum_rack_pads = crate::state::default_drum_rack_pads();
                    track.selected_drum_pad = 0;
                    track.plugin_instrument_name = None;
                    track.plugin_instrument_ref = None;
                    track.plugin_instrument_descriptors = &[];
                    track.has_plugin_instrument_gui = false;
                }
                engine.set_built_in_instrument(track_id, instrument_kind);
                for (param_index, value) in instrument_params.into_iter().enumerate() {
                    engine.send(EngineCommand::SetInstrumentParam {
                        track_id,
                        param_index,
                        value,
                    });
                }
                self.context_menu = None;
                if replaced_plugin {
                    action.close_gui = Some(PluginGuiKey::Instrument { track_id });
                }
                action.status = Some(format!("Added {}", instrument_kind.name()));
            }
            DevicesMsg::RemoveTrackInstrument(track_id) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    track.has_instrument = false;
                    track.instrument_latency_samples = None;
                    track.instrument_kind = None;
                    track.sample_name = None;
                    track.sample_source = None;
                    track.sample_audio = None;
                    track.instrument_params.clear();
                    track.drum_rack_pads = crate::state::default_drum_rack_pads();
                    track.selected_drum_pad = 0;
                    track.plugin_instrument_name = None;
                    track.plugin_instrument_ref = None;
                    track.plugin_instrument_descriptors = &[];
                    track.has_plugin_instrument_gui = false;
                }
                engine.send(EngineCommand::RemoveTrackInstrument(track_id));
                action.close_gui = Some(PluginGuiKey::Instrument { track_id });
                action.status = Some("Removed instrument".to_string());
            }
            DevicesMsg::SetInstrumentParam(track_id, param_index, value) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    store_instrument_values(track, &[(param_index, value)]);
                }
                engine.send(EngineCommand::SetInstrumentParam {
                    track_id,
                    param_index,
                    value,
                });
                action.status = Some(format!("Param {param_index} = {value:.2}"));
            }
            DevicesMsg::SetInstrumentParams(track_id, updates) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    store_instrument_values(track, &updates);
                }
                for (param_index, value) in updates {
                    engine.send(EngineCommand::SetInstrumentParam {
                        track_id,
                        param_index,
                        value,
                    });
                }
            }
            DevicesMsg::SelectDrumRackPad(track_id, pad_index) => {
                // Audition the pad like Ableton: hear it on click.
                let Some(pitch) = vibez_core::track::drum_rack_pad_pitch(pad_index) else {
                    return action;
                };
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
            DevicesMsg::SelectDrumRackBank(track_id, bank_index) => {
                if let Some(track) = find_track_mut(tracks, master, buses, track_id) {
                    let bank_index =
                        bank_index.min(vibez_core::track::DRUM_RACK_BANK_COUNT.saturating_sub(1));
                    let slot = track.selected_drum_pad % vibez_core::track::DRUM_RACK_BANK_SIZE;
                    track.selected_drum_pad = bank_index
                        .saturating_mul(vibez_core::track::DRUM_RACK_BANK_SIZE)
                        .saturating_add(slot)
                        .min(track.drum_rack_pads.len().saturating_sub(1));
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
                            DrumPadParam::FadeIn => {
                                pad.fade_in_ms =
                                    value.clamp(0.0, vibez_core::track::DRUM_PAD_MAX_FADE_MS);
                            }
                            DrumPadParam::FadeOut => {
                                pad.fade_out_ms =
                                    value.clamp(0.0, vibez_core::track::DRUM_PAD_MAX_FADE_MS);
                            }
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
            DevicesMsg::SetDrumPadParams(track_id, pad_index, updates) => {
                let Some(pad) = find_track_mut(tracks, master, buses, track_id)
                    .and_then(|track| track.drum_rack_pads.get_mut(pad_index))
                else {
                    return action;
                };
                let params = pad.sampler_params_mut();
                for &(index, value) in &updates {
                    if let Some(slot) = params.get_mut(index) {
                        *slot = value;
                    }
                }
                for (param_index, value) in updates {
                    engine.send(EngineCommand::SetDrumRackPadParam {
                        track_id,
                        pad_index,
                        param_index,
                        value,
                    });
                }
            }
            DevicesMsg::RenameDrumPad(track_id, pad_index, name) => {
                if let Some(pad) = find_track_mut(tracks, master, buses, track_id)
                    .and_then(|track| track.drum_rack_pads.get_mut(pad_index))
                {
                    let name = name.trim();
                    pad.name = if name.is_empty() {
                        pad.source
                            .as_ref()
                            .map(vibez_core::track::MediaSourceRef::display_name)
                    } else {
                        Some(name.to_string())
                    };
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

    fn surge_device() -> vibez_core::effect::PluginDeviceInfo {
        vibez_core::effect::PluginDeviceInfo {
            format: "clap".to_string(),
            uid: "org.surge-synth-team.surge-xt".to_string(),
            path: "/usr/lib/clap/Surge XT.clap".into(),
            name: "Surge XT".to_string(),
            state_b64: Some("plugin-state".to_string()),
        }
    }

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
            reconfiguration_failed: false,
            latency_samples: Some(0),
            inactive_sidechains: Default::default(),
            sidechains: Default::default(),
            external_inputs: Default::default(),
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
    fn a_full_device_chain_refuses_another_device_without_reaching_the_engine() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        let template = tracks[0].effects[0].clone();
        while tracks[0].has_device_room() {
            let mut effect = template.clone();
            effect.id = EffectId::new();
            tracks[0].effects.push(effect);
        }
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        let action = devices.update(
            DevicesMsg::AddEffect(track_id, EffectType::Reverb),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );
        assert_eq!(
            tracks[0].effects.len(),
            vibez_engine::mixer::DEVICE_CHAIN_CAPACITY
        );
        assert!(engine.0.is_empty());
        assert_eq!(action.status.as_deref(), Some(DEVICE_CHAIN_FULL));
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
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
    fn selecting_sampler_atomically_replaces_a_plugin_instrument() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].has_instrument = true;
        tracks[0].plugin_instrument_name = Some("Surge XT".to_string());
        tracks[0].plugin_instrument_ref = Some(surge_device());
        tracks[0].plugin_instrument_descriptors =
            vibez_dsp::factory::create_effect(EffectType::Gain, 44_100.0).param_descriptors();
        tracks[0].has_plugin_instrument_gui = true;
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();

        let action = devices.update(
            DevicesMsg::SetTrackInstrument(track_id, InstrumentKind::Sampler),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );

        assert_eq!(tracks[0].instrument_kind, Some(InstrumentKind::Sampler));
        assert!(tracks[0].plugin_instrument_name.is_none());
        assert!(tracks[0].plugin_instrument_ref.is_none());
        assert!(tracks[0].plugin_instrument_descriptors.is_empty());
        assert!(!tracks[0].has_plugin_instrument_gui);
        assert_eq!(
            action.close_gui,
            Some(PluginGuiKey::Instrument { track_id })
        );
    }

    #[test]
    fn removing_plugin_instrument_clears_its_persisted_identity() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].has_instrument = true;
        tracks[0].plugin_instrument_name = Some("Surge XT".to_string());
        tracks[0].plugin_instrument_ref = Some(surge_device());
        tracks[0].plugin_instrument_descriptors =
            vibez_dsp::factory::create_effect(EffectType::Gain, 44_100.0).param_descriptors();
        tracks[0].has_plugin_instrument_gui = true;
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();

        devices.update(
            DevicesMsg::RemoveTrackInstrument(track_id),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );

        assert!(tracks[0].plugin_instrument_name.is_none());
        assert!(tracks[0].plugin_instrument_ref.is_none());
        assert!(tracks[0].plugin_instrument_descriptors.is_empty());
        assert!(!tracks[0].has_plugin_instrument_gui);
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
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
    fn rack_bank_navigation_preserves_the_selected_slot_without_auditioning() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].drum_rack_pads = crate::state::default_drum_rack_pads();
        tracks[0].selected_drum_pad = 3;
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();

        devices.update(
            DevicesMsg::SelectDrumRackBank(track_id, 1),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );

        assert_eq!(tracks[0].selected_drum_pad, 19);
        assert!(engine.0.is_empty());
        assert!(!DevicesMsg::SelectDrumRackBank(track_id, 2).marks_dirty());
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );
        assert_eq!(tracks[0].drum_rack_pads[0].gain, 2.0); // clamped
        assert!(matches!(
            engine.0[0],
            EngineCommand::SetDrumRackPadState { .. }
        ));

        devices.update(
            DevicesMsg::SetDrumPadParam {
                track_id,
                pad_index: 0,
                param: DrumPadParam::FadeOut,
                value: 500.0,
            },
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );
        assert_eq!(
            tracks[0].drum_rack_pads[0].fade_out_ms,
            vibez_core::track::DRUM_PAD_MAX_FADE_MS
        );
        assert!(matches!(
            engine.0[1],
            EngineCommand::SetDrumRackPadState {
                ref state,
                ..
            } if state.fade_out_ms == vibez_core::track::DRUM_PAD_MAX_FADE_MS
        ));
    }

    #[test]
    fn a_pad_edit_sets_its_sampler_values_from_the_ones_it_was_saved_with() {
        use vibez_instruments::sampler::param;
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
        tracks[0].drum_rack_pads[2] = UiDrumPad::from_state(&vibez_core::track::DrumPadState {
            coarse_tune: 5,
            ..Default::default()
        });
        tracks[0].drum_rack_pads[2].params.clear();
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        devices.update(
            DevicesMsg::SetDrumPadParams(track_id, 2, vec![(param::PAN, -0.5)]),
            &mut engine,
            &mut tracks,
            &mut crate::state::new_master_track(),
            &mut [],
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
        );

        let params = &tracks[0].drum_rack_pads[2].params;
        assert_eq!(params.len(), param::COUNT);
        assert_eq!(params[param::PAN], -0.5);
        assert_eq!(params[param::TUNE], 5.0, "the saved tuning carried over");
        assert!(matches!(
            engine.0[..],
            [EngineCommand::SetDrumRackPadParam {
                pad_index: 2,
                param_index: param::PAN,
                value,
                ..
            }] if value == -0.5
        ));
    }

    #[test]
    fn renaming_a_pad_keeps_a_name_and_an_empty_one_falls_back_to_the_sample() {
        let (mut tracks, track_id, _) = midi_track_with_effect();
        tracks[0].drum_rack_pads = (0..16).map(|_| UiDrumPad::default()).collect();
        tracks[0].drum_rack_pads[1].source = Some(vibez_core::track::MediaSourceRef::LocalFile {
            path: "/kits/snare_02.wav".into(),
        });
        let mut devices = DevicesState::default();
        let mut engine = RecordingEngine::default();
        let mut rename = |tracks: &mut Vec<ProjectTrack>, name: &str| {
            devices.update(
                DevicesMsg::RenameDrumPad(track_id, 1, name.into()),
                &mut engine,
                tracks,
                &mut crate::state::new_master_track(),
                &mut [],
                DevicesCtx::default(),
            );
        };
        rename(&mut tracks, "  Snare  ");
        let renamed = tracks[0].drum_rack_pads[1].name.clone();
        rename(&mut tracks, "   ");

        assert_eq!(renamed.as_deref(), Some("Snare"));
        assert_eq!(
            tracks[0].drum_rack_pads[1].name.as_deref(),
            Some("snare_02.wav")
        );
        assert!(engine.0.is_empty());
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
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
            DevicesCtx {
                sample_rate: 44_100,
                ..Default::default()
            },
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
