//! Actual App polling serves prepared note storage before stale-target lookup and during teardown.

use super::*;
use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::state::{ProjectTrack, UiClip, UiNoteClip};
use std::sync::Weak;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;
use vibez_engine::clip_insertion::{ClipInsertionId, ClipInsertionOutcome, PreparedNoteClipInsertion};
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};
use vibez_engine::events::EngineEvent;

fn clip(name: &str, value: f32) -> UiClip {
    UiClip {
        id: ClipId::new(),
        name: name.into(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![value; 4096]],
            sample_rate: 44_100,
        }),
        source: None,
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration: 4096,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: Some(120.0),
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    }
}


fn note_clip(name: &str, position: f64) -> UiNoteClip {
    UiNoteClip {
        id: ClipId::new(),
        name: name.into(),
        position_beats: position,
        duration_beats: 8.0,
        notes: Vec::new(),
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 8.0,
        groove_grid: GrooveGrid::Off,
    }
}

fn render(engine: &mut AudioEngine, sounding: bool) -> [f32; 16] {
    let mut output = [0.0; 16];
    let length = if sounding { output.len() } else { 0 };
    engine.process_block(AudioProcessBlock::new(&mut output[..length], 2));
    output
}

fn assert_signal(output: &[f32], expected: f32) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - expected).abs() < 1e-7, "expected {expected}, actual {left}");
        assert_eq!(right, 0.0);
    }
}

struct Pending {
    app: App,
    engine: AudioEngine,
    track: TrackId,
    resident: ClipId,
    resident_notes: *const MidiNote,
    audio: Weak<DecodedAudio>,
    incoming: UiNoteClip,
    operation: ClipInsertionId,
}

