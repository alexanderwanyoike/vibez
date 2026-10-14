//! Pending removed clip owners survive real Section and Clip boundary source transfers.

use super::*;
use crate::playback_source::{PreparedClipPlayback, PreparedPlaybackSource};
use std::sync::Weak;
use vibez_core::{
    id::ClipId,
    perform::{MusicalBoundary, SectionLaunchQuantization},
};

fn source(
    id: ClipId,
    count: usize,
    value: f32,
) -> (PreparedPlaybackSource, Vec<Weak<DecodedAudio>>) {
    let mut owners = Vec::new();
    let clips = (0..count)
        .map(|_| {
            let audio = Arc::new(DecodedAudio {
                channels: vec![vec![value; 128]],
                sample_rate: 8,
            });
            owners.push(Arc::downgrade(&audio));
            let mut warp_markers = vibez_core::warp_marker::WarpMarkers::default();
            assert!(warp_markers.add(32, 32, 0, 128, 128));
            EngineClip {
                id,
                audio,
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: 128,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 128,
                linear_gain: 1.0,
                fades: Default::default(),
                playback_direction: Default::default(),
                warp_markers,
            }
        })
        .collect();
    (PreparedPlaybackSource::new(clips, vec![], vec![]), owners)
}

fn collect_staged(source: &mut PreparedPlaybackSource, returned: &mut Vec<*const DecodedAudio>) {
    while let Some(clip) = source.pop_removed_audio_clip() {
        returned.push(Arc::as_ptr(&clip.audio));
    }
}

#[test]
fn autonomous_section_and_clip_boundaries_transfer_staged_owners_once_under_full_ring() {
    for clip_mode in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let removed_id = ClipId::new();
        let new_id = ClipId::new();
        let count = crate::retirement::RETIREMENT_CAPACITY * 2 + 3;
        let (old, owners) = source(removed_id, count, 0.001);
        commands.push(EngineCommand::SetSampleRate(8)).unwrap();
        commands.push(EngineCommand::SetBpm(60.0)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        let section_id = SectionId::new();
        if clip_mode {
            commands
                .push(EngineCommand::QueueClips {
                    clips: vec![Box::new(PreparedClipPlayback {
                        track_id: track,
                        clip_id: Some(removed_id),
                        request_id: 1,
                        length_samples: 64,
                        looping: true,
                        source: Box::new(old),
                    })],
                    quantization: MusicalBoundary::Immediate,
                })
                .unwrap();
        } else {
            commands
                .push(EngineCommand::LaunchSection(Box::new(
                    PreparedSectionPlaybackSource::new(section_id, 8.0, true, vec![(track, old)]),
                )))
                .unwrap();
        }
        engine.process(&mut [0.0; 1], 1);
        while events.pop().is_ok() {}
        let (new, _) = source(new_id, 1, 0.25);
        if clip_mode {
            commands
                .push(EngineCommand::QueueClips {
                    clips: vec![Box::new(PreparedClipPlayback {
                        track_id: track,
                        clip_id: Some(new_id),
                        request_id: 2,
                        length_samples: 64,
                        looping: true,
                        source: Box::new(new),
                    })],
                    quantization: MusicalBoundary::OneBeat,
                })
                .unwrap();
        } else {
            commands
                .push(EngineCommand::QueueSection {
                    prepared: Box::new(PreparedSectionPlaybackSource::new(
                        SectionId::new(),
                        8.0,
                        true,
                        vec![(track, new)],
                    )),
                    quantization: SectionLaunchQuantization::OneBeat,
                })
                .unwrap();
        }
        engine.process(&mut [], 1);
        while events.pop().is_ok() {}
        // Isolate the source-transfer boundary after the independently tested removal transaction.
        let channel = &mut engine.tracks_mut_for_retirement()[0];
        let resident = if clip_mode {
            &mut channel.launcher_source
        } else {
            &mut channel.section_playback_source
        };
        resident.remove_audio_clips(removed_id);
        while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        let mut output = [0.0; 16];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process(&mut output, 1)),
            (0, 0),
            "clip_mode={clip_mode}"
        );
        assert_eq!(&output[..7], &[0.0; 7]);
        assert!(
            output[7..]
                .iter()
                .all(|sample| (*sample - 0.25).abs() < 1e-6),
            "clip_mode={clip_mode}: {output:?}"
        );
        assert_eq!(
            engine.pending_retirements.len(),
            crate::retirement::RETIREMENT_CAPACITY
        );
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        let mut returned = Vec::new();
        let mut transferred = 0;
        for _ in 0..4 {
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::RetiredAudioClip(clip) => returned.push(Arc::as_ptr(&clip.audio)),
                    EngineEvent::SectionTransitioned { mut retired, .. } => {
                        let before = returned.len();
                        for source in retired.tracks_mut() {
                            collect_staged(&mut source.source, &mut returned);
                        }
                        transferred += returned.len() - before;
                    }
                    EngineEvent::ClipTransitioned {
                        retired: Some(mut retired),
                        ..
                    } => {
                        let before = returned.len();
                        collect_staged(&mut retired.source, &mut returned);
                        transferred += returned.len() - before;
                    }
                    EngineEvent::ClipBatchRetired(batch) => {
                        let before = returned.len();
                        for mut retired in batch {
                            collect_staged(&mut retired.source, &mut returned);
                        }
                        transferred += returned.len() - before;
                    }
                    _ => {}
                }
            }
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
                (0, 0)
            );
        }
        assert_eq!(transferred, count - crate::retirement::RETIREMENT_CAPACITY);
        assert_eq!(returned.len(), count);
        for owner in owners {
            assert_eq!(owner.strong_count(), 0);
            assert_eq!(
                returned
                    .iter()
                    .filter(|pointer| **pointer == owner.as_ptr())
                    .count(),
                1
            );
        }
    }
}

