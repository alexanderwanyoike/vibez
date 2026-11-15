//! Which tracks a MIDI track can take its played notes from, and the edit that sets it.

use vibez_core::id::TrackId;
use vibez_core::midi_from::MidiFrom;

use vibez_core::routing::{RoutingError, RoutingGraph};

use crate::domains::sidechain::routing_channels;
use crate::state::ProjectTracksState;

/// Tracks whose plugin instrument could feed `receiver` without a MIDI From
/// loop, in track order. Built-in instruments play no notes out.
pub fn sources(tracks: &ProjectTracksState, receiver: TrackId) -> Vec<TrackId> {
    if !tracks
        .tracks
        .iter()
        .any(|track| track.id == receiver && track.kind.is_midi())
    {
        return Vec::new();
    }
    tracks
        .tracks
        .iter()
        .filter(|track| {
            track.id != receiver && track.kind.is_midi() && track.plugin_instrument_ref.is_some()
        })
        .filter(|track| !forms_loop(tracks, receiver, track.id))
        .map(|track| track.id)
        .collect()
}

/// The MIDI From `receiver` would have after choosing `source`, or `None`
/// when that is no change or `source` is not one of its [`sources`].
pub fn change(
    tracks: &ProjectTracksState,
    receiver: TrackId,
    source: Option<TrackId>,
) -> Option<Option<MidiFrom>> {
    let current = tracks
        .tracks
        .iter()
        .find(|track| track.id == receiver)?
        .midi_from;
    let next = match source {
        Some(source) if sources(tracks, receiver).contains(&source) => {
            Some(MidiFrom::new(receiver, source)?)
        }
        Some(_) => return None,
        None => None,
    };
    (next != current).then_some(next)
}

/// Applies [`change`]; false when there was nothing to apply.
pub fn set(tracks: &mut ProjectTracksState, receiver: TrackId, source: Option<TrackId>) -> bool {
    let Some(next) = change(tracks, receiver, source) else {
        return false;
    };
    if let Some(track) = tracks.tracks.iter_mut().find(|track| track.id == receiver) {
        track.midi_from = next;
    }
    true
}

fn forms_loop(tracks: &ProjectTracksState, receiver: TrackId, source: TrackId) -> bool {
    let mut channels = routing_channels(&tracks.tracks, &tracks.master, &tracks.buses);
    if let Some(channel) = channels.iter_mut().find(|channel| channel.id == receiver) {
        channel.midi_from = Some(source);
    }
    matches!(
        RoutingGraph::prepare(&channels),
        Err(RoutingError::FeedbackLoop)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ProjectTrack;
    use vibez_core::midi::TrackKind;

    fn plugin_track(name: &str) -> ProjectTrack {
        let mut track =
            ProjectTrack::new_instrument(TrackId::new(), name.into(), TrackKind::Midi, 0);
        track.has_instrument = true;
        track.plugin_instrument_ref = Some(vibez_core::effect::PluginDeviceInfo {
            format: "clap".into(),
            uid: format!("{name}.id"),
            path: format!("/plugins/{name}.clap").into(),
            name: name.into(),
            state_b64: None,
        });
        track
    }

    fn project(tracks: Vec<ProjectTrack>) -> ProjectTracksState {
        ProjectTracksState {
            tracks,
            ..Default::default()
        }
    }

    #[test]
    fn other_tracks_playing_a_plugin_instrument_are_sources() {
        let mut synth =
            ProjectTrack::new_instrument(TrackId::new(), "Synth".into(), TrackKind::Midi, 0);
        synth.has_instrument = true;
        let audio = ProjectTrack::new(TrackId::new(), "Vocal".into(), 0);
        let (rebeater, arp, lead) = (
            plugin_track("Rebeater"),
            plugin_track("Arp"),
            plugin_track("Lead"),
        );
        let tracks = project(vec![
            rebeater.clone(),
            synth,
            audio,
            arp.clone(),
            lead.clone(),
        ]);

        assert_eq!(sources(&tracks, lead.id), [rebeater.id, arp.id]);
    }

    #[test]
    fn a_track_that_takes_from_the_receiver_is_not_a_source() {
        let (first, second) = (plugin_track("First"), plugin_track("Second"));
        let mut tracks = project(vec![first.clone(), second.clone()]);
        assert!(set(&mut tracks, second.id, Some(first.id)));

        assert!(sources(&tracks, first.id).is_empty());
    }

    #[test]
    fn setting_routes_the_receiver_and_refuses_no_change_or_an_unlisted_source() {
        let (source, receiver) = (plugin_track("Rebeater"), plugin_track("Lead"));
        let mut tracks = project(vec![source.clone(), receiver.clone()]);

        assert!(set(&mut tracks, receiver.id, Some(source.id)));
        assert_eq!(
            tracks.tracks[1].midi_from.map(MidiFrom::source),
            Some(source.id)
        );
        assert!(!set(&mut tracks, receiver.id, Some(source.id)));
        assert!(!set(&mut tracks, receiver.id, Some(receiver.id)));
        assert!(set(&mut tracks, receiver.id, None));
        assert_eq!(tracks.tracks[1].midi_from, None);
    }

    #[test]
    fn the_routing_model_carries_a_tracks_midi_from() {
        let (source, receiver) = (plugin_track("Rebeater"), plugin_track("Lead"));
        let mut tracks = project(vec![source.clone(), receiver.clone()]);
        set(&mut tracks, receiver.id, Some(source.id));

        let channels = crate::domains::sidechain::routing_channels(
            &tracks.tracks,
            &tracks.master,
            &tracks.buses,
        );

        assert_eq!(channels[1].midi_from, Some(source.id));
        assert_eq!(channels[0].midi_from, None);
    }
}
