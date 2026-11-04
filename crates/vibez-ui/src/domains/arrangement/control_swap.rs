//! Brings the engine's copy of one channel's mixer and device values to a target with control commands.

use vibez_engine::commands::EngineCommand;

use crate::state::ProjectTrack;

use super::EngineHandle;

/// `current` and `target` share a channel layout
/// ([`crate::state::ProjectTracksState::same_channel_layout`]); only values
/// that differ are sent.
pub fn swap_changed_controls(
    engine: &mut impl EngineHandle,
    current: &ProjectTrack,
    target: &ProjectTrack,
) {
    let track_id = target.id;
    if current.gain != target.gain {
        engine.send(EngineCommand::SetTrackGain(track_id, target.gain));
    }
    if current.pan != target.pan {
        engine.send(EngineCommand::SetTrackPan(track_id, target.pan));
    }
    if current.mute != target.mute {
        engine.send(EngineCommand::SetTrackMute(track_id, target.mute));
    }
    if current.solo != target.solo {
        engine.send(EngineCommand::SetTrackSolo(track_id, target.solo));
    }
    if current.swing_offset != target.swing_offset {
        engine.send(EngineCommand::SetTrackSwingOffset(
            track_id,
            target.swing_offset,
        ));
    }
    for (&(bus_id, amount), &(_, current_amount)) in target.sends.iter().zip(&current.sends) {
        if amount != current_amount {
            engine.send(EngineCommand::SetSend {
                track_id,
                bus_id,
                amount,
            });
        }
    }
    for (effect, current_effect) in target.effects.iter().zip(&current.effects) {
        if effect.bypass != current_effect.bypass {
            engine.send(EngineCommand::SetEffectBypass {
                track_id,
                effect_id: effect.id,
                bypass: effect.bypass,
            });
        }
        for (param_index, (&value, &current_value)) in
            effect.params.iter().zip(&current_effect.params).enumerate()
        {
            if value != current_value {
                engine.send(EngineCommand::SetEffectParam {
                    track_id,
                    effect_id: effect.id,
                    param_index,
                    value,
                });
            }
        }
    }
    for (param_index, (&value, &current_value)) in target
        .instrument_params
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
