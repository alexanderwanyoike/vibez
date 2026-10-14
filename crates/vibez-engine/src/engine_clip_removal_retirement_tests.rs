//! Complete clip removal preserves playback and returns owned storage outside the callback.

use super::*;
use vibez_core::id::ClipId;
use vibez_core::track::{ClipFades, ClipPlaybackDirection};
use vibez_core::warp_marker::WarpMarkers;

fn audio(value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; 512]],
        sample_rate: 44_100,
    })
}

fn add_clip(track: TrackId, id: ClipId, audio: Arc<DecodedAudio>, duration: u64) -> EngineCommand {
    let mut warp_markers = WarpMarkers::default();
    assert!(warp_markers.add(32, 32, 0, 512, 512));
    EngineCommand::AddClip {
        track_id: track,
        clip_id: id,
        audio,
        position: 0,
        source_offset: 4,
        start_marker: 7,
        duration,
        loop_enabled: true,
        loop_start: 8,
        loop_end: 100,
        linear_gain: 0.75,
        fades: ClipFades::default(),
        playback_direction: ClipPlaybackDirection::Reverse,
        warp_markers,
    }
}

#[test]
fn removing_a_unique_audio_clip_preserves_survivors_without_callback_destruction() {
    for full_ring in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let ids = [ClipId::new(), ClipId::new(), ClipId::new()];
        let removed_audio = audio(0.25);
        let removed_owner = Arc::downgrade(&removed_audio);
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands.push(add_clip(track, ids[0], audio(0.125), 128)).unwrap();
        commands.push(add_clip(track, ids[1], removed_audio, 300)).unwrap();
        commands.push(add_clip(track, ids[2], audio(0.5), 200)).unwrap();
        commands.push(EngineCommand::Play).unwrap();
        let mut before = [0.0; 16];
        engine.process(&mut before, 2);
        let expected_before = (0.125 + 0.25 + 0.5) * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(before.iter().all(|sample| (*sample - expected_before).abs() < 1e-6));
        assert_eq!(engine.transport().audio_length(), Some(300));
        let survivor_geometry: Vec<_> = engine.tracks()[0].playback_source.clips.iter()
            .filter(|clip| clip.id != ids[1])
            .map(|clip| (
                clip.id, Arc::as_ptr(&clip.audio), clip.position, clip.source_offset,
                clip.start_marker, clip.duration, clip.loop_enabled, clip.loop_start,
                clip.loop_end, clip.linear_gain, clip.fades, clip.playback_direction,
                clip.warp_markers.as_slice().as_ptr(), clip.warp_markers.clone(),
            )).collect();
        while events.pop().is_ok() {}
        if full_ring {
            while engine.event_tx.push(EngineEvent::PlaybackStarted).is_ok() {}
        }
        commands.push(EngineCommand::RemoveClip(track, ids[1])).unwrap();
        let mut after = [0.0; 16];
        let allocation = crate::retirement::tests::allocations(|| engine.process(&mut after, 2));
        let expected_after = (0.125 + 0.5) * 0.75 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(after.iter().all(|sample| (*sample - expected_after).abs() < 1e-6));
        assert_eq!(engine.transport().audio_length(), Some(200));
        let survivors = &engine.tracks()[0].playback_source.clips;
        assert_eq!(survivors.len(), 2);
        for (clip, expected) in survivors.iter().zip(&survivor_geometry) {
            assert_eq!((clip.id, Arc::as_ptr(&clip.audio), clip.position, clip.source_offset,
                clip.start_marker, clip.duration, clip.loop_enabled, clip.loop_start,
                clip.loop_end, clip.linear_gain, clip.fades, clip.playback_direction,
                clip.warp_markers.as_slice().as_ptr(), &clip.warp_markers),
                (expected.0, expected.1, expected.2, expected.3, expected.4, expected.5,
                expected.6, expected.7, expected.8, expected.9, expected.10, expected.11,
                expected.12, &expected.13));
        }
        assert_eq!(allocation, (0, 0), "full_ring={full_ring}");
        assert_eq!(removed_owner.strong_count(), 1);
        while events.pop().is_ok() {}
        assert_eq!(crate::retirement::tests::allocations(|| engine.flush_retirements()), (0, 0));
        while events.pop().is_ok() {}
        assert_eq!(removed_owner.strong_count(), 0);
    }
}
