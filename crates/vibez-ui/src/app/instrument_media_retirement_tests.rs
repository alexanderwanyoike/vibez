//! Returned instrument media survives UI consumption until its final shared reference ends.

use super::*;
use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_engine::events::EngineEvent;
use vibez_instruments::InstrumentMediaOwners;

fn sample() -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![0.25; 128]],
        sample_rate: 48_000,
    })
}

#[test]
fn returned_media_keeps_one_ui_guard_while_other_owners_live_and_reaps_afterward() {
    let mut app = test_support::app();
    let (mut events, receiver) = rtrb::RingBuffer::new(64);
    app.event_rx = Some(receiver);
    let voice = sample();
    let weak = Arc::downgrade(&voice);
    for _ in 0..32 {
        events
            .push(EngineEvent::RetiredInstrumentMedia(
                InstrumentMediaOwners::incoming(Arc::clone(&voice), "repeated-owner.wav".into()),
            ))
            .unwrap();
    }
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 2, "one voice and exactly one UI guard");
    app.clear_project_runtime();
    assert_eq!(
        weak.strong_count(),
        2,
        "project reset cannot discard old-engine guards"
    );
    app.poll_engine_events();
    assert_eq!(weak.strong_count(), 2);
    drop(voice);
    app.poll_engine_events();
    assert_eq!(
        weak.strong_count(),
        0,
        "duplicate guards must not pin one another"
    );
}

#[test]
fn equal_audio_buffers_are_distinct_guards_and_ui_cache_owners_delay_reaping() {
    let mut app = test_support::app();
    let (mut events, receiver) = rtrb::RingBuffer::new(8);
    app.event_rx = Some(receiver);
    let first = sample();
    let second = sample();
    let weak_first = Arc::downgrade(&first);
    let weak_second = Arc::downgrade(&second);
    let cached = Arc::clone(&first);
    for owner in [&first, &second] {
        events
            .push(EngineEvent::RetiredInstrumentMedia(
                InstrumentMediaOwners::incoming(
                    Arc::clone(owner),
                    "same-name-and-values.wav".into(),
                ),
            ))
            .unwrap();
    }
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 3);
    assert_eq!(weak_second.strong_count(), 2);
    drop(first);
    drop(second);
    app.poll_engine_events();
    assert_eq!(
        weak_first.strong_count(),
        2,
        "a real cache owner is still present"
    );
    assert_eq!(weak_second.strong_count(), 0);
    drop(cached);
    app.poll_engine_events();
    assert_eq!(weak_first.strong_count(), 0);
}

