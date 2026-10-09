use vibez_core::id::{EffectId, TrackId};
use vibez_core::routing::{
    ExternalInputId, RoutingChannel, RoutingEffect, RoutingGraph, SidechainAssignment, SourceTap,
};

use crate::state::ProjectTrack;

pub fn routing_channels(
    tracks: &[ProjectTrack],
    master: &ProjectTrack,
    buses: &[ProjectTrack],
) -> Vec<RoutingChannel> {
    tracks
        .iter()
        .map(|track| channel(track, false))
        .chain(buses.iter().map(|track| channel(track, true)))
        .chain(std::iter::once(channel(master, true)))
        .collect()
}

fn channel(track: &ProjectTrack, is_bus: bool) -> RoutingChannel {
    RoutingChannel {
        id: track.id,
        is_bus,
        sends: track.sends.clone(),
        effects: track
            .effects
            .iter()
            .map(|effect| RoutingEffect {
                id: effect.id,
                inputs: effect.external_inputs.clone(),
                assignments: effect.sidechains.clone(),
            })
            .collect(),
    }
}

pub fn valid_taps(
    channels: &[RoutingChannel],
    receiver: TrackId,
    effect_id: EffectId,
    input_id: ExternalInputId,
    source: TrackId,
) -> Vec<SourceTap> {
    if source.is_master() || !channels.iter().any(|channel| channel.id == source) {
        return Vec::new();
    }
    [
        SourceTap::BeforeEffects,
        SourceTap::AfterEffects,
        SourceTap::AfterFader,
    ]
    .into_iter()
    .filter(|tap| {
        let mut candidate = channels.to_vec();
        let Some(effect) = candidate
            .iter_mut()
            .find(|channel| channel.id == receiver)
            .and_then(|channel| {
                channel
                    .effects
                    .iter_mut()
                    .find(|effect| effect.id == effect_id)
            })
        else {
            return false;
        };
        let Some(input) = effect
            .inputs
            .iter()
            .find(|input| input.id == input_id && input.supported())
        else {
            return false;
        };
        let input_name = input.name.clone();
        effect
            .assignments
            .retain(|route| route.input_id != input_id);
        effect.assignments.push(SidechainAssignment {
            input_id,
            input_name,
            source,
            source_name: String::new(),
            tap: *tap,
        });
        RoutingGraph::prepare(&candidate).is_ok()
    })
    .collect()
}

#[cfg(test)]
pub fn edit_source(
    tracks: &mut [ProjectTrack],
    master: &mut ProjectTrack,
    buses: &mut [ProjectTrack],
    receiver: TrackId,
    effect_id: EffectId,
    input_id: ExternalInputId,
    source: Option<TrackId>,
) -> bool {
    let channels = routing_channels(tracks, master, buses);
    edit_source_with_model(
        tracks, master, buses, (receiver, effect_id, input_id), source, &channels,
    )
}

pub fn edit_source_with_model(
    tracks: &mut [ProjectTrack],
    master: &mut ProjectTrack,
    buses: &mut [ProjectTrack],
    target: (TrackId, EffectId, ExternalInputId),
    source: Option<TrackId>,
    channels: &[RoutingChannel],
) -> bool {
    let (receiver, effect_id, input_id) = target;
    let source_name = source.and_then(|id| {
        tracks
            .iter()
            .chain(buses.iter())
            .find(|track| track.id == id)
            .map(|track| track.name.clone())
    });
    let taps = source.map(|id| valid_taps(channels, receiver, effect_id, input_id, id));
    let Some(effect) = tracks
        .iter_mut()
        .chain(buses.iter_mut())
        .chain(std::iter::once(master))
        .find(|track| track.id == receiver)
        .and_then(|track| {
            track
                .effects
                .iter_mut()
                .find(|effect| effect.id == effect_id)
        })
    else {
        return false;
    };
    let Some(source) = source else {
        let before = effect.sidechains.len();
        effect.sidechains.retain(|route| route.input_id != input_id);
        return before != effect.sidechains.len();
    };
    let Some(source_name) = source_name else {
        return false;
    };
    let taps = taps.unwrap_or_default();
    if taps.is_empty() {
        return false;
    }
    let previous = effect
        .sidechains
        .iter()
        .find(|route| route.input_id == input_id);
    let preferred = previous.map_or(SourceTap::AfterEffects, |route| route.tap);
    let tap = if taps.contains(&preferred) {
        preferred
    } else {
        taps[0]
    };
    let input_name = effect
        .external_inputs
        .iter()
        .find(|input| input.id == input_id)
        .unwrap()
        .name
        .clone();
    let assignment = SidechainAssignment {
        input_id,
        input_name,
        source,
        source_name,
        tap,
    };
    if previous == Some(&assignment) {
        return false;
    }
    effect.sidechains.retain(|route| route.input_id != input_id);
    effect.sidechains.push(assignment);
    true
}

#[cfg(test)]
pub fn edit_tap(
    tracks: &mut [ProjectTrack],
    master: &mut ProjectTrack,
    buses: &mut [ProjectTrack],
    receiver: TrackId,
    effect_id: EffectId,
    input_id: ExternalInputId,
    tap: SourceTap,
) -> bool {
    let channels = routing_channels(tracks, master, buses);
    edit_tap_with_model(
        tracks, master, buses, (receiver, effect_id, input_id), tap, &channels,
    )
}

pub fn edit_tap_with_model(
    tracks: &mut [ProjectTrack],
    master: &mut ProjectTrack,
    buses: &mut [ProjectTrack],
    target: (TrackId, EffectId, ExternalInputId),
    tap: SourceTap,
    channels: &[RoutingChannel],
) -> bool {
    let (receiver, effect_id, input_id) = target;
    let source = channels
        .iter()
        .find(|channel| channel.id == receiver)
        .and_then(|channel| channel.effects.iter().find(|effect| effect.id == effect_id))
        .and_then(|effect| {
            effect
                .assignments
                .iter()
                .find(|route| route.input_id == input_id)
        })
        .map(|route| route.source);
    let Some(source) = source else { return false };
    if !valid_taps(channels, receiver, effect_id, input_id, source).contains(&tap) {
        return false;
    }
    let Some(route) = tracks
        .iter_mut()
        .chain(buses.iter_mut())
        .chain(std::iter::once(master))
        .find(|track| track.id == receiver)
        .and_then(|track| {
            track
                .effects
                .iter_mut()
                .find(|effect| effect.id == effect_id)
        })
        .and_then(|effect| {
            effect
                .sidechains
                .iter_mut()
                .find(|route| route.input_id == input_id)
        })
    else {
        return false;
    };
    if route.tap == tap {
        return false;
    }
    route.tap = tap;
    true
}

#[cfg(test)]
#[path = "sidechain_tests.rs"]
mod tests;
