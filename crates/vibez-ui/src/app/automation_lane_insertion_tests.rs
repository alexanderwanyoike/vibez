//! Real App lane edits, restore and teardown service storage independently of canonical target presence.

use super::*;
use crate::domains::automation::AutomationMsg;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_engine::clip_insertion::ClipInsertionTarget;

struct LanePending {
    app: App,
    engine: AudioEngine,
    track: TrackId,
    resident: AutomationLane,
    points: *const AutomationPoint,
    seed: AutomationLane,
    edited: AutomationLane,
    operation: ClipInsertionId,
    audio: Weak<DecodedAudio>,
}

fn edit(app: &mut App, message: AutomationMsg) {
    let _ = app.update_and_refresh_clips(Message::Automation(message));
}

fn pending_lane() -> LanePending {
    let mut app = super::super::test_support::app();
    let (mut engine, commands, events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.event_rx = Some(events);
    let track = TrackId::new();
    let mut model = ProjectTrack::new(track, "App lane target".into(), 0);
    model.pan = 0.0;
    Arc::make_mut(&mut app.state.project_tracks).tracks.push(model.clone());
    let audio_clip = clip("Resident lane audio", 0.25);
    let audio = Arc::downgrade(&audio_clip.audio);
    let mut resident = AutomationLane::new(AutomationTarget::TrackPan);
    resident.points.push(AutomationPoint { beat: 0.0, value: 0.0, curve: 0.25 });
    let content = Arc::make_mut(&mut app.state.arrangement.timeline).ensure(track);
    content.clips.push(audio_clip);
    content.automation.push(resident.clone());
    app.replay_track_to_engine(&model);
    app.sync_sidechain_routing();
    app.send_command(EngineCommand::Play);
    assert_signal(&render(&mut engine, true), 0.25);
    app.poll_engine_events();
    let current = &engine.tracks()[0].playback_source.automation;
    assert_eq!((current.len(), current.capacity()), (1, 1));
    assert_eq!(current[0], resident);
    let points = current[0].points.as_ptr();
    edit(&mut app, AutomationMsg::AddLane { track_id: track, target: AutomationTarget::TrackGain });
    let seed = app.state.arrange_content(track).unwrap().automation[1].clone();
    assert_eq!(seed.points, [AutomationPoint { beat: 0.0, value: model.gain / 2.0, curve: 0.0 }]);
    render(&mut engine, false);
    let EngineEvent::ClipInsertionStorageRequested(request) = app.event_rx.as_ref().unwrap().peek().unwrap() else {
        panic!("real AddLane must exhaust the nonzero resident outer capacity");
    };
    let operation = request.id.clone();
    assert_eq!((request.track_id, request.target), (track, ClipInsertionTarget::AutomationLane(seed.id)));
    for message in [
        AutomationMsg::AddPoint { track_id: track, lane_id: seed.id, beat: 0.0, value: 0.25 },
        AutomationMsg::AddPoint { track_id: track, lane_id: seed.id, beat: 2.0, value: 0.25 },
        AutomationMsg::SetCurve { track_id: track, lane_id: seed.id, index: 0, curve: 0.5 },
    ] { edit(&mut app, message); }
    let edited = app.state.arrange_content(track).unwrap().automation[1].clone();
    assert_eq!(edited.points, [
        AutomationPoint { beat: 0.0, value: 0.25, curve: 0.5 },
        AutomationPoint { beat: 2.0, value: 0.25, curve: 0.0 },
    ]);
    LanePending { app, engine, track, resident, points, seed, edited, operation, audio }
}

#[test]
fn actual_lane_edits_preserve_cloned_fifo_intent_before_stale_undo_and_clear_servicing() {
    for change in [Change::Current, Change::CanonicalOnlyStale, Change::UndoReplay, Change::Clear] {
        let mut fixture = pending_lane();
        match change {
            Change::CanonicalOnlyStale => fixture.app.state.arrangement.timeline = Arc::new(Default::default()),
            Change::UndoReplay => {
                let _ = fixture.app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));
            }
            Change::Clear => fixture.app.clear_project_runtime(),
            Change::Current => {}
            _ => unreachable!(),
        }
        fixture.app.send_command(EngineCommand::SetBpm(137.0));
        assert_signal(&render(&mut fixture.engine, true), 0.25);
        assert_eq!(fixture.engine.transport().position(), 16);
        assert!(fixture.engine.transport().is_playing());
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        let current = &fixture.engine.tracks()[0].playback_source.automation;
        assert_eq!(current, &[fixture.resident.clone()]);
        assert_eq!(current[0].points.as_ptr(), fixture.points);
        let canonical_tracks = Arc::clone(&fixture.app.state.project_tracks);
        let canonical_timeline = Arc::clone(&fixture.app.state.arrangement.timeline);
        fixture.app.poll_engine_events();
        assert!(Arc::ptr_eq(&canonical_tracks, &fixture.app.state.project_tracks));
        assert!(Arc::ptr_eq(&canonical_timeline, &fixture.app.state.arrangement.timeline));
        render(&mut fixture.engine, false);
        let EngineEvent::ClipInsertionCompleted(completed) = fixture.app.event_rx.as_mut().unwrap().pop().unwrap() else {
            panic!("the original lane edit must complete before any later point or teardown command");
        };
        assert_eq!(completed.id(), fixture.operation);
        assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
        assert_eq!((completed.track_id, completed.target), (fixture.track, ClipInsertionTarget::AutomationLane(fixture.seed.id)));
        drop(completed);
        let mut routing_owners = Vec::new();
        let original_seed = loop {
            match fixture.app.event_rx.as_mut().unwrap().pop().unwrap() {
                EngineEvent::RoutingRetired(plan) => routing_owners.push(plan),
                EngineEvent::RetiredAutomationLane(value) => break value,
                event => panic!("the first point edit must retire its original seeded lane: {event:?}"),
            }
        };
        assert!(routing_owners.len() <= 1);
        assert_eq!(original_seed, fixture.seed);
        drop((original_seed, routing_owners));
        // Undo replay may exhaust fresh lane storage too; every request must be
        // served off callback, including one whose canonical target disappeared.
        let exchanges = 2 + fixture.app.state.arrange_content(fixture.track).map_or(0, |content| content.automation.len());
        for _ in 0..=exchanges {
            fixture.app.poll_engine_events();
            render(&mut fixture.engine, false);
        }
        fixture.app.poll_engine_events();
        assert_eq!(fixture.app.cmd_tx.pending_len(), 0);
        assert_eq!(fixture.engine.transport().bpm(), 137.0);
        assert!(!fixture.app.state.status_text.to_ascii_lowercase().contains("abandon"));
        let signal = match change {
            Change::Clear => {
                assert!(fixture.engine.tracks().is_empty());
                assert!(fixture.app.state.find_track(fixture.track).is_none());
                assert!(fixture.app.state.arrange_content(fixture.track).is_none());
                assert!(!fixture.engine.transport().is_playing());
                0.0
            }
            Change::Current | Change::CanonicalOnlyStale | Change::UndoReplay => {
                let mut expected = fixture.edited.clone();
                if matches!(change, Change::UndoReplay) { expected.points[0].curve = 0.0; }
                assert_eq!(fixture.engine.tracks()[0].playback_source.automation, [fixture.resident.clone(), expected.clone()]);
                if matches!(change, Change::CanonicalOnlyStale) {
                    assert!(fixture.app.state.arrange_content(fixture.track).is_none());
                } else {
                    assert_eq!(fixture.app.state.arrange_content(fixture.track).unwrap().automation, [fixture.resident.clone(), expected]);
                }
                0.125
            }
            _ => unreachable!(),
        };
        for _ in 0..8 {
            render(&mut fixture.engine, true);
            fixture.app.poll_engine_events();
        }
        assert_signal(&render(&mut fixture.engine, true), signal);
        drop((canonical_tracks, canonical_timeline));
        drop(fixture.engine);
        drop(fixture.app);
        assert_eq!(fixture.audio.strong_count(), 0, "{change:?}");
    }
}

