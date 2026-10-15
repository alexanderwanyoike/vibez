//! Actual Piano Roll edits keep canonical state and queued teardown separate from pending engine note growth.

use super::*;
use crate::domains::piano_roll::PianoRollMsg;

struct InnerPending {
    outer: Pending,
    operation: ClipInsertionId,
    note: MidiNote,
}

fn pending_inner() -> InnerPending {
    let mut outer = pending();
    outer.app.poll_engine_events();
    render(&mut outer.engine, false);
    outer.app.poll_engine_events();
    assert_eq!(outer.engine.tracks()[0].playback_source.note_clips.len(), 2);
    assert_eq!(
        outer.engine.tracks()[0].playback_source.note_clips[0]
            .notes
            .len(),
        1
    );
    assert_eq!(
        outer.engine.tracks()[0].playback_source.note_clips[0]
            .notes
            .capacity(),
        1
    );
    outer.app.state.arrangement.selected_note_clip = Some((outer.track, outer.resident));
    let note = MidiNote {
        pitch: 60,
        velocity: 100,
        start_beat: 1.0,
        duration_beats: 0.5,
    };
    let _ = outer
        .app
        .update_and_refresh_clips(Message::PianoRoll(PianoRollMsg::AddNote {
            track_id: outer.track,
            clip_id: outer.resident,
            pitch: note.pitch,
            start_beat: note.start_beat,
            duration_beats: note.duration_beats,
        }));
    let canonical = outer.app.state.arrange_content(outer.track).unwrap();
    assert_eq!(canonical.note_clips[0].notes.len(), 2);
    assert_eq!(canonical.note_clips[0].notes[1], note);
    render(&mut outer.engine, false);
    let EngineEvent::ClipInsertionStorageRequested(request) =
        outer.app.event_rx.as_ref().unwrap().peek().unwrap()
    else {
        panic!("the real drawing message must exhaust both resident note arrays");
    };
    let operation = request.id.clone();
    assert_eq!(
        (request.track_id, request.clip_id),
        (outer.track, outer.resident)
    );
    assert_eq!(
        outer.engine.tracks()[0].playback_source.note_clips[0]
            .notes
            .len(),
        1
    );
    InnerPending {
        outer,
        operation,
        note,
    }
}

#[derive(Clone, Copy, Debug)]
enum InnerChange {
    Current,
    DoubleBatch,
    CanonicalStale,
    DeleteTrack,
    ReuseIds,
    UndoReplay,
    Stop,
    Clear,
}

