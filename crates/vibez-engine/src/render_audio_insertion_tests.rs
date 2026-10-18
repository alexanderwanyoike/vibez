//! Actual offline Audio preparation preserves matching live and removal storage.

use super::*;

#[test]
fn offline_channel_prepares_matching_audio_storage_at_content_boundaries() {
    for count in [1, 2, 20, 21, 128, 129, 257] {
        let track = bare_track("storage boundaries");
        let track_id = track.id;
        let mut request = BounceRequest {
            tracks: vec![track],
            master: None,
            buses: Vec::new(),
            audio_clips: Vec::new(),
            note_clips: Vec::new(),
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, count as u64 * 32),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let mut owners = Vec::new();
        for index in 0..count {
            let id = ClipId::new();
            let audio = audio_of(16, (index + 1) as f32 / 1024.0);
            owners.push(Arc::downgrade(&audio));
            request.clip_audio.insert(id, audio);
            let mut markers = vibez_core::warp_marker::WarpMarkers::default();
            assert!(markers.add(2, 4, 0, 8, 8));
            request.audio_clips.push(ClipInfo {
                id,
                track_id,
                name: format!("clip {index}"),
                position: index as u64 * 32,
                source_offset: 0,
                start_marker: Some(0),
                duration: 16,
                source: None,
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: markers,
                transpose: Default::default(),
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
            });
        }
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        let mut warnings = Vec::new();
        let mut channel = prepare::channel(
            &request,
            &request.tracks[0],
            vibez_core::routing::SourceTap::AfterEffects,
            None,
            &mut warnings,
        );
        assert!(warnings.is_empty());
        let source = &channel.playback_source;
        assert_eq!(source.clips.len(), count);
        assert!(source.clips.capacity() >= count);
        assert!(source.removed_audio_clips.is_empty());
        assert!(source.removed_audio_clips.capacity() >= source.clips.len());
        for ((actual, original), owner) in
            source.clips.iter().zip(&request.audio_clips).zip(&owners)
        {
            assert_eq!(actual.id, original.id);
            assert_eq!(Arc::as_ptr(&actual.audio), owner.as_ptr());
            assert_eq!(owner.strong_count(), 2);
            assert_eq!(actual.position, original.position);
            assert_eq!(actual.source_offset, original.source_offset);
            assert_eq!(actual.start_marker, 0);
            assert_eq!(actual.duration, original.duration);
            assert_eq!(actual.loop_enabled, original.loop_enabled);
            assert_eq!((actual.loop_start, actual.loop_end), (0, 0));
            assert_eq!(actual.linear_gain, original.gain_db.linear());
            assert_eq!(actual.fades, original.fades);
            assert_eq!(actual.playback_direction, original.playback_direction);
            assert_eq!(actual.warp_markers, original.warp_markers);
        }
        for index in 0..count {
            assert!(channel.render(index as u64 * 32, 8, 2, None));
            let expected = (index + 1) as f32 / 1024.0;
            assert!(channel.mix_buffer[..16]
                .iter()
                .all(|sample| (*sample - expected).abs() < 1e-7));
        }
        drop(channel);
        assert!(owners.iter().all(|owner| owner.strong_count() == 1));
        drop(request);
        assert!(owners.iter().all(|owner| owner.strong_count() == 0));
    }
}
