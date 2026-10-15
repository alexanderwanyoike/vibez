//! Warp edits preserve clip geometry without destroying marker storage in the callback.

use super::*;
use vibez_core::id::ClipId;
use vibez_core::track::{ClipFades, ClipPlaybackDirection};
use vibez_core::warp_marker::WarpMarkers;

fn markers(source: u64, timeline: u64) -> WarpMarkers {
    let mut markers = WarpMarkers::default();
    assert!(markers.add(source, timeline, 8, 120, 100));
    markers
}

struct Fixture {
    engine: AudioEngine,
    commands: rtrb::Producer<EngineCommand>,
    events: rtrb::Consumer<EngineEvent>,
    track: TrackId,
    clip: ClipId,
}

impl Fixture {
    fn new() -> Self {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let clip = ClipId::new();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::AddClip {
                insertion: crate::clip_insertion::PreparedClipInsertion::new(),
                track_id: track,
                clip_id: clip,
                audio: Arc::new(DecodedAudio {
                    channels: vec![(0..128).map(|frame| frame as f32 / 128.0).collect()],
                    sample_rate: 44_100,
                }),
                position: 64,
                source_offset: 8,
                start_marker: 12,
                duration: 100,
                loop_enabled: false,
                loop_start: 16,
                loop_end: 80,
                linear_gain: 0.75,
                fades: ClipFades::new(10, 20, 100),
                playback_direction: ClipPlaybackDirection::Reverse,
                warp_markers: markers(72, 40),
            })
            .unwrap();
        crate::test_support::fulfill_insertion_setup(&mut engine, &mut events);
        engine.process(&mut [0.0; 32], 2);
        while events.pop().is_ok() {}
        Self {
            engine,
            commands,
            events,
            track,
            clip,
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

    fn command(&self, markers: WarpMarkers) -> EngineCommand {
        EngineCommand::SetClipWarpMarkers {
            track_id: self.track,
            clip_id: self.clip,
            warp_markers: markers,
        }
    }

    fn release_owners(&mut self) {
        while self.events.pop().is_ok() {}
        assert_eq!(
            crate::retirement::tests::allocations(|| self.engine.flush_retirements()),
            (0, 0)
        );
        while self.events.pop().is_ok() {}
    }
}

#[test]
fn nonempty_warp_edits_replace_storage_without_changing_clip_geometry_or_freeing_owners() {
    for full_ring in [false, true] {
        let mut fixture = Fixture::new();
        if full_ring {
            fixture.fill_event_ring();
        }
        let old_clip = &fixture.engine.tracks()[0].playback_source.clips[0];
        let audio_owner = Arc::as_ptr(&old_clip.audio);
        let fades = old_clip.fades;
        let incoming = markers(40, 70);
        let expected = incoming.clone();
        let incoming_storage = incoming.as_slice().as_ptr();
        let command = fixture.command(incoming);
        fixture.commands.push(command).unwrap();
        assert_eq!(
            crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
            (0, 0)
        );
        let clip = &fixture.engine.tracks()[0].playback_source.clips[0];
        assert_eq!(clip.warp_markers, expected);
        assert_eq!(clip.warp_markers.as_slice().as_ptr(), incoming_storage);
        assert_eq!(Arc::as_ptr(&clip.audio), audio_owner);
        assert_eq!(
            (
                clip.id,
                clip.position,
                clip.duration,
                clip.source_offset,
                clip.start_marker
            ),
            (fixture.clip, 64, 100, 8, 12)
        );
        assert_eq!(
            (clip.loop_enabled, clip.loop_start, clip.loop_end),
            (false, 16, 80)
        );
        assert_eq!(clip.linear_gain, 0.75);
        assert_eq!(clip.fades, fades);
        assert_eq!(clip.playback_direction, ClipPlaybackDirection::Reverse);
        assert_eq!(fixture.engine.transport().audio_length(), Some(164));
        assert_eq!(
            fixture.engine.pending_retirements.len(),
            usize::from(full_ring)
        );
        fixture.release_owners();
    }
}

#[test]
fn missing_warp_targets_return_nonempty_incoming_storage_without_callback_destruction() {
    for missing_track in [false, true] {
        for full_ring in [false, true] {
            let mut fixture = Fixture::new();
            if full_ring {
                fixture.fill_event_ring();
            }
            let old = fixture.engine.tracks()[0].playback_source.clips[0]
                .warp_markers
                .clone();
            fixture
                .commands
                .push(EngineCommand::SetClipWarpMarkers {
                    track_id: if missing_track {
                        TrackId::new()
                    } else {
                        fixture.track
                    },
                    clip_id: ClipId::new(),
                    warp_markers: markers(40, 70),
                })
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
                (0, 0)
            );
            assert_eq!(
                fixture.engine.tracks()[0].playback_source.clips[0].warp_markers,
                old
            );
            assert_eq!(
                fixture.engine.pending_retirements.len(),
                usize::from(full_ring)
            );
            fixture.release_owners();
        }
    }
}

