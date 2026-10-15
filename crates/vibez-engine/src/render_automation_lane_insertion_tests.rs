//! Offline lane collection preserves selected-track filtering before native outer growth.

use super::*;
use crate::playback_source::automation_lane_construction_tests::{lane, verify_lane_source_growth};
use vibez_core::automation::{AutomationLane, AutomationTarget};

#[test]
fn actual_offline_lane_collection_crosses_full_capacity_without_losing_original_point_values() {
    let mut track = TrackInfo::new("Offline lane boundary");
    track.automation = (0..4).map(|index| lane(index as f32 / 8.0)).collect();
    track.automation.push(AutomationLane::new(AutomationTarget::TrackMute));
    let expected = track.automation[..4].to_vec();
    let request = BounceRequest {
        mode: BounceMode::Track(track.id), tracks: vec![track], master: None, buses: vec![],
        audio_clips: vec![], note_clips: vec![], clip_audio: HashMap::new(), sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(), range_samples: (0, 512), bpm: 120.0, sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let mut warnings = Vec::new();
    let channel = super::super::channel(&request, &request.tracks[0], SourceTap::AfterEffects, None, &mut warnings);
    assert!(warnings.is_empty(), "{warnings:?}");
    let source = channel.playback_source;
    assert_eq!(source.automation, expected);
    assert_eq!((source.automation.len(), source.automation.capacity()), (4, 4));
    for (original, prepared) in request.tracks[0].automation.iter().zip(&source.automation) {
        assert_eq!(original, prepared);
        assert_ne!(original.points.as_ptr(), prepared.points.as_ptr());
    }
    verify_lane_source_growth(*source);
    assert_eq!(request.tracks[0].automation.len(), 5);
    assert_eq!(request.tracks[0].automation[..4], expected);
    assert_eq!(request.tracks[0].automation[4].target, AutomationTarget::TrackMute);
}
