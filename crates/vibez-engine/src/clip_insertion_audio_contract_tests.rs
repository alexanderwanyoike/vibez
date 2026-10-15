//! Prepared and live clip collections retain matching storage across real capacity boundaries.

use super::*;
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock, DEFAULT_SAMPLE_RATE};
use crate::events::EngineEvent;
use crate::mixer::EngineTrack;
use std::sync::Weak;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::routing::RoutingChannel;
use vibez_core::warp_marker::WarpMarkers;

fn callback(engine: &mut AudioEngine, sounding: bool) -> [f32; 16] {
    let mut output = [0.0; 16];
    let length = if sounding { output.len() } else { 0 };
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output[..length], 2));
    }), (0, 0));
    output
}

fn assert_signal(output: &[f32], value: f32) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert!((left - value).abs() < 1e-7, "expected {value}, actual {left}");
        assert_eq!(right, 0.0);
    }
}

struct Delta {
    command: Option<EngineCommand>,
    id: ClipInsertionId,
    clip: ClipId,
    audio: Weak<DecodedAudio>,
    markers: *const vibez_core::warp_marker::WarpMarker,
}

fn delta(track: TrackId) -> Delta {
    let insertion = PreparedClipInsertion::new();
    let id = insertion.id();
    let clip = ClipId::new();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.0625; 4096]], sample_rate: DEFAULT_SAMPLE_RATE,
    });
    let weak = Arc::downgrade(&audio);
    let mut warp_markers = WarpMarkers::default();
    assert!(warp_markers.add(32, 64, 0, 128, 128));
    let markers = warp_markers.as_slice().as_ptr();
    let command = EngineCommand::AddClip {
        insertion, track_id: track, clip_id: clip, audio, position: 0,
        source_offset: 0, start_marker: 0, duration: 4096,
        loop_enabled: true, loop_start: 0, loop_end: 128, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(), warp_markers,
    };
    Delta { command: Some(command), id, clip, audio: weak, markers }
}

struct Resident {
    id: ClipId,
    audio: Weak<DecodedAudio>,
    markers: WarpMarkers,
    marker_allocation: *const vibez_core::warp_marker::WarpMarker,
}

