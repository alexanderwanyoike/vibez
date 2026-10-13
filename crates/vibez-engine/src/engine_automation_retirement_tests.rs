//! Actual live lane edits transfer prepared point owners back to the UI.

use super::*;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

#[test]
fn replacing_and_removing_lanes_retains_point_storage_without_callback_destruction() {
    for replace in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let id = TrackId::new();
        let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
        lane.insert_point(AutomationPoint {
            beat: 0.0,
            value: 0.5,
            curve: 0.0,
        });
        let lane_id = lane.id;
        let mut source = crate::playback_source::PreparedPlaybackSource::default();
        source.automation.push(lane.clone());
        engine
            .tracks
            .push(EngineTrack::with_playback_source(id, source));
        // The actual resident owner, not the incoming replacement, is returned.
        let points = engine.tracks[0].playback_source.automation[0]
            .points
            .as_ptr();
        engine.process(&mut [0.0; 16], 2);
        while events.pop().is_ok() {}
        while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        commands
            .push(if replace {
                EngineCommand::SetAutomationLane { track_id: id, lane }
            } else {
                EngineCommand::RemoveAutomationLane {
                    track_id: id,
                    lane_id,
                }
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 16], 2)),
            (0, 0)
        );
        assert_eq!(engine.pending_retirements.len(), 1);
        while events.pop().is_ok() {}
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.flush_retirements()),
            (0, 0)
        );
        let returned = events.pop().unwrap();
        let EngineEvent::RetiredAutomationLane(owner) = returned else {
            panic!("lane owner not returned")
        };
        assert_eq!(owner.id, lane_id);
        assert_eq!(owner.points.as_ptr(), points);
        drop(owner);
    }
}
