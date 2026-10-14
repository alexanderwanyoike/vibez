//! Actual App polling fulfills clip storage across canonical edits and disposes complete owners.

use super::*;
use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::state::{ProjectTrack, UiClip};
use std::sync::Weak;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::warp_marker::{WarpMarker, WarpMarkers};
use vibez_engine::clip_insertion::{ClipInsertionId, ClipInsertionOutcome, PreparedClipInsertion};
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};
use vibez_engine::events::EngineEvent;

fn clip(name: &str, value: f32) -> UiClip {
    UiClip {
        id: ClipId::new(), name: name.into(),
        audio: Arc::new(DecodedAudio { channels: vec![vec![value; 4096]], sample_rate: 44_100 }),
        source: None, position: 0, source_offset: 0, start_marker: 0,
        duration: 4096, loop_enabled: false, loop_start: 0, loop_end: 0,
        gain_db: Default::default(), fades: Default::default(),
        playback_direction: Default::default(), transient_markers: Default::default(),
        warp_markers: Default::default(), transpose: Default::default(),
        original_bpm: Some(120.0), warped: false, warped_to_bpm: None, original_audio: None,
    }
}

fn command(track: TrackId, clip: &UiClip, insertion: Box<PreparedClipInsertion>) -> EngineCommand {
    EngineCommand::AddClip {
        insertion, track_id: track, clip_id: clip.id, audio: Arc::clone(&clip.audio),
        position: clip.position, source_offset: clip.source_offset,
        start_marker: clip.start_marker, duration: clip.duration,
        loop_enabled: clip.loop_enabled, loop_start: clip.loop_start,
        loop_end: clip.loop_end, linear_gain: clip.gain_db.linear(), fades: clip.fades,
        playback_direction: clip.playback_direction, warp_markers: clip.warp_markers.clone(),
    }
}

fn render(engine: &mut AudioEngine, sounding: bool) -> [f32; 16] {
    let mut output = [0.0; 16];
    let length = if sounding { output.len() } else { 0 };
    engine.process_block(AudioProcessBlock::new(&mut output[..length], 2));
    output
}

fn assert_signal(output: &[f32], value: f32) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - value).abs() < 1e-7, "expected {value}, actual {left}");
        assert_eq!(right, 0.0);
    }
}

struct Pending {
    app: App,
    engine: AudioEngine,
    track: TrackId,
    resident: ClipId,
    resident_audio: *const DecodedAudio,
    incoming: ClipId,
    audio: Weak<DecodedAudio>,
    operation: ClipInsertionId,
    markers: WarpMarkers,
    engine_markers: *const WarpMarker,
}