#[test]
fn prepared_source_and_live_growth_preserve_matching_storage_at_content_boundaries() {
    for count in [1, 2, 20, 21, 128, 129, 257] {
        let mut expected = Vec::new();
        let clips: Vec<_> = (0..count)
            .map(|index| {
                let audio = Arc::new(DecodedAudio {
                    channels: vec![vec![0.25; 16]],
                    sample_rate: DEFAULT_SAMPLE_RATE,
                });
                let id = ClipId::new();
                let mut markers = WarpMarkers::default();
                assert!(markers.add(2, 4, 0, 8, 8));
                expected.push(Resident {
                    id,
                    audio: Arc::downgrade(&audio),
                    markers: markers.clone(),
                    marker_allocation: markers.as_slice().as_ptr(),
                });
                EngineClip {
                    id,
                    audio,
                    position: if index == 0 {
                        0
                    } else {
                        4096 + index as u64 * 64
                    },
                    source_offset: 0,
                    start_marker: 0,
                    duration: if index == 0 { 4096 } else { 32 },
                    loop_enabled: true,
                    loop_start: 0,
                    loop_end: 8,
                    linear_gain: 1.0,
                    fades: Default::default(),
                    playback_direction: Default::default(),
                    warp_markers: markers,
                }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
            .into_vec();
        // A boxed slice gives an actually full allocation without relying on Vec's growth factor.
        assert_eq!(clips.capacity(), count);
        let live_allocation = clips.as_ptr();
        let source = PreparedPlaybackSource::new(clips, Vec::new(), Vec::new());
        assert_eq!(source.clips.as_ptr(), live_allocation);
        assert_eq!(source.clips.len(), count);
        assert!(source.removed_audio_clips.is_empty());
        assert!(source.removed_audio_clips.capacity() >= count);
        let staging_allocation = source.removed_audio_clips.as_ptr();
        let staging_capacity = source.removed_audio_clips.capacity();
        let track = TrackId::new();
        let mut channel = EngineTrack::with_playback_source(track, source);
        channel.pan = 0.0;
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        commands
            .push(EngineCommand::AddTrack(Box::new(channel)))
            .unwrap();
        let model: Vec<_> = [track, TrackId::MASTER]
            .into_iter()
            .map(|id| RoutingChannel {
                id,
                is_bus: id.is_master(),
                sends: Vec::new(),
                effects: Vec::new(),
            })
            .collect();
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare(&model, 8).unwrap(),
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut warmup = [0.0; 16];
        engine.process_block(AudioProcessBlock::new(&mut warmup, 2));
        assert_signal(&warmup, 0.25);
        while events.pop().is_ok() {}
        let mut incoming = delta(track);
        commands.push(incoming.command.take().unwrap()).unwrap();
        callback(&mut engine, false);
        let source = &engine.tracks()[0].playback_source;
        assert_eq!(source.clips.len(), count);
        assert_eq!(source.clips.as_ptr(), live_allocation);
        assert_eq!(source.removed_audio_clips.as_ptr(), staging_allocation);
        let EngineEvent::ClipInsertionStorageRequested(request) = events.pop().unwrap() else {
            panic!("full prepared source must request real growth");
        };
        assert_eq!(request.id, incoming.id);
        // The zero-frame callback still publishes its position and silent master meters.
        assert_eq!(events.pop().unwrap(), EngineEvent::PlaybackPosition(8));
        assert_eq!(
            events.pop().unwrap(),
            EngineEvent::Metering(crate::events::MeteringEvent {
                peak_l: 0.0,
                peak_r: 0.0,
                rms_l: 0.0,
                rms_r: 0.0,
            })
        );
        assert!(events.pop().is_err());
        request.fulfill();
        callback(&mut engine, false);
        let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else {
            panic!("fulfilled growth must return its old storage");
        };
        assert_eq!(completed.id(), incoming.id);
        assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
        let ClipInsertionStorage::Audio(old) = completed.owner.reply.as_ref().unwrap() else {
            panic!("Audio growth returns the original Audio storage");
        };
        assert!(old.live.is_empty());
        assert_eq!(old.live.as_ptr(), live_allocation);
        assert_eq!(old.live.capacity(), count);
        assert!(old.removed.is_empty());
        assert_eq!(old.removed.as_ptr(), staging_allocation);
        assert_eq!(old.removed.capacity(), staging_capacity);
        let source = &engine.tracks()[0].playback_source;
        assert_eq!(source.clips.len(), count + 1);
        assert!(source.clips.capacity() >= source.clips.len());
        assert!(source.removed_audio_clips.capacity() >= source.clips.len());
        assert!(source.removed_audio_clips.is_empty());
        for (index, resident) in expected.iter().enumerate() {
            let clip = &source.clips[index];
            assert_eq!(clip.id, resident.id);
            assert_eq!(Arc::as_ptr(&clip.audio), resident.audio.as_ptr());
            assert_eq!(resident.audio.strong_count(), 1);
            assert_eq!(clip.warp_markers, resident.markers);
            assert_eq!(
                clip.warp_markers.as_slice().as_ptr(),
                resident.marker_allocation
            );
            assert_eq!(
                clip.position,
                if index == 0 {
                    0
                } else {
                    4096 + index as u64 * 64
                }
            );
            assert_eq!(clip.duration, if index == 0 { 4096 } else { 32 });
            assert_eq!((clip.source_offset, clip.start_marker), (0, 0));
            assert!(clip.loop_enabled);
            assert_eq!((clip.loop_start, clip.loop_end), (0, 8));
            assert_eq!(clip.linear_gain, 1.0);
            assert_eq!(clip.fades, Default::default());
            assert_eq!(clip.playback_direction, Default::default());
        }
        let appended = &source.clips[count];
        assert_eq!(appended.id, incoming.clip);
        assert_eq!(Arc::as_ptr(&appended.audio), incoming.audio.as_ptr());
        assert_eq!(appended.warp_markers.as_slice().as_ptr(), incoming.markers);
        assert_eq!(incoming.audio.strong_count(), 1);
        drop(completed);
        assert_signal(&callback(&mut engine, true), 0.3125);
        drop(engine);
        assert!(expected
            .iter()
            .all(|resident| resident.audio.strong_count() == 0));
        assert_eq!(incoming.audio.strong_count(), 0);
    }
}

struct PendingAudio {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    resident: Weak<DecodedAudio>,
    incoming: Delta,
    request: ClipInsertionStorageRequest,
}

fn pending_audio() -> PendingAudio {
    let track = TrackId::new();
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let mut resident = delta(track);
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands.push(resident.command.take().unwrap()).unwrap();
    commands.push(EngineCommand::SetTrackPan(track, 0.0)).unwrap();
    let channels: Vec<_> = [track, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
    }).collect();
    commands.push(EngineCommand::SetRouting(
        crate::routing::PreparedRouting::prepare(&channels, 8).unwrap(),
    )).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    assert_signal(&callback(&mut engine, true), 0.0625);
    while events.pop().is_ok() {}
    let source = &engine.tracks()[0].playback_source;
    assert_eq!((source.clips.len(), source.clips.capacity()), (1, 1));
    let mut incoming = delta(track);
    commands.push(incoming.command.take().unwrap()).unwrap();
    commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    callback(&mut engine, false);
    let EngineEvent::ClipInsertionStorageRequested(request) = events.pop().unwrap() else {
        panic!("exhausted Audio storage must request growth");
    };
    assert_eq!(request.id, incoming.id);
    assert_eq!(request.target, ClipInsertionTarget::Clip(incoming.clip));
    while events.pop().is_ok() {}
    PendingAudio { engine, commands, events, resident: resident.audio, incoming, request }
}

#[test]
fn audio_reply_or_abandonment_preserves_complete_delta_and_waiting_fifo() {
    for applied in [false, true] {
        let PendingAudio { mut engine, commands, mut events, resident, incoming, request } = pending_audio();
        assert_signal(&callback(&mut engine, true), 0.0625);
        assert_eq!(engine.transport().position(), 16);
        assert_eq!(engine.transport().bpm(), 120.0);
        assert_eq!(engine.tracks()[0].playback_source.clips.len(), 1);
        assert_eq!(incoming.audio.strong_count(), 1);
        if applied { request.fulfill(); } else { drop(request); }
        callback(&mut engine, false);
        assert_eq!(engine.transport().bpm(), 137.0);
        let mut returned = Vec::new();
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipInsertionCompleted(completed) = event { returned.push(completed); }
        }
        assert_eq!(returned.len(), 1);
        let complete = &returned[0];
        assert_eq!(complete.id(), incoming.id);
        assert_eq!(complete.target, ClipInsertionTarget::Clip(incoming.clip));
        assert_eq!(complete.outcome, if applied { ClipInsertionOutcome::Applied } else { ClipInsertionOutcome::Abandoned });
        if applied {
            assert!(complete.owner.clip.is_none());
            let clip = &engine.tracks()[0].playback_source.clips[1];
            assert_eq!(clip.id, incoming.clip);
            assert_eq!(Arc::as_ptr(&clip.audio), incoming.audio.as_ptr());
            assert_eq!(clip.warp_markers.as_slice().as_ptr(), incoming.markers);
        } else {
            let Some(ClipInsertionDelta::Audio(clip)) = &complete.owner.clip else {
                panic!("abandoned Audio insertion returns its complete original delta");
            };
            assert_eq!(clip.id, incoming.clip);
            assert_eq!(Arc::as_ptr(&clip.audio), incoming.audio.as_ptr());
            assert_eq!(clip.warp_markers.as_slice().as_ptr(), incoming.markers);
        }
        assert_signal(&callback(&mut engine, true), if applied { 0.125 } else { 0.0625 });
        assert_eq!(engine.transport().position(), 24);
        drop(returned);
        assert_eq!(incoming.audio.strong_count(), usize::from(applied));
        drop(engine);
        drop(commands);
        drop(events);
        assert_eq!(incoming.audio.strong_count(), 0);
        assert_eq!(resident.strong_count(), 0);
    }
}

#[test]
fn audio_request_and_ready_reply_release_complete_owners_in_both_endpoint_orders() {
    for reply_ready in [false, true] {
        for engine_first in [false, true] {
            let PendingAudio { engine, commands, events, resident, incoming, request } = pending_audio();
            let mut request = Some(request);
            if reply_ready { request.take().unwrap().fulfill(); }
            assert_eq!(resident.strong_count(), 1);
            assert_eq!(incoming.audio.strong_count(), 1);
            if engine_first {
                drop(engine);
                assert_eq!(resident.strong_count(), 0);
                assert_eq!(incoming.audio.strong_count(), 0);
                if let Some(request) = request.take() { request.fulfill(); }
                drop(commands);
                drop(events);
            } else {
                drop(request.take());
                drop(commands);
                drop(events);
                assert_eq!(resident.strong_count(), 1);
                assert_eq!(incoming.audio.strong_count(), 1);
                drop(engine);
            }
            assert_eq!(resident.strong_count(), 0);
            assert_eq!(incoming.audio.strong_count(), 0);
        }
    }
}
