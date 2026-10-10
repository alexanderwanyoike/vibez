//! Timing fingerprints track device reports and live eligibility without changing stored coordinates.

use vibez_core::id::TrackId;
use vibez_core::routing::{NodeStage, RoutingNode};
use vibez_core::track::InputMonitoring;

use crate::state::{ProjectTrack, ProjectTracksState};

#[derive(Debug, Clone, PartialEq)]
pub struct TimingSignature {
    pub reports: Vec<(RoutingNode, u32)>,
    pub reduced_tracks: Vec<TrackId>,
    pub sample_rate: u32,
    pub controls: Vec<(TrackId, vibez_core::automation::AutomationTarget)>,
}

pub fn signature(
    project: &ProjectTracksState,
    selected_instrument: Option<TrackId>,
    armed_audio: Option<TrackId>,
    monitored_audio: Option<TrackId>,
    sample_rate: u32,
) -> TimingSignature {
    let mut reports = Vec::new();
    let mut reduced_tracks = Vec::new();
    for track in project
        .tracks
        .iter()
        .chain(&project.buses)
        .chain(std::iter::once(&project.master))
    {
        reports.push((
            RoutingNode {
                channel: track.id,
                stage: NodeStage::Source,
            },
            track.instrument_latency_samples.unwrap_or(0),
        ));
        for effect in &track.effects {
            if let Some(latency) = effect.latency_samples {
                reports.push((
                    RoutingNode {
                        channel: track.id,
                        stage: NodeStage::Effect(effect.id),
                    },
                    latency,
                ));
            }
        }
        if project.reduced_latency_monitoring
            && !track.id.is_master()
            && !project.buses.iter().any(|bus| bus.id == track.id)
            && eligible(track, selected_instrument, armed_audio, monitored_audio)
        {
            reduced_tracks.push(track.id);
        }
    }
    TimingSignature {
        reports,
        reduced_tracks,
        sample_rate,
        controls: Vec::new(),
    }
}

pub fn signature_for_state(state: &crate::state::AppState) -> TimingSignature {
    signature_for_live_routes(state, None)
}

pub fn signature_for_live_routes(
    state: &crate::state::AppState,
    midi_target: Option<TrackId>,
) -> TimingSignature {
    let mut timing = signature(
        &state.project_tracks,
        state.perform.instrument_target(),
        state.audio_recording.armed_track,
        state.audio_recording.monitor_track,
        state.transport.sample_rate,
    );
    if state.project_tracks.reduced_latency_monitoring {
        if let Some(track) = midi_target.and_then(|id| state.find_track(id)) {
            if track.is_playable_midi_target() && !timing.reduced_tracks.contains(&track.id) {
                timing.reduced_tracks.push(track.id);
            }
        }
    }
    let timelines = std::iter::once(state.arrangement.timeline.as_ref())
        .chain(
            state
                .perform
                .sections
                .sections
                .iter()
                .map(|section| section.timeline.as_ref()),
        )
        .chain(
            state
                .perform
                .clips
                .clips
                .iter()
                .map(|clip| clip.timeline.as_ref()),
        );
    for timeline in timelines {
        for (&track, content) in &timeline.by_track {
            for lane in &content.automation {
                timing.controls.push((track, lane.target));
            }
        }
    }
    if let Some(previous) = &state.devices.last_timing {
        if previous.reports == timing.reports
            && previous.reduced_tracks == timing.reduced_tracks
            && previous.sample_rate == timing.sample_rate
        {
            timing.controls.extend(previous.controls.iter().copied());
        }
    }
    timing
        .controls
        .sort_by_cached_key(|(track, target)| (track.raw(), format!("{target:?}")));
    timing.controls.dedup();
    timing
}

pub fn eligible(
    track: &ProjectTrack,
    selected_instrument: Option<TrackId>,
    armed_audio: Option<TrackId>,
    monitored_audio: Option<TrackId>,
) -> bool {
    if track.is_playable_midi_target() {
        return selected_instrument == Some(track.id);
    }
    track.audio_input_route.is_hardware()
        && match track.input_monitoring {
            InputMonitoring::Off => false,
            InputMonitoring::Auto => armed_audio == Some(track.id),
            InputMonitoring::On => {
                monitored_audio == Some(track.id) || armed_audio == Some(track.id)
            }
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_full_timing_and_whole_monitored_track_eligibility() {
        let id = TrackId::new();
        let mut project = ProjectTracksState::default();
        let mut track = ProjectTrack::new(id, "Input".into(), 0);
        track.input_monitoring = InputMonitoring::Auto;
        project.tracks.push(track);
        assert!(signature(&project, None, Some(id), None, 48000)
            .reduced_tracks
            .is_empty());
        project.reduced_latency_monitoring = true;
        assert_eq!(
            signature(&project, None, Some(id), None, 48000).reduced_tracks,
            vec![id]
        );
        assert!(signature(&project, None, None, None, 48000)
            .reduced_tracks
            .is_empty());
    }
}

#[cfg(test)]
#[path = "compensation_project_tests.rs"]
mod project_tests;