fn pending() -> Pending {
    let mut app = super::test_support::app();
    app.state.confirm_project_track_deletion = false;
    let (mut engine, commands, events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.event_rx = Some(events);
    let track = TrackId::new();
    let mut model = ProjectTrack::new(track, "Audio".into(), 0);
    model.pan = 0.0;
    Arc::make_mut(&mut app.state.project_tracks).tracks.push(model.clone());
    let resident = clip("Resident", 0.25);
    let resident_id = resident.id;
    let resident_audio = Arc::as_ptr(&resident.audio);
    Arc::make_mut(&mut app.state.arrangement.timeline).ensure(track).clips.push(resident);
    app.replay_track_to_engine(&model);
    app.sync_sidechain_routing();
    app.send_command(EngineCommand::Play);
    assert_signal(&render(&mut engine, true), 0.25);
    app.poll_engine_events();
    assert_eq!(engine.transport().position(), 8);
    assert_eq!(engine.tracks()[0].playback_source.clips.len(), engine.tracks()[0].playback_source.clips.capacity());
    app.push_undo_snapshot(None);
    let mut incoming = clip("Incoming", 0.0625);
    incoming.loop_enabled = true;
    incoming.loop_end = 128;
    assert!(incoming.warp_markers.add(32, 64, 0, 128, 128));
    let incoming_id = incoming.id;
    let audio = Arc::downgrade(&incoming.audio);
    let markers = incoming.warp_markers.clone();
    let insertion = PreparedClipInsertion::new();
    let operation = insertion.id();
    let add = command(track, &incoming, insertion);
    let EngineCommand::AddClip { warp_markers, .. } = &add else { unreachable!(); };
    let engine_markers = warp_markers.as_slice().as_ptr();
    Arc::make_mut(&mut app.state.arrangement.timeline).ensure(track).clips.push(incoming);
    app.send_command(add);
    render(&mut engine, false);
    assert!(matches!(app.event_rx.as_ref().unwrap().peek().unwrap(),
        EngineEvent::ClipInsertionStorageRequested(request) if request.id == operation));
    assert_eq!(engine.tracks()[0].playback_source.clips.len(), 1);
    Pending { app, engine, track, resident: resident_id, resident_audio,
        incoming: incoming_id, audio, operation, markers, engine_markers }
}

#[derive(Debug, Clone, Copy)]
enum Change {
    Current,
    CanonicalOnlyStale,
    DeleteTrack,
    UndoReplay,
    Stop,
    Clear,
}

#[test]
fn clip_request_polling_serves_current_stale_delete_undo_stop_and_clear_state() {
    for change in [Change::Current, Change::CanonicalOnlyStale, Change::DeleteTrack,
        Change::UndoReplay, Change::Stop, Change::Clear] {
        let mut fixture = pending();
        match change {
            Change::Current => {}
            Change::CanonicalOnlyStale => {
                // This deliberate canonical-only fixture mutation isolates late
                // target staleness without inventing an engine cancellation.
                fixture.app.state.arrangement.timeline = Arc::new(Default::default());
            }
            Change::DeleteTrack => { let _ = fixture.app.update_and_refresh_clips(
                Message::Arrangement(ArrangementMsg::RemoveTrack(fixture.track)));
            }
            Change::UndoReplay => { let _ = fixture.app.update_and_refresh_clips(
                Message::Project(ProjectMsg::Undo));
            }
            Change::Stop => { let _ = fixture.app.update_and_refresh_clips(
                Message::Transport(TransportMsg::Stop));
            }
            Change::Clear => fixture.app.clear_project_runtime(),
        }
        fixture.app.send_command(EngineCommand::SetBpm(137.0));
        assert_signal(&render(&mut fixture.engine, true), 0.25);
        assert_eq!(fixture.engine.transport().position(), 16, "{change:?}");
        assert!(fixture.engine.transport().is_playing(), "{change:?}");
        assert_eq!(fixture.engine.transport().bpm(), 120.0, "{change:?}");
        assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 1);
        let canonical_tracks = Arc::clone(&fixture.app.state.project_tracks);
        let canonical_timeline = Arc::clone(&fixture.app.state.arrangement.timeline);
        fixture.app.poll_engine_events();
        assert!(Arc::ptr_eq(&canonical_tracks, &fixture.app.state.project_tracks));
        assert!(Arc::ptr_eq(&canonical_timeline, &fixture.app.state.arrangement.timeline));
        render(&mut fixture.engine, false);
        assert!(matches!(fixture.app.event_rx.as_ref().unwrap().peek().unwrap(),
            EngineEvent::ClipInsertionCompleted(completed)
                if completed.id() == fixture.operation && completed.outcome == ClipInsertionOutcome::Applied), "{change:?}");
        fixture.app.poll_engine_events();
        assert_eq!(fixture.engine.transport().bpm(), 137.0, "{change:?}");
        assert_eq!(fixture.app.cmd_tx.pending_len(), 0);
        assert!(!fixture.app.state.status_text.to_ascii_lowercase().contains("abandon"));
        let expected = match change {
            Change::DeleteTrack | Change::Clear => {
                assert!(fixture.app.state.find_track(fixture.track).is_none());
                assert!(fixture.app.state.arrange_content(fixture.track).is_none());
                assert!(fixture.engine.tracks().is_empty());
                0.0
            }
            Change::UndoReplay => {
                let content = fixture.app.state.arrange_content(fixture.track).unwrap();
                assert_eq!(content.clips.len(), 1);
                assert_eq!(content.clips[0].id, fixture.resident);
                assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 1);
                assert_eq!(Arc::as_ptr(&fixture.engine.tracks()[0].playback_source.clips[0].audio), fixture.resident_audio);
                0.25
            }
            Change::Current | Change::CanonicalOnlyStale | Change::Stop => {
                let source = &fixture.engine.tracks()[0].playback_source;
                assert_eq!(source.clips.len(), 2);
                assert_eq!(source.clips[1].id, fixture.incoming);
                assert_eq!(Arc::as_ptr(&source.clips[1].audio), fixture.audio.as_ptr());
                assert_eq!(source.clips[1].warp_markers.as_slice().as_ptr(), fixture.engine_markers);
                assert_eq!(source.clips[1].warp_markers, fixture.markers);
                if matches!(change, Change::CanonicalOnlyStale) {
                    assert!(fixture.app.state.arrange_content(fixture.track).is_none());
                } else {
                    let content = fixture.app.state.arrange_content(fixture.track).unwrap();
                    assert_eq!(content.clips[1].id, fixture.incoming);
                    assert_eq!(content.clips[1].name, "Incoming");
                    assert_eq!(content.clips[1].warp_markers, fixture.markers);
                }
                if matches!(change, Change::Stop) {
                    assert!(!fixture.engine.transport().is_playing());
                    assert_eq!(fixture.engine.transport().position(), 0);
                    0.0
                } else { 0.3125 }
            }
        };
        // Real routing edits can morph for64 frames. The blocked phase above
        // is exact immediately; final audio is checked after that existing morph.
        for _ in 0..8 {
            render(&mut fixture.engine, true);
            fixture.app.poll_engine_events();
        }
        assert_signal(&render(&mut fixture.engine, true), expected);
        drop(canonical_tracks);
        drop(canonical_timeline);
        drop(fixture.engine);
        drop(fixture.app);
        assert_eq!(fixture.audio.strong_count(), 0, "{change:?}");
    }
}