#[test]
fn clearing_nonempty_warp_markers_retains_the_displaced_owner() {
    let mut fixture = Fixture::new();
    fixture.fill_event_ring();
    let command = fixture.command(WarpMarkers::default());
    fixture.commands.push(command).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
        (0, 0)
    );
    assert!(fixture.engine.tracks()[0].playback_source.clips[0]
        .warp_markers
        .is_empty());
    assert_eq!(fixture.engine.pending_retirements.len(), 1);
    fixture.release_owners();
}

#[test]
fn saturated_warp_retirements_keep_later_commands_queued_until_ui_delivery_resumes() {
    let mut fixture = Fixture::new();
    fixture.fill_event_ring();
    let count = crate::retirement::RETIREMENT_CAPACITY + 1;
    let mut storage = Vec::new();
    for index in 0..count {
        let incoming = markers(40 + index as u64, 70);
        storage.push(incoming.as_slice().as_ptr());
        let command = fixture.command(incoming);
        fixture.commands.push(command).unwrap();
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
    assert_eq!(
        fixture.engine.tracks()[0].playback_source.clips[0]
            .warp_markers
            .as_slice()
            .as_ptr(),
        storage[retired - 1]
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
    assert!(fixture.engine.pending_retirements.is_empty());
    assert_eq!(fixture.engine.cmd_rx.slots(), 0);
    assert_eq!(
        fixture.engine.tracks()[0].playback_source.clips[0]
            .warp_markers
            .as_slice()
            .as_ptr(),
        storage[count - 1]
    );
    fixture.release_owners();
}

#[test]
fn playing_audio_follows_the_replacement_warp_map_numerically() {
    let mut fixture = Fixture::new();
    let clip = &mut fixture.engine.tracks_mut_for_retirement()[0]
        .playback_source
        .clips[0];
    clip.position = 0;
    clip.start_marker = 8;
    clip.playback_direction = ClipPlaybackDirection::Forward;
    clip.fades = ClipFades::default();
    fixture.engine.recalculate_audio_length();
    let command = fixture.command(markers(40, 70));
    fixture.commands.push(command).unwrap();
    fixture.commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 32];
    assert_eq!(
        crate::retirement::tests::allocations(|| fixture.engine.process(&mut output, 2)),
        (0, 0)
    );
    for (frame, samples) in output.as_chunks::<2>().0.iter().enumerate() {
        let source_frame = 8.0 + frame as f32 * 32.0 / 70.0;
        let expected = source_frame / 128.0 * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(samples
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
    }
}

#[test]
fn untimed_retirement_returns_the_exact_displaced_or_rejected_marker_storage() {
    for valid_target in [false, true] {
        for full_ring in [false, true] {
            let mut fixture = Fixture::new();
            if full_ring {
                fixture.fill_event_ring();
            }
            let old = &fixture.engine.tracks()[0].playback_source.clips[0].warp_markers;
            let incoming = markers(40, 70);
            let expected_storage = if valid_target {
                old.as_slice().as_ptr()
            } else {
                incoming.as_slice().as_ptr()
            };
            let expected_value = if valid_target {
                old.clone()
            } else {
                incoming.clone()
            };
            fixture
                .commands
                .push(EngineCommand::SetClipWarpMarkers {
                    track_id: fixture.track,
                    clip_id: if valid_target {
                        fixture.clip
                    } else {
                        ClipId::new()
                    },
                    warp_markers: incoming,
                })
                .unwrap();
            assert_eq!(
                crate::retirement::tests::allocations(|| fixture.engine.process(&mut [0.0; 32], 2)),
                (0, 0)
            );
            if full_ring {
                while fixture.events.pop().is_ok() {}
                assert_eq!(
                    crate::retirement::tests::allocations(|| fixture.engine.flush_retirements()),
                    (0, 0)
                );
            }
            let returned = std::iter::from_fn(|| fixture.events.pop().ok())
                .find(|event| matches!(event, EngineEvent::RetiredWarpMarkers(_)))
                .expect("the marker owner returns to the UI");
            assert_eq!(returned.time_domain(), crate::events::TimeDomain::Untimed);
            let EngineEvent::RetiredWarpMarkers(owner) = returned else {
                unreachable!()
            };
            assert_eq!(owner.as_slice().as_ptr(), expected_storage);
            assert_eq!(owner, expected_value);
            assert_eq!(
                EngineEvent::RetiredWarpMarkers(owner),
                EngineEvent::RetiredWarpMarkers(expected_value)
            );
        }
    }
}
