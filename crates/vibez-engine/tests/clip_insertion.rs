//! A public native event consumer serves unlimited clip growth and preserves audible FIFO edits.

use std::sync::{Arc, Weak};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::routing::RoutingChannel;
use vibez_core::warp_marker::WarpMarkers;
use vibez_engine::clip_insertion::{ClipInsertionId, ClipInsertionOutcome, PreparedClipInsertion};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};
use vibez_engine::events::EngineEvent;
use vibez_engine::routing::PreparedRouting;

fn add(
    track: TrackId,
    clip: ClipId,
    audio: Arc<DecodedAudio>,
    position: u64,
    duration: u64,
    looping: bool,
    markers: WarpMarkers,
) -> (EngineCommand, ClipInsertionId) {
    let insertion = PreparedClipInsertion::new();
    let id = insertion.id();
    (EngineCommand::AddClip {
        insertion, track_id: track, clip_id: clip, audio, position,
        source_offset: 0, start_marker: 0, duration, loop_enabled: looping,
        loop_start: 0, loop_end: if looping { 8 } else { 0 }, linear_gain: 1.0,
        fades: Default::default(), playback_direction: Default::default(), warp_markers: markers,
    }, id)
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

struct Expected {
    clip: ClipId,
    operation: ClipInsertionId,
    audio: Weak<DecodedAudio>,
    markers: WarpMarkers,
    value: f32,
}

#[test]
fn native_consumer_fulfills_clip_growth_requests_across_capacity_boundaries() {
    for count in [1, 2, 20, 21, 128, 129] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands.push(EngineCommand::SetTrackPan(track, 0.0)).unwrap();
        let resident = Arc::new(DecodedAudio {
            channels: vec![vec![0.25; 4096]], sample_rate: 44_100,
        });
        let resident_owner = Arc::downgrade(&resident);
        let resident_id = ClipId::new();
        let (seed, _) = add(track, resident_id, resident, 0, 4096, false, Default::default());
        commands.push(seed).unwrap();
        let model: Vec<_> = [track, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
            id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
        }).collect();
        commands.push(EngineCommand::SetRouting(PreparedRouting::prepare(&model, 8).unwrap())).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        assert_signal(&render(&mut engine, true), 0.25);
        while events.pop().is_ok() {}
        assert_eq!(engine.tracks()[0].playback_source.clips.len(), engine.tracks()[0].playback_source.clips.capacity());
        let mut expected = Vec::new();
        for index in 0..count {
            let value = (index + 1) as f32 / 256.0;
            let audio = Arc::new(DecodedAudio { channels: vec![vec![value; 8]], sample_rate: 44_100 });
            let weak = Arc::downgrade(&audio);
            let clip = ClipId::new();
            let mut markers = WarpMarkers::default();
            assert!(markers.add(2, 4, 0, 8, 8));
            let (command, operation) = add(track, clip, audio, 4096 + index as u64 * 64,
                32, true, markers.clone());
            commands.push(command).unwrap();
            expected.push(Expected { clip, operation, audio: weak, markers, value });
        }
        commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        commands.push(EngineCommand::Stop).unwrap();
        render(&mut engine, false);
        assert!(matches!(events.peek().unwrap(), EngineEvent::ClipInsertionStorageRequested(_)));
        assert_signal(&render(&mut engine, true), 0.25);
        assert!(engine.transport().is_playing());
        assert_eq!(engine.transport().position(), 16);
        assert_eq!(engine.transport().bpm(), 120.0);
        let mut applied = vec![false; count];
        let mut requests = 0;
        // Each delta can require at most one request and one completion.
        // This bound follows from the protocol, rather than a polling timeout.
        for _ in 0..=count {
            render(&mut engine, false);
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::ClipInsertionStorageRequested(request) => {
                        assert_eq!(request.track_id, track);
                        let item = expected.iter().find(|item| item.operation == request.id).unwrap();
                        assert_eq!(request.clip_id, item.clip);
                        request.fulfill();
                        requests += 1;
                    }
                    EngineEvent::ClipInsertionCompleted(completed) => {
                        assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
                        assert_eq!(completed.track_id, track);
                        let index = expected.iter().position(|item| item.operation == completed.id()).unwrap();
                        assert_eq!(completed.clip_id, expected[index].clip);
                        assert!(!applied[index]);
                        applied[index] = true;
                        drop(completed);
                    }
                    _ => {}
                }
            }
            if applied.iter().all(|done| *done) { break; }
        }
        assert!(applied.iter().all(|done| *done), "native consumer stalled at {count} clips");
        assert!(requests > 0);
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().position(), 16);
        assert_eq!(engine.transport().bpm(), 137.0);
        assert_eq!(engine.transport().audio_length(), Some(4096 + (count as u64 - 1) * 64 + 32));
        let clips = &engine.tracks()[0].playback_source.clips;
        assert_eq!(clips.len(), count + 1);
        assert_eq!(clips[0].id, resident_id);
        assert_eq!(Arc::as_ptr(&clips[0].audio), resident_owner.as_ptr());
        for (index, item) in expected.iter().enumerate() {
            let clip = &clips[index + 1];
            assert_eq!(clip.id, item.clip);
            assert_eq!(Arc::as_ptr(&clip.audio), item.audio.as_ptr());
            assert_eq!(item.audio.strong_count(), 1);
            assert_eq!((clip.position, clip.source_offset, clip.start_marker, clip.duration),
                (4096 + index as u64 * 64, 0, 0, 32));
            assert!(clip.loop_enabled);
            assert_eq!((clip.loop_start, clip.loop_end), (0, 8));
            assert_eq!(clip.warp_markers, item.markers);
        }
        for (index, item) in expected.iter().enumerate() {
            commands.push(EngineCommand::Seek(4096 + index as u64 * 64 + 4)).unwrap();
            commands.push(EngineCommand::Play).unwrap();
            assert_signal(&render(&mut engine, true), item.value);
            while events.pop().is_ok() {}
        }
        drop(engine);
        assert_eq!(resident_owner.strong_count(), 0);
        assert!(expected.iter().all(|item| item.audio.strong_count() == 0));
    }
}
