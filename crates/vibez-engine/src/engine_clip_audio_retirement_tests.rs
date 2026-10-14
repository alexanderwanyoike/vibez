//! Clip audio replacements preserve geometry and return owners without callback destruction.

use super::*;
use std::sync::Weak;
use vibez_core::id::ClipId;
use vibez_core::track::{ClipFades, ClipPlaybackDirection};
use vibez_core::warp_marker::WarpMarkers;

#[derive(Clone, Copy, Debug)]
enum Replacement {
    Geometry,
    BufferOnly,
}

impl Replacement {
    fn command(
        self,
        track_id: TrackId,
        clip_id: ClipId,
        audio: Arc<DecodedAudio>,
    ) -> EngineCommand {
        match self {
            Self::Geometry => EngineCommand::ReplaceClipAudio {
                track_id,
                clip_id,
                audio,
                duration: 28,
                source_offset: 4,
                start_marker: 6,
                loop_start: 7,
                loop_end: 27,
            },
            Self::BufferOnly => EngineCommand::ReplaceClipBuffer {
                track_id,
                clip_id,
                audio,
            },
        }
    }
}

fn audio(value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; 128]],
        sample_rate: 44_100,
    })
}

struct Fixture {
    engine: AudioEngine,
    commands: rtrb::Producer<EngineCommand>,
    events: rtrb::Consumer<EngineEvent>,
    track: TrackId,
    clip: ClipId,
    original: Weak<DecodedAudio>,
}

impl Fixture {
    fn new() -> Self {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let clip = ClipId::new();
        let old_audio = audio(0.25);
        let original = Arc::downgrade(&old_audio);
        let mut warp_markers = WarpMarkers::default();
        assert!(warp_markers.add(75, 50, 3, 120, 100));
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::AddClip {
                track_id: track,
                clip_id: clip,
                audio: old_audio,
                position: 64,
                source_offset: 3,
                start_marker: 5,
                duration: 100,
                loop_enabled: true,
                loop_start: 7,
                loop_end: 91,
                linear_gain: 0.75,
                fades: ClipFades::new(10, 20, 100),
                playback_direction: ClipPlaybackDirection::Reverse,
                warp_markers,
            })
            .unwrap();
        engine.process(&mut [0.0; 32], 2);
        while events.pop().is_ok() {}
        Self {
            engine,
            commands,
            events,
            track,
            clip,
            original,
        }
    }

    fn fill_event_ring(&mut self) {
        while self
            .engine
            .event_tx
            .push(EngineEvent::PlaybackStarted)
            .is_ok()
        {}
    }

    fn release_retired_owners(&mut self) {
        while self.events.pop().is_ok() {}
        assert_eq!(
            crate::retirement::tests::allocations(|| self.engine.flush_retirements()),
            (0, 0)
        );
        while self.events.pop().is_ok() {}
    }
}

#[test]
fn replacement_returns_unique_old_audio_and_preserves_each_commands_geometry() {
    for replacement in [Replacement::Geometry, Replacement::BufferOnly] {
        for full_ring in [false, true] {
            let mut fixture = Fixture::new();
            if full_ring {
                fixture.fill_event_ring();
            }
            let old_clip = &fixture.engine.tracks()[0].playback_source.clips[0];
            let fades = old_clip.fades;
            let markers = old_clip.warp_markers.clone();
            let marker_storage = old_clip.warp_markers.as_slice().as_ptr();
            let incoming = audio(0.75);
            let incoming_owner = Arc::downgrade(&incoming);
            fixture
                .commands
                .push(replacement.command(fixture.track, fixture.clip, incoming))
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
                (0, 0),
                "{replacement:?}, full_ring={full_ring}"
            );
            assert_eq!(fixture.original.strong_count(), 1);
            assert_eq!(incoming_owner.strong_count(), 1);
            let clip = &fixture.engine.tracks()[0].playback_source.clips[0];
            assert_eq!(Arc::as_ptr(&clip.audio), incoming_owner.as_ptr());
            assert_eq!(clip.id, fixture.clip);
            assert_eq!(clip.position, 64);
            assert_eq!(clip.linear_gain, 0.75);
            assert!(clip.loop_enabled);
            assert_eq!(clip.playback_direction, ClipPlaybackDirection::Reverse);
            assert_eq!(clip.warp_markers, markers);
            assert_eq!(clip.warp_markers.as_slice().as_ptr(), marker_storage);
            match replacement {
                Replacement::Geometry => {
                    assert_eq!(
                        (
                            clip.duration,
                            clip.source_offset,
                            clip.start_marker,
                            clip.loop_start,
                            clip.loop_end
                        ),
                        (28, 4, 6, 7, 27)
                    );
                    assert_eq!(clip.fades, fades.clamped_to(28));
                    assert_eq!(fixture.engine.transport().audio_length(), Some(92));
                }
                Replacement::BufferOnly => {
                    assert_eq!(
                        (
                            clip.duration,
                            clip.source_offset,
                            clip.start_marker,
                            clip.loop_start,
                            clip.loop_end
                        ),
                        (100, 3, 5, 7, 91)
                    );
                    assert_eq!(clip.fades, fades);
                    assert_eq!(fixture.engine.transport().audio_length(), Some(164));
                }
            }
            assert_eq!(
                fixture.engine.pending_retirements.len(),
                usize::from(full_ring)
            );
            fixture.release_retired_owners();
            assert_eq!(fixture.original.strong_count(), 0);
            assert_eq!(incoming_owner.strong_count(), 1);
        }
    }
}