fn pending() -> Pending {
    let mut app = super::test_support::app();
    app.state.confirm_project_track_deletion = false;
    let (mut engine, commands, events) = AudioEngine::new();
    app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
    app.event_rx = Some(events);
    let track = TrackId::new();
    let mut model = ProjectTrack::new(track, "Note insertion target".into(), 0);
    model.pan = 0.0;
    Arc::make_mut(&mut app.state.project_tracks).tracks.push(model.clone());
    let audio_clip = clip("Resident audio", 0.125);
    let audio = Arc::downgrade(&audio_clip.audio);
    let mut resident = note_clip("Resident notes", 0.0);
    resident.notes.push(MidiNote { pitch: 60, velocity: 100, start_beat: 0.0, duration_beats: 4.0 });
    let resident_id = resident.id;
    let content = Arc::make_mut(&mut app.state.arrangement.timeline).ensure(track);
    content.clips.push(audio_clip);
    content.note_clips.push(resident);
    app.replay_track_to_engine(&model);
    // The probe makes the real scheduled note audible numerically. Undo must
    // restore the canonical instrument-free model, rather than retain this fixture.
    app.send_command(EngineCommand::SetInstrument {
        track_id: track,
        instrument: Box::new(vibez_engine::test_support::DelayProbeInstrument::new(0, 0.25)),
    });
    app.sync_sidechain_routing();
    app.send_command(EngineCommand::Play);
    assert_signal(&render(&mut engine, true), 0.25);
    app.poll_engine_events();
    assert_eq!(engine.transport().position(), 8);
    let notes = &engine.tracks()[0].playback_source.note_clips;
    assert_eq!((notes.len(), notes.capacity()), (1, 1));
    let resident_notes = notes[0].notes.as_ptr();
    app.push_undo_snapshot(None);
    let mut incoming = note_clip("Prepared incoming notes", 10.0);
    incoming.duration_beats = 5.5;
    incoming.start_marker_beats = 0.75;
    incoming.loop_enabled = true;
    incoming.loop_start_beats = 0.25;
    incoming.loop_end_beats = 2.5;
    incoming.groove_grid = GrooveGrid::Eighth;
    let insertion = PreparedNoteClipInsertion::new(
        incoming.id, incoming.position_beats, incoming.duration_beats,
        incoming.start_marker_beats, incoming.loop_enabled,
        incoming.loop_start_beats, incoming.loop_end_beats, incoming.groove_grid,
    );
    let operation = insertion.id();
    Arc::make_mut(&mut app.state.arrangement.timeline).ensure(track).note_clips.push(incoming.clone());
    app.send_command(EngineCommand::AddNoteClip { track_id: track, insertion });
    render(&mut engine, false);
    assert!(matches!(app.event_rx.as_ref().unwrap().peek().unwrap(),
        EngineEvent::ClipInsertionStorageRequested(request) if request.id == operation));
    assert_eq!(engine.tracks()[0].playback_source.note_clips.len(), 1);
    Pending { app, engine, track, resident: resident_id, resident_notes, audio, incoming, operation }
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
fn note_request_polling_serves_current_stale_delete_undo_stop_and_clear_state() {
    for change in [Change::Current, Change::CanonicalOnlyStale, Change::DeleteTrack,
        Change::UndoReplay, Change::Stop, Change::Clear] {
        let mut fixture = pending();
        match change {
            Change::Current => {}
            Change::CanonicalOnlyStale => {
                // Isolate a late absent UI target without inventing an engine cancellation.
                fixture.app.state.arrangement.timeline = Arc::new(Default::default());
            }
            Change::DeleteTrack => {
                let _ = fixture.app.update_and_refresh_clips(Message::Arrangement(
                    ArrangementMsg::RemoveTrack(fixture.track)));
            }
            Change::UndoReplay => {
                let _ = fixture.app.update_and_refresh_clips(Message::Project(ProjectMsg::Undo));
            }
            Change::Stop => {
                let _ = fixture.app.update_and_refresh_clips(Message::Transport(TransportMsg::Stop));
            }
            Change::Clear => fixture.app.clear_project_runtime(),
        }
        fixture.app.send_command(EngineCommand::SetBpm(137.0));
        assert_signal(&render(&mut fixture.engine, true), 0.25);
        assert_eq!(fixture.engine.transport().position(), 16, "{change:?}");
        assert!(fixture.engine.transport().is_playing(), "{change:?}");
        assert_eq!(fixture.engine.transport().bpm(), 120.0, "{change:?}");
        assert_eq!(fixture.engine.tracks()[0].playback_source.note_clips.len(), 1);
        assert_eq!(fixture.engine.tracks()[0].playback_source.note_clips[0].notes.as_ptr(), fixture.resident_notes);
        let canonical_tracks = Arc::clone(&fixture.app.state.project_tracks);
        let canonical_timeline = Arc::clone(&fixture.app.state.arrangement.timeline);
        fixture.app.poll_engine_events();
        assert!(Arc::ptr_eq(&canonical_tracks, &fixture.app.state.project_tracks));
        assert!(Arc::ptr_eq(&canonical_timeline, &fixture.app.state.arrangement.timeline));
        render(&mut fixture.engine, false);
        assert!(matches!(fixture.app.event_rx.as_ref().unwrap().peek().unwrap(),
            EngineEvent::ClipInsertionCompleted(completed)
                if completed.id() == fixture.operation && completed.outcome == ClipInsertionOutcome::Applied
                    && (completed.track_id, completed.clip_id) == (fixture.track, fixture.incoming.id)), "{change:?}");
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
                assert_eq!(content.note_clips.len(), 1);
                assert_eq!(content.note_clips[0].id, fixture.resident);
                let notes = &fixture.engine.tracks()[0].playback_source.note_clips;
                assert_eq!(notes.len(), 1);
                assert_eq!(notes[0].id, fixture.resident);
                assert_eq!(notes[0].notes, content.note_clips[0].notes);
                assert!(fixture.engine.tracks()[0].instrument.is_none());
                0.125
            }
            Change::Current | Change::CanonicalOnlyStale | Change::Stop => {
                let notes = &fixture.engine.tracks()[0].playback_source.note_clips;
                assert_eq!(notes.len(), 2);
                assert_eq!(notes[0].id, fixture.resident);
                assert_eq!(notes[0].notes.as_ptr(), fixture.resident_notes);
                let actual = &notes[1];
                let expected = &fixture.incoming;
                assert_eq!(actual.id, expected.id);
                assert_eq!(actual.position_beats, expected.position_beats);
                assert_eq!(actual.duration_beats, expected.duration_beats);
                assert_eq!(actual.start_marker_beats, expected.start_marker_beats);
                assert_eq!(actual.loop_enabled, expected.loop_enabled);
                assert_eq!(actual.loop_start_beats, expected.loop_start_beats);
                assert_eq!(actual.loop_end_beats, expected.loop_end_beats);
                assert_eq!(actual.groove_grid, expected.groove_grid);
                assert!(actual.notes.is_empty());
                if matches!(change, Change::CanonicalOnlyStale) {
                    assert!(fixture.app.state.arrange_content(fixture.track).is_none());
                } else {
                    let content = fixture.app.state.arrange_content(fixture.track).unwrap();
                    assert_eq!(content.note_clips[1].id, expected.id);
                    assert_eq!(content.note_clips[1].name, expected.name);
                    assert_eq!(content.note_clips[1].notes, expected.notes);
                }
                if matches!(change, Change::Stop) {
                    assert!(!fixture.engine.transport().is_playing());
                    assert_eq!(fixture.engine.transport().position(), 0);
                    0.0
                } else { 0.25 }
            }
        };
        // Existing routing edits morph for64 frames before final-state audio is stable.
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

#[derive(Debug, Clone, Copy)]
enum ShutdownStage { RequestUnpolled, ReplyReady, CompletionUnpolled }

#[test]
fn note_app_idle_stream_shutdown_consumes_each_real_protocol_stage() {
    for stage in [ShutdownStage::RequestUnpolled, ShutdownStage::ReplyReady, ShutdownStage::CompletionUnpolled] {
        let mut fixture = pending();
        if !matches!(stage, ShutdownStage::RequestUnpolled) {
            fixture.app.poll_engine_events();
            assert_eq!(fixture.engine.tracks()[0].playback_source.note_clips.len(), 1);
        }
        if matches!(stage, ShutdownStage::CompletionUnpolled) {
            render(&mut fixture.engine, false);
            assert_eq!(fixture.engine.tracks()[0].playback_source.note_clips.len(), 2);
            assert!(matches!(fixture.app.event_rx.as_ref().unwrap().peek().unwrap(),
                EngineEvent::ClipInsertionCompleted(completed) if completed.id() == fixture.operation));
        }
        assert!(fixture.audio.strong_count() > 0);
        fixture.app._stream = Some(vibez_audio_io::audio_stream::AudioOutputStream::idle(fixture.engine));
        drop(fixture.app);
        assert_eq!(fixture.audio.strong_count(), 0, "{stage:?}");
    }
}