#[test]
fn clip_completion_polling_drops_unique_rejected_owners_with_current_or_stale_ui_targets() {
    for stale in [false, true] {
        let mut app = super::test_support::app();
        let (mut engine, commands, events) = AudioEngine::new();
        app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
        app.event_rx = Some(events);
        let track = TrackId::new();
        if !stale {
            Arc::make_mut(&mut app.state.project_tracks).tracks.push(
                ProjectTrack::new(track, "UI-only target".into(), 0));
        }
        let mut incoming = clip("Rejected", 0.0625);
        assert!(incoming.warp_markers.add(32, 64, 0, 128, 128));
        let clip_id = incoming.id;
        let audio = Arc::downgrade(&incoming.audio);
        let insertion = PreparedClipInsertion::new();
        let operation = insertion.id();
        app.send_command(command(track, &incoming, insertion));
        drop(incoming);
        assert_eq!(audio.strong_count(), 1);
        app.state.status_text = "Retain this status".into();
        render(&mut engine, false);
        assert_eq!(audio.strong_count(), 1);
        assert!(matches!(app.event_rx.as_ref().unwrap().peek().unwrap(),
            EngineEvent::ClipInsertionCompleted(completed)
                if completed.id() == operation
                    && completed.outcome == ClipInsertionOutcome::MissingTrack
                    && (completed.track_id, completed.clip_id) == (track, clip_id)));
        app.poll_engine_events();
        assert_eq!(audio.strong_count(), 0);
        assert_eq!(app.state.find_track(track).is_none(), stale);
        assert!(app.state.arrange_content(track).is_none());
        assert!(engine.tracks().is_empty());
        assert_eq!(app.state.status_text, "Retain this status");
    }
}

#[derive(Debug, Clone, Copy)]
enum ShutdownStage {
    RequestUnpolled,
    ReplyReady,
    CompletionUnpolled,
}

#[test]
fn clip_app_idle_stream_shutdown_disposes_undrained_request_reply_and_completion_owners() {
    for stage in [ShutdownStage::RequestUnpolled, ShutdownStage::ReplyReady,
        ShutdownStage::CompletionUnpolled] {
        let mut fixture = pending();
        if !matches!(stage, ShutdownStage::RequestUnpolled) {
            fixture.app.poll_engine_events();
            assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 1);
        }
        if matches!(stage, ShutdownStage::CompletionUnpolled) {
            render(&mut fixture.engine, false);
            assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 2);
            assert!(matches!(fixture.app.event_rx.as_ref().unwrap().peek().unwrap(),
                EngineEvent::ClipInsertionCompleted(completed) if completed.id() == fixture.operation));
        }
        assert!(fixture.audio.strong_count() > 0);
        fixture.app._stream = Some(vibez_audio_io::AudioOutputStream::idle(fixture.engine));
        drop(fixture.app);
        assert_eq!(fixture.audio.strong_count(), 0, "{stage:?}");
    }
}