#[test]
fn thirty_two_sounding_retired_generations_do_not_block_note_off_or_stop_and_ui_reaping() {
    use vibez_core::id::TrackId;
    use vibez_core::midi::InstrumentKind;
    use vibez_core::track::DrumPadState;
    use vibez_engine::commands::EngineCommand;
    use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

    for stop in [false, true] {
        let mut app = test_support::app();
        let (mut engine, mut commands, events) = AudioEngine::new();
        app.event_rx = Some(events);
        let track = TrackId::new();
        commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::SetRouting(
                vibez_engine::routing::PreparedRouting::prepare(
                    &[track, TrackId::MASTER].map(|id| vibez_core::routing::RoutingChannel {
                        id,
                        is_bus: id.is_master(),
                        effects: vec![],
                        sends: vec![],
                    }),
                    vibez_engine::routing::MAX_BLOCK_FRAMES,
                )
                .unwrap(),
            ))
            .unwrap();
        commands
            .push(EngineCommand::set_built_in_instrument(
                track,
                InstrumentKind::DrumRack,
                1000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::set_drum_rack_pad_state(
                track,
                0,
                DrumPadState {
                    fade_in_ms: 0.0,
                    fade_out_ms: 0.0,
                    one_shot: false,
                    ..Default::default()
                },
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [0.0; 1], 1));
        app.poll_engine_events();
        assert!(engine.transport().is_playing());

        let make_sample = || {
            Arc::new(DecodedAudio {
                channels: vec![vec![0.001; 4096]],
                sample_rate: 1000,
            })
        };
        let initial = make_sample();
        let mut old_generations = vec![Arc::downgrade(&initial)];
        commands
            .push(EngineCommand::LoadDrumRackPadSample {
                track_id: track,
                pad_index: 0,
                sample: initial,
                sample_name: "generation-0.wav".into(),
            })
            .unwrap();
        engine.process_block(AudioProcessBlock::new(&mut [], 1));
        app.poll_engine_events();
        // Distinct generations fill every voice slot without letting any voice finish.
        for generation in 0..32 {
            commands
                .push(EngineCommand::ExternalNoteOn {
                    track_id: track,
                    pitch: 36,
                    velocity: 127,
                })
                .unwrap();
            let mut sounding = [0.0; 1];
            engine.process_block(AudioProcessBlock::new(&mut sounding, 1));
            assert!((sounding[0] - (generation + 1) as f32 * 0.001).abs() < 1e-6);
            let next = make_sample();
            if generation < 31 {
                old_generations.push(Arc::downgrade(&next));
            }
            commands
                .push(EngineCommand::LoadDrumRackPadSample {
                    track_id: track,
                    pad_index: 0,
                    sample: next,
                    sample_name: format!("generation-{}.wav", generation + 1),
                })
                .unwrap();
            engine.process_block(AudioProcessBlock::new(&mut [], 1));
            app.poll_engine_events();
        }
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 2));
        let mut sustained = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut sustained, 1));
        app.poll_engine_events();
        assert!(sustained
            .iter()
            .all(|sample| (*sample - 0.032).abs() < 1e-6));
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 2));

        commands
            .push(if stop {
                EngineCommand::Stop
            } else {
                EngineCommand::ExternalNoteOff {
                    track_id: track,
                    pitch: 36,
                }
            })
            .unwrap();
        commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        let mut released = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut released, 1));
        assert_eq!(
            engine.transport().bpm(),
            137.0,
            "FIFO command after release must run"
        );
        assert_eq!(engine.transport().is_playing(), !stop);
        if stop {
            // Stop flushes scheduled notes; external live notes retain their existing behavior.
            assert!(released.iter().all(|sample| (*sample - 0.032).abs() < 1e-6));
            assert!(old_generations
                .iter()
                .all(|sample| sample.strong_count() == 2));
            app.poll_engine_events();
            assert!(old_generations
                .iter()
                .all(|sample| sample.strong_count() == 2));
            commands
                .push(EngineCommand::ExternalNoteOff {
                    track_id: track,
                    pitch: 36,
                })
                .unwrap();
            engine.process_block(AudioProcessBlock::new(&mut released, 1));
        }
        assert_eq!(
            released, [0.0; 16],
            "NoteOff must not wait for voice completion"
        );
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 1));
        app.poll_engine_events();
        assert!(old_generations
            .iter()
            .all(|sample| sample.strong_count() == 0));
    }
}

