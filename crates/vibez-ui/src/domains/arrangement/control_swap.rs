//! Brings the engine's copy of one channel's mixer and device values to a target with control commands.

use vibez_engine::commands::EngineCommand;

use crate::state::{ProjectTrack, UiEffect};

use super::EngineHandle;

/// `current` and `target` share a channel layout
/// ([`crate::state::ProjectTracksState::same_channel_layout`]); only values
/// that differ are sent.
pub fn swap_changed_controls(
    engine: &mut impl EngineHandle,
    current: &ProjectTrack,
    target: &ProjectTrack,
) {
    // Every field is named, as in `same_channel_layout`, so a new one has to
    // be sent here or held equal there.
    let ProjectTrack {
        id: track_id,
        gain,
        pan,
        mute,
        solo,
        swing_offset,
        sends,
        effects,
        instrument_params,
        name: _,
        audio_input_route: _,
        input_monitoring: _,
        peak_l: _,
        peak_r: _,
        kind: _,
        color_index: _,
        has_instrument: _,
        instrument_kind: _,
        sample_name: _,
        sample_source: _,
        sample_audio: _,
        drum_rack_pads: _,
        selected_drum_pad: _,
        plugin_instrument_name: _,
        plugin_instrument_ref: _,
        plugin_instrument_descriptors: _,
        has_plugin_instrument_gui: _,
        instrument_latency_samples: _,
    } = target;
    let track_id = *track_id;
    if current.gain != *gain {
        engine.send(EngineCommand::SetTrackGain(track_id, *gain));
    }
    if current.pan != *pan {
        engine.send(EngineCommand::SetTrackPan(track_id, *pan));
    }
    if current.mute != *mute {
        engine.send(EngineCommand::SetTrackMute(track_id, *mute));
    }
    if current.solo != *solo {
        engine.send(EngineCommand::SetTrackSolo(track_id, *solo));
    }
    if current.swing_offset != *swing_offset {
        engine.send(EngineCommand::SetTrackSwingOffset(track_id, *swing_offset));
    }
    for (&(bus_id, amount), &(_, current_amount)) in sends.iter().zip(&current.sends) {
        if amount != current_amount {
            engine.send(EngineCommand::SetSend {
                track_id,
                bus_id,
                amount,
            });
        }
    }
    for (effect, current_effect) in effects.iter().zip(&current.effects) {
        let UiEffect {
            id: effect_id,
            bypass,
            params,
            reconfiguration_failed: _,
            latency_samples: _,
            effect_type: _,
            descriptors: _,
            plugin_name: _,
            has_plugin_gui: _,
            plugin_ref: _,
            sidechains: _,
            inactive_sidechains: _,
            external_inputs: _,
        } = effect;
        if current_effect.bypass != *bypass {
            engine.send(EngineCommand::SetEffectBypass {
                track_id,
                effect_id: *effect_id,
                bypass: *bypass,
            });
        }
        for (param_index, (&value, &current_value)) in
            params.iter().zip(&current_effect.params).enumerate()
        {
            if value != current_value {
                engine.send(EngineCommand::SetEffectParam {
                    track_id,
                    effect_id: *effect_id,
                    param_index,
                    value,
                });
            }
        }
    }
    for (param_index, (&value, &current_value)) in instrument_params
        .iter()
        .zip(&current.instrument_params)
        .enumerate()
    {
        if value != current_value {
            engine.send(EngineCommand::SetInstrumentParam {
                track_id,
                param_index,
                value,
            });
        }
    }
}
