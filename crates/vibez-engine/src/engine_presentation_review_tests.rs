//! Retained state remains ordered and owned across cancellation and overflow.

use super::*;
use crate::mixer::QueuedTrackMute;
use crate::playback_source::{PreparedClipPlayback, PreparedSectionPlaybackSource};
use vibez_core::id::ClipId;

fn full_ring(engine: &mut AudioEngine) {
    while engine
        .event_tx
        .push(EngineEvent::Metering {
            peak_l: 0.0,
            peak_r: 0.0,
            rms_l: 0.0,
            rms_r: 0.0,
        })
        .is_ok()
    {}
}
fn section(id: SectionId) -> Box<PreparedSectionPlaybackSource> {
    Box::new(PreparedSectionPlaybackSource::new(id, 8.0, true, vec![]))
}
fn local_recovery(engine: &mut AudioEngine) {
    engine.pending_device_reconfiguration = Some(reconfiguration::PendingDeviceReconfiguration {
        handoff_id: 1,
        track_id: TrackId::new(),
        effect_id: None,
        position: 0,
        bypass: false,
        local_recovery: true,
    });
}

#[test]
fn rejected_capture_during_recovery_preserves_applied_section_acknowledgement_and_owner() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = SectionId::new();
    full_ring(&mut engine);
    let source = section(id);
    let identity = &*source as *const _;
    engine.activate_section(source, 0);
    let note_track = TrackId::new();
    engine.command_external_note_on(note_track, 42, 100);
    local_recovery(&mut engine);
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.command_start_performance_capture()),
        (0, 0)
    );
    while events.pop().is_ok() {}
    engine.flush_presentation();
    let mut saw = false;
    let mut started = false;
    let mut recording_note = false;
    while let Ok(event) = events.pop() {
        started |= matches!(event, EngineEvent::PlaybackStarted);
        recording_note |= matches!(event, EngineEvent::InstrumentNoteInput { section_id: Some(section), track_id, .. } if section == id && track_id == note_track);
        if let EngineEvent::SectionTransitioned {
            section_id,
            retired,
            ..
        } = event
        {
            assert_eq!(section_id, id);
            assert_eq!(&*retired as *const _, identity);
            saw = true;
        }
    }
    assert!(saw && started && recording_note);
    assert!(engine.transport.is_playing());
    assert_eq!(engine.active_section.unwrap().section_id, id);
}

#[test]
fn perform_seek_preserves_applied_section_acknowledgements() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = SectionId::new();
    full_ring(&mut engine);
    engine.activate_section(section(id), 0);
    engine.command_seek(900);
    while events.pop().is_ok() {}
    engine.flush_presentation();
    assert!(
        std::iter::from_fn(|| events.pop().ok()).any(|event| matches!(event,
        EngineEvent::SectionTransitioned { section_id, .. } if section_id == id))
    );
    assert!(engine.transport.is_playing());
}

#[test]
fn queued_quantized_mutes_never_overflow_retirement_after_presentation_fault() {
    let (mut engine, _, _) = AudioEngine::new();
    for _ in 0..40 {
        let mut track = EngineTrack::new(TrackId::new());
        track.queued_mute = Some(QueuedTrackMute {
            muted: true,
            effective_at_samples: 8,
            end_of_section: false,
        });
        engine.tracks.push(track);
    }
    engine.transport.play();
    full_ring(&mut engine);
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(521_u64),
        );
    }
    let capacity = engine.pending_retirements.capacity();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.apply_track_mutes_due(8)),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.capacity(), capacity);
    assert!(engine
        .tracks
        .iter()
        .all(|track| !track.mute && track.queued_mute.is_some()));
}

#[test]
fn stop_and_play_outside_clip_performance_survive_full_ring() {
    for play in [false, true] {
        let (mut engine, _, mut events) = AudioEngine::new();
        if !play {
            engine.transport.play();
        }
        full_ring(&mut engine);
        if play {
            engine.command_play();
        } else {
            engine.command_stop();
        }
        while events.pop().is_ok() {}
        engine.flush_presentation();
        assert!(
            std::iter::from_fn(|| events.pop().ok()).any(|event| match event {
                EngineEvent::PlaybackStarted => play,
                EngineEvent::PlaybackStopped => !play,
                _ => false,
            })
        );
    }
}

#[test]
fn an_earlier_due_mute_cannot_be_overtaken_after_ring_drain() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = TrackId::new();
    full_ring(&mut engine);
    engine.present_event(EngineEvent::TrackMuteChanged {
        track_id: id,
        muted: true,
        effective_at_samples: 0,
    });
    while events.pop().is_ok() {}
    engine.present_event(EngineEvent::TrackMuteChanged {
        track_id: id,
        muted: false,
        effective_at_samples: 0,
    });
    engine.flush_presentation();
    let values: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::TrackMuteChanged { muted, .. } => Some(muted),
            _ => None,
        })
        .collect();
    assert_eq!(values, [true, false]);
}