#[test]
fn actual_pad_sync_prepares_all_source_names_and_preserves_canonical_media_and_scalars() {
    use crate::state::{ProjectTrack, UiDrumPad};
    use vibez_core::id::TrackId;
    use vibez_core::track::{MediaProvenance, MediaSourceRef};
    use vibez_engine::commands::EngineCommand;

    let sources = [
        (
            Some(MediaSourceRef::LocalFile {
                path: "/samples/kick.wav".into(),
            }),
            Some("kick.wav"),
        ),
        (
            Some(MediaSourceRef::StagedProjectMedia {
                id: "staged-id".into(),
                file_name: "snare.wav".into(),
                staging_path: "/staging/snare.wav".into(),
                source_path: "/source/snare-original.wav".into(),
            }),
            Some("snare.wav"),
        ),
        (
            Some(MediaSourceRef::StagedRemoteProjectMedia {
                id: "remote-staged-id".into(),
                file_name: "remote.wav".into(),
                staging_path: "/staging/remote.wav".into(),
                provenance: Box::new(MediaProvenance::Remote {
                    provider: "dropbox".into(),
                    connection_id: "connection-id".into(),
                    connection_name: Some("Studio".into()),
                    source_id: "remote-source-id".into(),
                    source_path: "/remote/original.wav".into(),
                    revision: Some("revision-123".into()),
                }),
            }),
            Some("remote.wav"),
        ),
        (
            Some(MediaSourceRef::ProjectMedia {
                id: "embedded-id".into(),
                file_name: "embedded.wav".into(),
                provenance: Some(Box::new(MediaProvenance::Local {
                    source_path: "/old/original.wav".into(),
                })),
            }),
            Some("embedded.wav"),
        ),
        (
            Some(MediaSourceRef::DropboxFile {
                path_lower: "/drums/hat.wav".into(),
                display_path: "/Drums/Hat.wav".into(),
                rev: Some("dropbox-revision".into()),
            }),
            Some("Hat.wav"),
        ),
        (None, None),
    ];
    for (source, fallback) in sources {
        for explicit in [None, Some("Custom name"), Some("")] {
            let mut app = test_support::app();
            let (commands, mut received) = rtrb::RingBuffer::new(8);
            app.cmd_tx = crate::domains::EngineCommandQueue::new(commands);
            let track_id = TrackId::new();
            let canonical_audio = sample();
            let audio_pointer = Arc::as_ptr(&canonical_audio);
            let pad = UiDrumPad {
                name: explicit.map(str::to_owned),
                source: source.clone(),
                audio: Some(canonical_audio),
                gain: 0.37,
                pan: -0.23,
                start: 0.12,
                end: 0.83,
                fade_in_ms: -2.0,
                fade_out_ms: 300.0,
                coarse_tune: -7,
                fine_tune: 31.25,
                one_shot: false,
                choke_group: Some(11),
            };
            let canonical = pad.to_state();
            let serialized = serde_json::to_value(&canonical).unwrap();
            let mut track = ProjectTrack::new(track_id, "Canonical drum rack".into(), 0);
            track.drum_rack_pads = vec![pad];
            Arc::make_mut(&mut app.state.project_tracks)
                .tracks
                .push(track);
            app.sync_drum_rack_pad_state(track_id, 0);
            let EngineCommand::SetDrumRackPadState {
                track_id: actual_track,
                pad_index,
                state,
            } = received.pop().unwrap()
            else {
                panic!("actual App sync must send the prepared state command");
            };
            assert_eq!(actual_track, track_id);
            assert_eq!(pad_index, 0);
            assert_eq!(state.name.as_deref(), explicit.or(fallback));
            assert_eq!(state.gain, canonical.gain);
            assert_eq!(state.pan, canonical.pan);
            assert_eq!(state.start, canonical.start);
            assert_eq!(state.end, canonical.end);
            // Preparation preserves settings; the resident rack retains its existing fade clamp.
            assert_eq!(state.fade_in_ms, canonical.fade_in_ms);
            assert_eq!(state.fade_out_ms, canonical.fade_out_ms);
            assert_eq!(state.coarse_tune, canonical.coarse_tune);
            assert_eq!(state.fine_tune, canonical.fine_tune);
            assert_eq!(state.one_shot, canonical.one_shot);
            assert_eq!(state.choke_group, canonical.choke_group);
            let actual = &app.state.find_track(track_id).unwrap().drum_rack_pads[0];
            assert_eq!(actual.to_state(), canonical);
            assert_eq!(serde_json::to_value(actual.to_state()).unwrap(), serialized);
            assert_eq!(actual.audio.as_ref().map(Arc::as_ptr), Some(audio_pointer));
            assert!(received.pop().is_err());
        }
    }
}

#[test]
fn stale_media_returns_queued_before_or_delivered_after_project_reset_are_guarded_and_reaped() {
    use crate::state::ProjectTrack;
    use vibez_core::id::TrackId;

    for queued_before_reset in [false, true] {
        let mut app = test_support::app();
        let (mut events, receiver) = rtrb::RingBuffer::new(8);
        app.event_rx = Some(receiver);
        let old_track = TrackId::new();
        let voice = sample();
        let old_audio = Arc::downgrade(&voice);
        let mut track = ProjectTrack::new(old_track, "Removed old sampler".into(), 0);
        track.sample_audio = Some(Arc::clone(&voice));
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let mut old_return = Some(EngineEvent::RetiredInstrumentMedia(
            InstrumentMediaOwners::incoming(Arc::clone(&voice), "old-project-media.wav".into()),
        ));
        if queued_before_reset {
            events.push(old_return.take().unwrap()).unwrap();
        }
        app.clear_project_runtime();
        assert!(app.state.find_track(old_track).is_none());
        assert_eq!(
            old_audio.strong_count(),
            2,
            "old voice and unconsumed return remain"
        );
        // A cache in the current UI can independently share media returned by the old engine.
        app.state.browser.waveform_audio = Some(Arc::clone(&voice));
        if let Some(event) = old_return {
            events.push(event).unwrap();
        }
        app.poll_engine_events();
        assert_eq!(
            events.slots(),
            8,
            "owner delivery releases ordinary event-ring capacity"
        );
        assert!(app.state.find_track(old_track).is_none());
        assert_eq!(
            old_audio.strong_count(),
            3,
            "same allocation is held by voice, cache and actual App guard"
        );
        assert_eq!(
            app.state.browser.waveform_audio.as_ref().map(Arc::as_ptr),
            Some(old_audio.as_ptr())
        );
        drop(voice);
        app.poll_engine_events();
        assert_eq!(
            old_audio.strong_count(),
            2,
            "cache keeps the old allocation readable"
        );
        app.state.browser.waveform_audio = None;
        assert_eq!(old_audio.strong_count(), 1, "only actual App guard remains");
        app.poll_engine_events();
        assert_eq!(
            old_audio.strong_count(),
            0,
            "last owner is released by UI polling"
        );
    }
}