#[test]
fn piano_roll_note_growth_is_served_before_current_stale_batch_reuse_undo_stop_and_clear_lookup() {
    for change in [
        InnerChange::Current,
        InnerChange::DoubleBatch,
        InnerChange::CanonicalStale,
        InnerChange::DeleteTrack,
        InnerChange::ReuseIds,
        InnerChange::UndoReplay,
        InnerChange::Stop,
        InnerChange::Clear,
    ] {
        let InnerPending {
            mut outer,
            operation,
            note,
        } = pending_inner();
        let old = outer.engine.tracks()[0].playback_source.note_clips[0].notes[0];
        let reused = MidiNote {
            pitch: 74,
            velocity: 55,
            start_beat: 2.5,
            duration_beats: 0.75,
        };
        match change {
            InnerChange::Current => {}
            InnerChange::DoubleBatch => {
                let _ = outer.app.update_and_refresh_clips(Message::PianoRoll(
                    PianoRollMsg::DoubleNoteClip(outer.track, outer.resident),
                ));
                let canonical = outer.app.state.arrange_content(outer.track).unwrap();
                assert_eq!(canonical.note_clips[0].notes.len(), 4);
                assert_eq!(canonical.note_clips[0].duration_beats, 16.0);
            }
            InnerChange::CanonicalStale => {
                outer.app.state.arrangement.timeline = Arc::new(Default::default())
            }
            InnerChange::DeleteTrack | InnerChange::ReuseIds => {
                let _ = outer.app.update_and_refresh_clips(Message::Arrangement(
                    ArrangementMsg::RemoveTrack(outer.track),
                ));
                if matches!(change, InnerChange::ReuseIds) {
                    let mut model =
                        ProjectTrack::new(outer.track, "Reused Track identity".into(), 0);
                    model.pan = 0.0;
                    Arc::make_mut(&mut outer.app.state.project_tracks)
                        .tracks
                        .push(model.clone());
                    let mut replacement = note_clip("Reused Clip identity", 2.0);
                    replacement.id = outer.resident;
                    replacement.notes.push(reused);
                    Arc::make_mut(&mut outer.app.state.arrangement.timeline)
                        .ensure(outer.track)
                        .note_clips
                        .push(replacement);
                    outer.app.replay_track_to_engine(&model);
                    outer.app.sync_sidechain_routing();
                }
            }
            InnerChange::UndoReplay => {
                let _ = outer
                    .app
                    .update_and_refresh_clips(Message::Project(ProjectMsg::Undo));
            }
            InnerChange::Stop => {
                let _ = outer
                    .app
                    .update_and_refresh_clips(Message::Transport(TransportMsg::Stop));
            }
            InnerChange::Clear => outer.app.clear_project_runtime(),
        }
        outer.app.send_command(EngineCommand::SetBpm(137.0));
        assert_signal(&render(&mut outer.engine, true), 0.25);
        assert_eq!(outer.engine.transport().position(), 16, "{change:?}");
        assert!(outer.engine.transport().is_playing(), "{change:?}");
        assert_eq!(outer.engine.transport().bpm(), 120.0);
        let resident = &outer.engine.tracks()[0].playback_source.note_clips[0];
        assert_eq!(resident.id, outer.resident);
        assert_eq!(resident.notes, [old]);
        assert_eq!(resident.notes.as_ptr(), outer.resident_notes);
        let canonical_tracks = Arc::clone(&outer.app.state.project_tracks);
        let canonical_timeline = Arc::clone(&outer.app.state.arrangement.timeline);
        outer.app.poll_engine_events();
        assert!(Arc::ptr_eq(
            &canonical_tracks,
            &outer.app.state.project_tracks
        ));
        assert!(Arc::ptr_eq(
            &canonical_timeline,
            &outer.app.state.arrangement.timeline
        ));
        render(&mut outer.engine, false);
        assert!(
            matches!(outer.app.event_rx.as_ref().unwrap().peek().unwrap(),
            EngineEvent::ClipInsertionCompleted(owner) if owner.id() == operation
                && owner.outcome == ClipInsertionOutcome::Applied
                && (owner.track_id, owner.clip_id) == (outer.track, outer.resident)),
            "{change:?}"
        );
        // Undo/replay and batch expansion may themselves need the same real
        // exchange. App must serve each request, not just the original edit.
        let exchanges = 2 + outer
            .app
            .state
            .arrange_content(outer.track)
            .map_or(0, |content| {
                content
                    .note_clips
                    .iter()
                    .map(|clip| 1 + clip.notes.len())
                    .sum::<usize>()
            });
        for _ in 0..=exchanges {
            outer.app.poll_engine_events();
            render(&mut outer.engine, false);
        }
        outer.app.poll_engine_events();
        assert_eq!(outer.app.cmd_tx.pending_len(), 0);
        assert_eq!(outer.engine.transport().bpm(), 137.0, "{change:?}");
        assert!(!outer
            .app
            .state
            .status_text
            .to_ascii_lowercase()
            .contains("abandon"));
        let expected_signal = match change {
            InnerChange::DeleteTrack | InnerChange::Clear => {
                assert!(outer.app.state.find_track(outer.track).is_none());
                assert!(outer.app.state.arrange_content(outer.track).is_none());
                assert!(outer.engine.tracks().is_empty());
                0.0
            }
            InnerChange::ReuseIds => {
                let canonical = outer.app.state.arrange_content(outer.track).unwrap();
                assert_eq!(canonical.note_clips.len(), 1);
                assert_eq!(canonical.note_clips[0].id, outer.resident);
                assert_eq!(canonical.note_clips[0].name, "Reused Clip identity");
                assert_eq!(canonical.note_clips[0].notes, [reused]);
                let actual = &outer.engine.tracks()[0].playback_source.note_clips;
                assert_eq!(actual.len(), 1);
                assert_eq!(actual[0].id, outer.resident);
                assert_eq!(actual[0].notes, [reused]);
                assert_eq!(actual[0].position_beats, 2.0);
                assert!(outer.engine.tracks()[0].instrument.is_none());
                0.0
            }
            InnerChange::UndoReplay => {
                let canonical = outer.app.state.arrange_content(outer.track).unwrap();
                assert_eq!(canonical.note_clips.len(), 2);
                assert_eq!(canonical.note_clips[0].notes, [old]);
                assert!(canonical.note_clips[1].notes.is_empty());
                let actual = &outer.engine.tracks()[0].playback_source.note_clips;
                assert_eq!(actual.len(), 2);
                assert_eq!(actual[0].notes, [old]);
                assert!(actual[1].notes.is_empty());
                assert!(outer.engine.tracks()[0].instrument.is_none());
                0.125
            }
            InnerChange::Current
            | InnerChange::CanonicalStale
            | InnerChange::Stop
            | InnerChange::DoubleBatch => {
                let actual = &outer.engine.tracks()[0].playback_source.note_clips;
                assert_eq!(actual.len(), 2);
                assert_eq!(actual[0].id, outer.resident);
                assert_eq!(actual[1].id, outer.incoming.id);
                assert!(actual[1].notes.is_empty());
                if matches!(change, InnerChange::DoubleBatch) {
                    let later_old = MidiNote {
                        start_beat: old.start_beat + 8.0,
                        ..old
                    };
                    let later_new = MidiNote {
                        start_beat: note.start_beat + 8.0,
                        ..note
                    };
                    assert_eq!(actual[0].notes, [old, note, later_old, later_new]);
                    assert_eq!(actual[0].duration_beats, 16.0);
                    let canonical = outer.app.state.arrange_content(outer.track).unwrap();
                    assert_eq!(canonical.note_clips[0].notes, actual[0].notes);
                    // Double's existing duration update flushes the held voice.
                    0.0
                } else {
                    assert_eq!(actual[0].notes, [old, note]);
                    assert_eq!(actual[0].duration_beats, 8.0);
                    if matches!(change, InnerChange::CanonicalStale) {
                        assert!(outer.app.state.arrange_content(outer.track).is_none());
                    } else {
                        let canonical = outer.app.state.arrange_content(outer.track).unwrap();
                        assert_eq!(canonical.note_clips[0].notes, [old, note]);
                    }
                    if matches!(change, InnerChange::Stop) {
                        assert!(!outer.engine.transport().is_playing());
                        assert_eq!(outer.engine.transport().position(), 0);
                        assert!(!outer.app.state.transport.playing);
                        0.0
                    } else {
                        0.25
                    }
                }
            }
        };
        for _ in 0..8 {
            render(&mut outer.engine, true);
            outer.app.poll_engine_events();
        }
        assert_signal(&render(&mut outer.engine, true), expected_signal);
        if matches!(change, InnerChange::Clear) {
            assert!(!outer.app.state.transport.playing);
        }
        drop(canonical_tracks);
        drop(canonical_timeline);
        drop(outer.engine);
        drop(outer.app);
        assert_eq!(outer.audio.strong_count(), 0, "{change:?}");
    }
}