#[test]
fn faulted_packet_publication_and_snapshots_cannot_resurrect_playing_clips() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let track = TrackId::new();
    engine.tracks.push(EngineTrack::new(track));
    let model = [
        vibez_core::routing::RoutingChannel {
            id: track,
            is_bus: false,
            sends: vec![],
            effects: vec![],
        },
        vibez_core::routing::RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    engine.routing = Some(crate::routing::PreparedRouting::prepare(&model, 8).unwrap());
    engine.begin_clip_performance();
    while events.pop().is_ok() {}
    engine.performance_position = 1;
    engine.queue_clips(
        vec![PreparedClipPlayback::stop(track, 0)],
        vibez_core::perform::MusicalBoundary::OneBar,
    );
    while events.pop().is_ok() {}
    full_ring(&mut engine);
    for _ in 0..presentation_queue::PRESENTATION_EVENT_CAPACITY - 3 {
        engine.present_event_at(
            EngineEvent::PlaybackStarted,
            engine.output_position.saturating_add(5000_u64),
        );
    }
    let clip_id = ClipId::new();
    let mut source = PreparedClipPlayback::stop(track, 1);
    source.clip_id = Some(clip_id);
    source.length_samples = 100;
    source.looping = true;
    engine.queue_clips(
        vec![source],
        vibez_core::perform::MusicalBoundary::Immediate,
    );
    assert!(engine.pending_clip_batch.is_some());
    assert!(engine.tracks[0].active_clip.is_some());
    engine.fail_presentation();
    let mut playing = false;
    let mut stopped = false;
    let mut retired = 0;
    for _ in 0..10 {
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::PlaybackStopped => {
                    stopped = true;
                    playing = false;
                }
                EngineEvent::ClipTransitioned {
                    clip_id: Some(_), ..
                } => playing = true,
                EngineEvent::ClipStateResynced(snapshot) => playing = snapshot.playing.is_some(),
                EngineEvent::ClipRequestRetired(_) => retired += 1,
                _ => {}
            }
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 1], 1)),
            (0, 0)
        );
    }
    assert!(stopped);
    assert!(!playing);
    assert_eq!(retired, 2);
    assert!(engine.pending_clip_batch.is_none());
    assert!(engine.tracks[0].active_clip.is_none());
}

#[test]
fn large_projects_accept_ordinary_commands_and_resumably_cancel_mutes() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    engine
        .tracks
        .extend((0..2058).map(|_| EngineTrack::new(TrackId::new())));
    commands.push(EngineCommand::Play).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert!(engine.transport.is_playing());
    for track in &mut engine.tracks {
        track.queued_mute = Some(QueuedTrackMute {
            muted: true,
            effective_at_samples: 9000,
            end_of_section: false,
        });
    }
    full_ring(&mut engine);
    commands.push(EngineCommand::Stop).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.drain_commands()),
        (0, 0)
    );
    assert!(!engine.transport.is_playing());
    let mut cancelled = 0;
    for _ in 0..12 {
        while let Ok(event) = events.pop() {
            cancelled += usize::from(matches!(event, EngineEvent::TrackMuteQueueCancelled { .. }));
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.drain_commands()),
            (0, 0)
        );
    }
    assert_eq!(cancelled, 2058);
    assert!(engine
        .tracks
        .iter()
        .all(|track| track.queued_mute.is_none()));
    assert!(!engine.pending_source_cleanup);
    commands.push(EngineCommand::Play).unwrap();
    engine.drain_commands();
    assert!(engine.transport.is_playing());
}

#[test]
fn normal_stop_does_not_overtake_a_retained_applied_mute() {
    let (mut engine, _, mut events) = AudioEngine::new();
    let id = TrackId::new();
    engine.tracks.push(EngineTrack::new(id));
    engine.transport.play();
    full_ring(&mut engine);
    engine.set_track_mute(id, true);
    engine.command_stop();
    while events.pop().is_ok() {}
    engine.flush_presentation();
    let state: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::TrackMuteChanged { .. } => Some("mute"),
            EngineEvent::PlaybackStopped => Some("stop"),
            _ => None,
        })
        .collect();
    assert_eq!(state, ["mute", "stop"]);
}

#[test]
fn accepted_capture_start_and_notes_survive_a_stalled_ui_until_capture_closes() {
    for global_stop in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let id = TrackId::new();
        engine.tracks.push(EngineTrack::new(id));
        engine.begin_clip_performance();
        engine.process(&mut [0.0; 16], 1);
        while events.pop().is_ok() {}
        full_ring(&mut engine);
        commands
            .push(EngineCommand::StartPerformanceCapture)
            .unwrap();
        commands
            .push(EngineCommand::ExternalNoteOn {
                track_id: id,
                pitch: 42,
                velocity: 100,
            })
            .unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 8], 1)),
            (0, 0)
        );
        if global_stop {
            commands.push(EngineCommand::Stop).unwrap();
        } else {
            local_recovery(&mut engine);
            commands
                .push(EngineCommand::StartPerformanceCapture)
                .unwrap();
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 8], 1)),
            (0, 0)
        );
        while events.pop().is_ok() {}
        engine.flush_presentation();
        let mut phase = "Starting";
        let mut captured = 0;
        let mut order = Vec::new();
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::PerformanceCaptureStarted { .. } => {
                    phase = "Recording";
                    order.push("start");
                }
                EngineEvent::InstrumentNoteInput { .. } => {
                    if phase == "Recording" {
                        captured += 1;
                    }
                    order.push("note");
                }
                EngineEvent::PerformanceCaptureStopped { .. } => {
                    phase = "Idle";
                    order.push("stop");
                }
                _ => {}
            }
        }
        assert_eq!(
            order,
            ["start", "note", "stop"],
            "global stop={global_stop}"
        );
        assert_eq!(captured, 1);
        assert_eq!(phase, "Idle");
    }
}
