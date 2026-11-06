use super::*;

#[test]
fn a_content_swap_touches_only_the_clips_and_lanes_that_differ() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, _unchanged) = add_audio_clip(&mut a, 0, 0, 1_000);
    let (_, moved) = add_audio_clip(&mut a, 0, 2_000, 1_000);
    let mut current = a.arrangement.timeline.get(track_id).unwrap().clone();
    let dropped_lane = AutomationLane::new(AutomationTarget::TrackPan);
    current.automation.push(dropped_lane.clone());
    let mut target = current.clone();
    target.clips[1].position = 4_000;
    target.automation = vec![AutomationLane::new(AutomationTarget::TrackGain)];

    let mut engine = RecordingEngine::default();
    swap_changed_content(&mut engine, track_id, &current, &target);

    assert!(
        matches!(
            engine.0.as_slice(),
            [
                EngineCommand::RemoveClip(_, removed),
                EngineCommand::RemoveAutomationLane { lane_id, .. },
                EngineCommand::AddClip { clip_id, position: 4_000, .. },
                EngineCommand::SetAutomationLane { .. },
            ] if *removed == moved && *lane_id == dropped_lane.id && *clip_id == moved
        ),
        "only the moved clip and the changed lanes reach the engine"
    );
}

#[test]
fn identical_content_sends_nothing() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, _) = add_audio_clip(&mut a, 0, 0, 1_000);
    let content = a.arrangement.timeline.get(track_id).unwrap().clone();

    let mut engine = RecordingEngine::default();
    swap_changed_content(&mut engine, track_id, &content, &content.clone());

    assert!(engine.0.is_empty());
}