#[test]
fn missing_clip_or_track_returns_unique_incoming_audio_without_callback_destruction() {
    for replacement in [Replacement::Geometry, Replacement::BufferOnly] {
        for missing_track in [false, true] {
            for full_ring in [false, true] {
                let mut fixture = Fixture::new();
                if full_ring {
                    fixture.fill_event_ring();
                }
                let incoming = audio(0.75);
                let incoming_owner = Arc::downgrade(&incoming);
                let track = if missing_track {
                    TrackId::new()
                } else {
                    fixture.track
                };
                fixture
                    .commands
                    .push(replacement.command(track, ClipId::new(), incoming))
                    .unwrap();
                assert_eq!(
                    crate::retirement::tests::allocations(|| fixture
                        .engine
                        .process(&mut [0.0; 32], 2)),
                    (0, 0),
                    "{replacement:?}, missing_track={missing_track}, full_ring={full_ring}"
                );
                assert_eq!(incoming_owner.strong_count(), 1);
                assert_eq!(fixture.original.strong_count(), 1);
                assert_eq!(
                    Arc::as_ptr(&fixture.engine.tracks()[0].playback_source.clips[0].audio),
                    fixture.original.as_ptr()
                );
                assert_eq!(fixture.engine.transport().audio_length(), Some(164));
                fixture.release_retired_owners();
                assert_eq!(incoming_owner.strong_count(), 0);
                assert_eq!(fixture.original.strong_count(), 1);
            }
        }
    }
}

#[test]
fn audio_replacement_retirement_saturation_retains_and_retries_queued_owners() {
    for replacement in [Replacement::Geometry, Replacement::BufferOnly] {
        let mut fixture = Fixture::new();
        fixture.fill_event_ring();
        let count = crate::retirement::RETIREMENT_CAPACITY + 1;
        let mut owners = vec![fixture.original.clone()];
        for index in 0..count {
            let incoming = audio(index as f32 / count as f32);
            owners.push(Arc::downgrade(&incoming));
            fixture
                .commands
                .push(replacement.command(fixture.track, fixture.clip, incoming))
                .unwrap();
        }
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        let retired = fixture.engine.pending_retirements.len();
        let queued = fixture.engine.cmd_rx.slots();
        assert!(retired > 0 && retired <= crate::retirement::RETIREMENT_CAPACITY);
        assert!(queued > 0);
        assert_eq!(retired + queued, count);
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        assert_eq!(
            Arc::as_ptr(&fixture.engine.tracks()[0].playback_source.clips[0].audio),
            owners[retired].as_ptr()
        );
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        assert_eq!(fixture.engine.cmd_rx.slots(), queued);
        while fixture.events.pop().is_ok() {}
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        assert_eq!(fixture.engine.cmd_rx.slots(), 0);
        assert!(fixture.engine.pending_retirements.is_empty());
        assert_eq!(
            Arc::as_ptr(&fixture.engine.tracks()[0].playback_source.clips[0].audio),
            owners[count].as_ptr()
        );
        fixture.release_retired_owners();
        assert!(owners[..count]
            .iter()
            .all(|owner| owner.strong_count() == 0));
        assert_eq!(owners[count].strong_count(), 1);
    }
}

#[test]
fn replacement_renders_the_incoming_samples_through_the_playing_channel_strip() {
    for replacement in [Replacement::Geometry, Replacement::BufferOnly] {
        let mut fixture = Fixture::new();
        let clip = &mut fixture.engine.tracks_mut_for_retirement()[0]
            .playback_source
            .clips[0];
        clip.position = 0;
        clip.warp_markers = WarpMarkers::default();
        clip.playback_direction = ClipPlaybackDirection::Forward;
        clip.fades = ClipFades::default();
        fixture.engine.recalculate_audio_length();
        fixture.commands.push(EngineCommand::Play).unwrap();
        fixture
            .commands
            .push(replacement.command(fixture.track, fixture.clip, audio(0.75)))
            .unwrap();
        let mut output = [0.0; 32];
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut output, 2)),
            (0, 0)
        );
        let expected = 0.75 * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
    }
}

#[test]
fn returned_audio_events_keep_the_exact_owner_and_untimed_identity_semantics() {
    for replacement in [Replacement::Geometry, Replacement::BufferOnly] {
        for valid_target in [false, true] {
            let mut fixture = Fixture::new();
            let incoming = audio(0.75);
            let expected = if valid_target {
                fixture.original.as_ptr()
            } else {
                Arc::as_ptr(&incoming)
            };
            let target = if valid_target {
                fixture.clip
            } else {
                ClipId::new()
            };
            fixture
                .commands
                .push(replacement.command(fixture.track, target, incoming))
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
                (0, 0)
            );
            let owner = std::iter::from_fn(|| fixture.events.pop().ok())
                .find_map(|event| match event {
                    EngineEvent::RetiredAudioBuffer(owner) => Some(owner),
                    _ => None,
                })
                .expect("the replacement returns its audio owner");
            assert_eq!(Arc::as_ptr(&owner), expected);
            let event = EngineEvent::RetiredAudioBuffer(Arc::clone(&owner));
            assert_eq!(event.time_domain(), crate::events::TimeDomain::Untimed);
            assert_eq!(event, EngineEvent::RetiredAudioBuffer(owner));
            assert_ne!(event, EngineEvent::RetiredAudioBuffer(audio(0.75)));
        }
    }
}