#[test]
fn rejected_prepared_sources_and_offline_teardown_keep_staged_audio_ownership_complete() {
    for clip_mode in [false, true] {
        for full_ring in [false, true] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let id = ClipId::new();
            let track = TrackId::new();
            let (mut staged, owners) = source(id, 35, 0.001);
            staged.remove_audio_clips(id);
            if clip_mode {
                commands
                    .push(EngineCommand::RefreshClip(Box::new(PreparedClipPlayback {
                        track_id: track,
                        clip_id: Some(id),
                        request_id: 1,
                        length_samples: 128,
                        looping: true,
                        source: Box::new(staged),
                    })))
                    .unwrap();
            } else {
                commands
                    .push(EngineCommand::RefreshSection(Box::new(
                        PreparedSectionPlaybackSource::new(
                            SectionId::new(),
                            8.0,
                            true,
                            vec![(track, staged)],
                        ),
                    )))
                    .unwrap();
            }
            if full_ring {
                while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
            }
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
                (0, 0)
            );
            assert!(owners.iter().all(|owner| owner.strong_count() == 1));
            let mut returned = Vec::new();
            let mut source_events = 0;
            for _ in 0..3 {
                while let Ok(event) = events.pop() {
                    match event {
                        EngineEvent::ClipRequestRetired(mut owner) => {
                            source_events += 1;
                            collect_staged(&mut owner.source, &mut returned);
                        }
                        EngineEvent::SectionSourceRefreshed {
                            applied,
                            mut retired,
                            ..
                        } => {
                            assert!(!applied);
                            source_events += 1;
                            for source in retired.tracks_mut() {
                                collect_staged(&mut source.source, &mut returned);
                            }
                        }
                        EngineEvent::RetiredAudioClip(_) => {
                            panic!("rejected source keeps its entire staged owner")
                        }
                        _ => {}
                    }
                }
                assert_eq!(
                    crate::retirement::tests::allocations(|| engine.process(&mut [], 1)),
                    (0, 0)
                );
            }
            assert_eq!(source_events, 1);
            assert_eq!(returned.len(), 35);
            for owner in owners {
                assert_eq!(owner.strong_count(), 0);
                assert_eq!(
                    returned
                        .iter()
                        .filter(|pointer| **pointer == owner.as_ptr())
                        .count(),
                    1
                );
            }
        }
    }
    let id = ClipId::new();
    let (mut staged, owners) = source(id, 35, 0.001);
    staged.remove_audio_clips(id);
    assert!(owners.iter().all(|owner| owner.strong_count() == 1));
    drop(staged);
    assert!(owners.iter().all(|owner| owner.strong_count() == 0));
}
