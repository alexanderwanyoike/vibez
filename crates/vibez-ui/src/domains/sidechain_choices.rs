//! Per-device view cache of the shared core routing choices.
use std::collections::HashMap;
pub use vibez_core::routing::InputSourceChoice;
use vibez_core::{
    id::EffectId,
    routing::{ExternalInputId, RoutingChannel},
};
pub type SidechainChoiceCache = HashMap<(EffectId, ExternalInputId), Vec<InputSourceChoice>>;

pub fn input_source_choices(channels: &[RoutingChannel]) -> SidechainChoiceCache {
    channels
        .iter()
        .flat_map(|receiver| {
            receiver.effects.iter().flat_map(move |effect| {
                effect
                    .inputs
                    .iter()
                    .filter(|input| input.supported())
                    .map(move |input| {
                        (
                            (effect.id, input.id),
                            vibez_core::routing::input_source_choices(
                                channels,
                                receiver.id,
                                effect.id,
                                input.id,
                            ),
                        )
                    })
            })
        })
        .collect()
}