#[test]
fn loaded_lane_payloads_reach_engine_after_real_app_request_servicing() {
    let mut app = super::super::test_support::app();
    let (mut engine, commands, events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.event_rx = Some(events);
    let mut track = vibez_core::track::TrackInfo::new("Loaded lanes");
    for (target, value) in [(AutomationTarget::TrackGain, 0.25), (AutomationTarget::TrackPan, 0.0), (AutomationTarget::TrackMute, 0.0)] {
        let mut lane = AutomationLane::new(target);
        lane.points = vec![AutomationPoint { beat: 0.0, value, curve: -0.5 }, AutomationPoint { beat: 4.0, value, curve: 0.75 }];
        track.automation.push(lane);
    }
    let expected = track.automation.clone();
    let track_id = track.id;
    let project = vibez_project::Project { tracks: vec![track], ..Default::default() };
    app.rebuild_from_loaded_project(crate::message::ProjectLoadResult {
        path: "lane-ingress-test.vzp".into(), project, clips: vec![], unresolved_clips: vec![],
        sampler_samples: vec![], drum_rack_pad_samples: vec![], warnings: vec![],
    });
    app.send_command(EngineCommand::SetBpm(137.0));
    render(&mut engine, false);
    assert_eq!(engine.tracks()[0].id, track_id);
    let current = &engine.tracks()[0].playback_source.automation;
    assert_eq!((current.len(), current.capacity()), (1, 1));
    assert_eq!(current, &expected[..1]);
    assert_eq!(engine.transport().bpm(), 120.0);
    for _ in 0..=expected.len() {
        app.poll_engine_events();
        render(&mut engine, false);
    }
    app.poll_engine_events();
    assert_eq!(engine.tracks()[0].playback_source.automation, expected);
    assert_eq!(app.state.arrange_content(track_id).unwrap().automation, expected);
    assert_eq!(engine.transport().bpm(), 137.0);
    assert_eq!(app.cmd_tx.pending_len(), 0);
}

#[test]
fn actual_lane_edit_idle_stream_shutdown_releases_request_reply_and_complete_owners() {
    for stage in [ShutdownStage::RequestUnpolled, ShutdownStage::ReplyReady, ShutdownStage::CompletionUnpolled] {
        let mut fixture = pending_lane();
        if !matches!(stage, ShutdownStage::RequestUnpolled) {
            fixture.app.poll_engine_events();
            assert_eq!(fixture.engine.tracks()[0].playback_source.automation, [fixture.resident.clone()]);
        }
        if matches!(stage, ShutdownStage::CompletionUnpolled) {
            render(&mut fixture.engine, false);
            assert_eq!(fixture.engine.tracks()[0].playback_source.automation, [fixture.resident.clone(), fixture.edited.clone()]);
            assert!(matches!(fixture.app.event_rx.as_ref().unwrap().peek().unwrap(),
                EngineEvent::ClipInsertionCompleted(completed) if completed.id() == fixture.operation));
        }
        assert!(fixture.audio.strong_count() > 0);
        fixture.app._stream = Some(vibez_audio_io::audio_stream::AudioOutputStream::idle(fixture.engine));
        drop(fixture.app);
        assert_eq!(fixture.audio.strong_count(), 0, "{stage:?}");
    }
}