#[test]
fn drawn_note_growth_idle_stream_shutdown_handles_request_reply_and_complete_owners() {
    for stage in [
        ShutdownStage::RequestUnpolled,
        ShutdownStage::ReplyReady,
        ShutdownStage::CompletionUnpolled,
    ] {
        let InnerPending {
            mut outer,
            operation,
            ..
        } = pending_inner();
        if !matches!(stage, ShutdownStage::RequestUnpolled) {
            outer.app.poll_engine_events();
            assert_eq!(
                outer.engine.tracks()[0].playback_source.note_clips[0]
                    .notes
                    .len(),
                1
            );
        }
        if matches!(stage, ShutdownStage::CompletionUnpolled) {
            render(&mut outer.engine, false);
            assert_eq!(
                outer.engine.tracks()[0].playback_source.note_clips[0]
                    .notes
                    .len(),
                2
            );
            assert!(
                matches!(outer.app.event_rx.as_ref().unwrap().peek().unwrap(),
                EngineEvent::ClipInsertionCompleted(owner) if owner.id() == operation)
            );
        }
        assert!(outer.audio.strong_count() > 0);
        outer.app._stream = Some(vibez_audio_io::audio_stream::AudioOutputStream::idle(
            outer.engine,
        ));
        // This base drops UI fields/endpoints before its idle stream. The
        // actual App field order must still finish every queued mailbox owner.
        drop(outer.app);
        assert_eq!(outer.audio.strong_count(), 0, "{stage:?}");
    }
}
