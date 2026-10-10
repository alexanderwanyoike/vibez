use super::*;
use vibez_core::{
    audio_buffer::DecodedAudio,
    effect::{EffectInfo, EffectType, PluginDeviceInfo},
    id::EffectId,
    midi::{NoteClipInfo, TrackKind},
    track::{ClipInfo, TrackInfo},
};
use vibez_engine::render::{
    render_offline_with_plugins, BounceMode, BounceRequest, OfflinePlugins,
};
use vibez_engine::test_support::{DelayProbe, DelayProbeInstrument};

fn source(early: TrackId, full: TrackId, second: bool) -> CapturedTimelineSource {
    let mut section = Section::new(0);
    section.length_beats = 16.0;
    for (track, onsets, value) in [
        (
            early,
            if second {
                vec![0, 50, 300]
            } else {
                vec![400, 700, 1200, 1750]
            },
            0.1,
        ),
        (
            full,
            if second {
                vec![0, 100]
            } else {
                vec![100, 600, 1050, 1800]
            },
            0.2,
        ),
    ] {
        let mut samples = vec![0.0; 2048];
        for onset in onsets {
            samples[onset] = value;
        }
        let audio = Arc::new(DecodedAudio {
            channels: vec![samples],
            sample_rate: 128,
        });
        Arc::make_mut(&mut section.timeline)
            .ensure(track)
            .clips
            .push(UiClip {
                id: ClipId::new(),
                name: "Section pulse".into(),
                audio,
                source: None,
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: 2048,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                gain_db: Default::default(),
                fades: Default::default(),
                playback_direction: Default::default(),
                transient_markers: Default::default(),
                warp_markers: Default::default(),
                transpose: Default::default(),
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
    }
    CapturedTimelineSource::from_section_with_offsets(&section, Arc::from([(early, 384)]))
}

#[test]
fn fully_compensated_bounce_replays_heard_section_and_live_note_relationship_without_a_second_lead()
{
    let early = TrackId::new();
    let full = TrackId::new();
    let live = TrackId::new();
    let mut capture = CaptureState {
        phase: CapturePhase::Starting,
        ..Default::default()
    };
    capture.prepare(0, 128, 60.0);
    capture.prepare_controlled_tracks([(early, false), (full, false), (live, false)]);
    capture.start(0, Some((source(early, full, false), 0)));
    capture.input_note(live, 60, 100, true, 626);
    capture.input_note(live, 60, 0, false, 716);
    capture.transition(source(early, full, true), 1536);
    let materialized = capture.finish(1700).unwrap().materialize();
    let identity = || PluginDeviceInfo {
        format: "fixture".into(),
        uid: "timing".into(),
        path: Default::default(),
        name: "Timing".into(),
        state_b64: None,
    };
    let mut plugins = OfflinePlugins::default();
    let mut tracks = Vec::new();
    let mut audio_clips = Vec::new();
    let mut note_clips = Vec::new();
    let mut clip_audio = HashMap::new();
    for (id, delay) in [(early, 137), (full, 521), (live, 137)] {
        let mut track = TrackInfo::new("Capture");
        track.id = id;
        let effect = EffectId::new();
        track.effects.push(EffectInfo {
            inactive_sidechains: vec![],
            id: effect,
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![],
            plugin: Some(identity()),
            sidechains: vec![],
        });
        plugins
            .effects
            .insert(effect, Box::new(DelayProbe::with_report(delay, delay)));
        if id == live {
            track.kind = TrackKind::Midi;
            track.plugin_instrument = Some(identity());
            plugins
                .instruments
                .insert(id, Box::new(DelayProbeInstrument::new(0, 0.3)));
        }
        let content = &materialized.by_track[&id];
        for clip in &content.clips {
            clip_audio.insert(clip.id, Arc::clone(&clip.audio));
            audio_clips.push(ClipInfo {
                id: clip.id,
                track_id: id,
                name: clip.name.clone(),
                position: clip.position,
                source_offset: clip.source_offset,
                start_marker: Some(clip.start_marker),
                duration: clip.duration,
                source: None,
                file_path: None,
                loop_enabled: clip.loop_enabled,
                loop_start: clip.loop_start,
                loop_end: clip.loop_end,
                gain_db: clip.gain_db,
                fades: clip.fades,
                playback_direction: clip.playback_direction,
                transient_markers: Default::default(),
                warp_markers: clip.warp_markers.clone(),
                transpose: Default::default(),
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
            });
        }
        for clip in &content.note_clips {
            note_clips.push(NoteClipInfo {
                id: clip.id,
                track_id: id,
                name: clip.name.clone(),
                position_beats: clip.position_beats,
                duration_beats: clip.duration_beats,
                notes: clip.notes.clone(),
                start_marker_beats: Some(clip.start_marker_beats),
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
                groove_grid: clip.groove_grid,
            });
        }
        tracks.push(track);
    }
    let request = BounceRequest {
        tracks,
        master: None,
        buses: vec![],
        audio_clips,
        note_clips,
        clip_audio,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 1700),
        bpm: 60.0,
        sample_rate: 128,
        swing: Default::default(),
    };
    let output = render_offline_with_plugins(&request, &mut plugins, |_| {}).unwrap();
    for (frame, &sample) in output.audio.channels[0].iter().enumerate() {
        let mut expected = if (626..716).contains(&frame) {
            0.3
        } else {
            0.0
        };
        if [16, 316, 816, 1152, 1202, 1452].contains(&frame) {
            expected += 0.1;
        }
        if [100, 600, 1050, 1536, 1636].contains(&frame) {
            expected += 0.2;
        }
        assert!(
            (sample - expected * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6,
            "frame{frame}: {sample} expected{expected}"
        );
    }
}
