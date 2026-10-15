//! A reachable queued Section boundary cannot redirect pending Arrange lane growth to a colliding live lane ID.

use super::*;
use crate::clip_insertion::{ClipInsertionOutcome, ClipInsertionTarget, PreparedAutomationLaneInsertion};
use crate::events::EngineEvent;
use crate::playback_source::automation_lane_construction_tests::lane;
use vibez_core::automation::AutomationTarget;
use vibez_core::perform::SectionLaunchQuantization;

#[test]
fn queued_section_with_colliding_lane_id_keeps_arrange_growth_and_perform_owners_distinct() {
    let track_id = TrackId::new();
    let resident = lane(0.0);
    let resident_points = resident.points.as_ptr();
    let expected_resident = resident.clone();
    let mut arrange = home_source(1);
    arrange.automation = vec![resident].into_boxed_slice().into_vec();
    let mut track = Box::new(EngineTrack::with_playback_source(track_id, arrange));
    track.pan = 0.0;
    let calls = Arc::new(VoiceCalls::default());
    track.instrument = Some(Box::new(CountedInstrument {
        instrument: crate::test_support::DelayProbeInstrument::new(0, 0.25), calls: Arc::clone(&calls),
    }));
    let arrange = HomeSnapshot::observe(&track.playback_source);
    let arrange_outer = track.playback_source.automation.as_ptr();
    let inactive = HomeSnapshot::observe(&track.empty_launcher_source);
    let launcher = HomeSnapshot::observe(&track.launcher_source);
    let initial_section = HomeSnapshot::observe(&track.section_playback_source);
    let old_section_id = SectionId::new();
    let next_section_id = SectionId::new();
    let old_lane = lane(0.0);
    let old_points = old_lane.points.as_ptr();
    let expected_old = old_lane.clone();
    let mut old_source = home_source(1);
    old_source.automation = vec![old_lane];
    let old_prepared = PreparedSectionPlaybackSource::new(old_section_id, 2.0, true, vec![(track_id, old_source)]);
    let old_home = HomeSnapshot::observe(&old_prepared.tracks()[0].source);
    let mut next_lane = lane(0.0);
    next_lane.id = expected_old.id;
    next_lane.points[0].curve = 0.25;
    let next_points = next_lane.points.as_ptr();
    let expected_next = next_lane.clone();
    let mut next_source = home_source(2);
    next_source.automation = vec![next_lane];
    let next_prepared = PreparedSectionPlaybackSource::new(next_section_id, 2.0, true, vec![(track_id, next_source)]);
    let next_home = HomeSnapshot::observe(&next_prepared.tracks()[0].source);
    for (index, left) in [arrange.source, inactive.source, launcher.source, initial_section.source, old_home.source, next_home.source].iter().enumerate() {
        for right in [arrange.source, inactive.source, launcher.source, initial_section.source, old_home.source, next_home.source].iter().skip(index + 1) {
            assert_ne!(left, right, "all source homes are simultaneously owned before any transition");
        }
    }
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::SetSampleRate(32)).unwrap();
    commands.push(EngineCommand::AddTrack(track)).unwrap();
    let channels = [track_id, TrackId::MASTER].map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: vec![], effects: vec![],
    });
    let mut routing = crate::routing::PreparedRouting::prepare(&channels, 8).unwrap();
    routing.configure_automation(&[(track_id, AutomationTarget::TrackPan), (track_id, AutomationTarget::TrackGain)]).unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    commands.push(EngineCommand::LaunchSection(Box::new(old_prepared))).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert_sounding(&output);
    let mut initial_owners = Vec::new();
    while let Ok(event) = events.pop() {
        if let EngineEvent::SectionTransitioned { retired, .. } = event {
            initial_section.assert_unchanged(&retired.tracks()[0].source);
            initial_owners.push(retired);
        }
    }
    assert_eq!(initial_owners.len(), 1);
    commands.push(EngineCommand::QueueSection {
        prepared: Box::new(next_prepared), quantization: SectionLaunchQuantization::EndOfSection,
    }).unwrap();
    let mut incoming = lane(0.25);
    incoming.id = expected_next.id;
    incoming.target = AutomationTarget::TrackGain;
    let incoming_points = incoming.points.as_ptr();
    let expected_incoming = incoming.clone();
    let insertion = PreparedAutomationLaneInsertion::new(incoming);
    let operation = insertion.id();
    commands.push(EngineCommand::SetAutomationLane { track_id, insertion }).unwrap();
    commands.push(EngineCommand::SetTrackGain(track_id, 0.5)).unwrap();
    measured_callback(&mut engine, &mut []);
    let mut request = None;
    let mut queued = 0;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionStorageRequested(value) => {
                assert_eq!(value.id, operation);
                assert_eq!((value.track_id, value.target), (track_id, ClipInsertionTarget::AutomationLane(expected_incoming.id)));
                assert!(request.replace(value).is_none());
            }
            EngineEvent::SectionQueued { section_id, effective_at_samples, retired } => {
                assert_eq!((section_id, effective_at_samples), (next_section_id, 32));
                assert!(retired.is_none());
                queued += 1;
            }
            EngineEvent::PlaybackPosition(0) | EngineEvent::PerformancePosition(8) => {}
            EngineEvent::SectionPlaybackPosition { section_id, position_samples } => assert_eq!((section_id, position_samples), (old_section_id, 8)),
            EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0)),
            event => panic!("unexpected source-boundary admission event: {event:?}"),
        }
    }
    assert_eq!(queued, 1);
    let request = request.expect("restored Arrange capacity must require growth despite the live Section's matching ID");
    let mut retired_sections = Vec::new();
    for step in 0..4 {
        let output_position = engine.output_position_samples();
        measured_callback(&mut engine, &mut output);
        assert_sounding(&output);
        assert_eq!(engine.output_position_samples(), output_position + 8);
        assert_eq!(engine.transport().position(), 0);
        assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY - 1);
        let channel = &engine.tracks()[0];
        arrange.assert_unchanged(&channel.playback_source);
        assert_eq!(channel.playback_source.automation, [expected_resident.clone()]);
        assert_eq!(channel.playback_source.automation.as_ptr(), arrange_outer);
        assert_eq!(channel.playback_source.automation[0].points.as_ptr(), resident_points);
        assert_eq!(channel.gain, 1.0);
        let mut heard_perform = None;
        let mut heard_section = None;
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::SectionTransitioned { section_id, effective_at_samples, retired } => {
                    assert_eq!((section_id, effective_at_samples), (next_section_id, 32));
                    old_home.assert_unchanged(&retired.tracks()[0].source);
                    assert_eq!(retired.tracks()[0].source.automation, [expected_old.clone()]);
                    assert_eq!(retired.tracks()[0].source.automation[0].points.as_ptr(), old_points);
                    retired_sections.push(retired);
                }
                EngineEvent::PlaybackPosition(0) => {}
                EngineEvent::PerformancePosition(value) => assert!(heard_perform.replace(value).is_none()),
                EngineEvent::SectionPlaybackPosition { section_id, position_samples } => assert!(heard_section.replace((section_id, position_samples)).is_none()),
                EngineEvent::TrackMeter { track_id: id, peak_l, peak_r } => {
                    assert!([track_id, TrackId::MASTER].contains(&id));
                    assert_eq!((peak_l, peak_r), (0.25, 0.0));
                }
                EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.25, 0.0, 0.25, 0.0)),
                event => panic!("pending Arrange lane must not redirect or duplicate owners: {event:?}"),
            }
        }
        assert_eq!(heard_perform, Some(16 + step * 8));
        assert_eq!(heard_section, Some((if step < 3 { old_section_id } else { next_section_id }, (16 + step * 8) % 32)));
    }
    assert_eq!(retired_sections.len(), 1);
    let before = calls.observed();
    let next = &engine.tracks()[0].section_playback_source;
    next_home.assert_unchanged(next);
    assert_eq!(next.automation, [expected_next.clone()]);
    assert_eq!(next.automation[0].points.as_ptr(), next_points);
    request.fulfill();
    measured_callback(&mut engine, &mut []);
    let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else { panic!("Arrange lane completion must precede the queued gain edit") };
    assert_eq!(completed.id(), operation);
    assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
    assert_eq!((completed.track_id, completed.target), (track_id, ClipInsertionTarget::AutomationLane(expected_incoming.id)));
    assert_eq!(crate::retirement::tests::allocations(|| drop(completed)), (0, 4));
    let channel = &engine.tracks()[0];
    arrange.assert_unchanged(&channel.playback_source);
    assert_eq!(channel.playback_source.automation, [expected_resident, expected_incoming]);
    assert_eq!(channel.playback_source.automation[0].points.as_ptr(), resident_points);
    assert_eq!(channel.playback_source.automation[1].points.as_ptr(), incoming_points);
    next_home.assert_unchanged(&channel.section_playback_source);
    assert_eq!(channel.section_playback_source.automation, [expected_next]);
    assert_eq!(channel.section_playback_source.automation[0].points.as_ptr(), next_points);
    inactive.assert_unchanged(&channel.empty_launcher_source);
    launcher.assert_unchanged(&channel.launcher_source);
    assert_eq!(channel.gain, 0.5);
    assert_eq!(calls.observed(), before);
    assert_eq!(engine.transport().position(), 0);
    assert_eq!(engine.output_position_samples(), 40);
    assert_eq!(commands.slots(), vibez_core::constants::RING_BUFFER_CAPACITY);
    measured_callback(&mut engine, &mut output);
    for &[left, right] in output.as_chunks::<2>().0 { assert_eq!((left, right), (0.125, 0.0)); }
    assert_eq!(calls.observed(), before);
    assert_eq!(engine.output_position_samples(), 48);
}
