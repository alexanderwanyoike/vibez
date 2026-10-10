use super::*;
use crate::commands::EngineCommand as Command;
use crate::engine::{AudioEngine, AudioProcessBlock};
use crate::playback_source::PreparedPlaybackSource;
use vibez_core::constants::{DEFAULT_TRACK_GAIN, DEFAULT_TRACK_PAN};
use vibez_core::effect::{EffectInfo, EffectType, ParamDescriptor, PluginDeviceInfo};
use vibez_core::id::EffectId;
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};

struct ConstantPluginInstrument {
    active: bool,
}

impl vibez_instruments::Instrument for ConstantPluginInstrument {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }
    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, _pitch: u8, _velocity: u8) {
        self.active = true;
    }
    fn note_off(&mut self, _pitch: u8) {
        self.active = false;
    }
    fn render(&mut self, buffer: &mut [f32], _channels: usize) {
        if self.active {
            buffer.iter_mut().for_each(|sample| *sample = 0.5);
        }
    }
    fn reset(&mut self) {
        self.active = false;
    }
}

struct ScalePluginEffect(f32);

impl vibez_dsp::effect::AudioEffect for ScalePluginEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }
    fn get_param(&self, _index: usize) -> f32 {
        self.0
    }
    fn process(&mut self, buffer: &mut [f32], _channels: usize) {
        buffer.iter_mut().for_each(|sample| *sample *= self.0);
    }
    fn reset(&mut self) {}
}

fn plugin_device(name: &str) -> PluginDeviceInfo {
    PluginDeviceInfo {
        format: "clap".into(),
        uid: format!("test.{name}"),
        path: format!("/test/{name}.clap").into(),
        name: name.into(),
        state_b64: Some("c3RhdGU=".into()),
    }
}

fn audio_of(frames: usize, value: f32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![vec![value; frames], vec![value; frames]],
        sample_rate: 44_100,
    })
}

fn bare_track(name: &str) -> TrackInfo {
    TrackInfo {
        id: TrackId::new(),
        name: name.into(),
        gain: DEFAULT_TRACK_GAIN,
        pan: DEFAULT_TRACK_PAN,
        mute: false,
        solo: false,
        audio_input_route: Default::default(),
        input_monitoring: Default::default(),
        swing_offset: None,
        effects: Vec::new(),
        kind: TrackKind::Audio,
        color_index: 0,
        instrument: None,
        native_instrument: None,
        plugin_instrument: None,
        automation: Vec::new(),
        sends: Vec::new(),
    }
}

#[test]
fn offline_project_render_has_no_audition_bus_input() {
    let request = BounceRequest {
        tracks: Vec::new(),
        master: None,
        buses: Vec::new(),
        audio_clips: Vec::new(),
        note_clips: Vec::new(),
        clip_audio: HashMap::new(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 512),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };

    let result = render_offline(&request);

    assert!(result
        .audio
        .channels
        .iter()
        .flatten()
        .all(|sample| sample.abs() < f32::EPSILON));
}

#[test]
fn master_bounce_routes_sends_through_buses() {
    let mut track = bare_track("audio");
    track.pan = DEFAULT_TRACK_PAN;
    let bus = bare_track("A Return");
    let bus_id = bus.id;
    track.sends.push((bus_id, 1.0));
    let tid = track.id;
    let audio = audio_of(200, 0.5);
    let cid = ClipId::new();
    let clip = ClipInfo {
        id: cid,
        track_id: tid,
        name: "c".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 200,
        source: None,
        file_path: None,
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
    };
    let mut clip_audio = HashMap::new();
    clip_audio.insert(cid, audio);

    let mut req = BounceRequest {
        master: None,
        buses: vec![bus],
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 200),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let result = render_offline(&req);
    // Dry + unity send through a flat centered bus doubles the
    // contribution, exactly like the live engine.
    let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2 * 2.0;
    assert!(
        (result.audio.channels[0][10] - expected).abs() < 1e-3,
        "expected {expected}, got {}",
        result.audio.channels[0][10]
    );

    req.buses[0].solo = true;
    req.buses[0].gain = 0.5;
    let soloed = render_offline(&req);
    let wet_only = 0.5 * std::f32::consts::FRAC_1_SQRT_2 * 0.5;
    assert!(
        (soloed.audio.channels[0][10] - wet_only).abs() < 1e-3,
        "soloed return should suppress dry audio: expected {wet_only}, got {}",
        soloed.audio.channels[0][10]
    );
}

#[test]
fn master_render_applies_audio_clip_gain() {
    let mut track = bare_track("audio");
    track.pan = DEFAULT_TRACK_PAN;
    let tid = track.id;
    let audio = audio_of(200, 0.5);
    let cid = ClipId::new();
    let clip = ClipInfo {
        id: cid,
        track_id: tid,
        name: "c".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 200,
        source: None,
        file_path: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: vibez_core::track::ClipGainDb::new(-6.0).unwrap(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
    };

    let mut clip_audio = HashMap::new();
    clip_audio.insert(cid, audio);

    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 200),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };

    let result = render_offline(&req);
    assert_eq!(result.audio.num_frames(), 200);
    let expected = 0.5
        * vibez_core::track::ClipGainDb::new(-6.0).unwrap().linear()
        * std::f32::consts::FRAC_1_SQRT_2;
    for frame in 0..200 {
        assert!((result.audio.channels[0][frame] - expected).abs() < 1e-4);
        assert!((result.audio.channels[1][frame] - expected).abs() < 1e-4);
    }
}

#[test]
fn offline_render_uses_the_same_reverse_traversal_as_live_playback() {
    let mut track = bare_track("audio");
    track.pan = DEFAULT_TRACK_PAN;
    let track_id = track.id;
    let clip_id = ClipId::new();
    let clip = ClipInfo {
        id: clip_id,
        track_id,
        name: "reverse".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 4,
        source: None,
        file_path: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 4,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: vibez_core::track::ClipPlaybackDirection::Reverse,
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
    };
    let request = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio: HashMap::from([(
            clip_id,
            Arc::new(DecodedAudio {
                channels: vec![vec![1.0, 2.0, 3.0, 4.0]],
                sample_rate: 44_100,
            }),
        )]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 4),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };

    let result = render_offline(&request);
    let pan = std::f32::consts::FRAC_1_SQRT_2;
    assert_eq!(
        result.audio.channels[0],
        vec![4.0 * pan, 3.0 * pan, 2.0 * pan, pan]
    );
    assert_eq!(result.audio.channels[1], result.audio.channels[0]);
}

#[test]
fn offline_and_prepared_live_paths_share_the_exact_piecewise_warp_map() {
    let mut track = bare_track("audio");
    track.pan = DEFAULT_TRACK_PAN;
    let track_id = track.id;
    let clip_id = ClipId::new();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.0, 10.0, 20.0, 30.0, 40.0]],
        sample_rate: 44_100,
    });
    let mut warp_markers = vibez_core::warp_marker::WarpMarkers::default();
    assert!(warp_markers.add(1, 2, 0, 4, 4));
    let clip = ClipInfo {
        id: clip_id,
        track_id,
        name: "piecewise".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 4,
        source: None,
        file_path: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 4,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: warp_markers.clone(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
    };
    let request = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio: HashMap::from([(clip_id, Arc::clone(&audio))]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 4),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let mut live = vec![0.0; 4];
    PreparedPlaybackSource::new(
        vec![EngineClip {
            id: clip_id,
            audio,
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 4,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 4,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers,
        }],
        Vec::new(),
        Vec::new(),
    )
    .render_audio(&mut live, 0, 4, 1, None);

    assert_eq!(live, vec![0.0, 5.0, 10.0, 25.0]);
    let offline = render_offline(&request);
    let pan = std::f32::consts::FRAC_1_SQRT_2;
    assert_eq!(
        offline.audio.channels[0],
        live.iter().map(|sample| sample * pan).collect::<Vec<_>>()
    );
    assert_eq!(offline.audio.channels[1], offline.audio.channels[0]);
}

#[test]
fn clip_bounce_keeps_reverse_warp_and_shaped_fades_in_one_render() {
    let mut track = bare_track("audio");
    track.pan = DEFAULT_TRACK_PAN;
    let track_id = track.id;
    let clip_id = ClipId::new();
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.0, 0.1, 0.25, 0.4, 0.55, 0.7, 0.85, 1.0, 0.5]],
        sample_rate: 44_100,
    });
    let mut warp_markers = vibez_core::warp_marker::WarpMarkers::default();
    assert!(warp_markers.add(2, 3, 0, 8, 8));
    let fades = vibez_core::track::ClipFades::new(3, 3, 8)
        .with_fade_in_curve(vibez_core::track::FadeCurve::new(70))
        .with_fade_out_curve(vibez_core::track::FadeCurve::new(-60));
    let clip = ClipInfo {
        id: clip_id,
        track_id,
        name: "edited".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 8,
        source: None,
        file_path: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 8,
        gain_db: Default::default(),
        fades,
        playback_direction: vibez_core::track::ClipPlaybackDirection::Reverse,
        transient_markers: Default::default(),
        warp_markers: warp_markers.clone(),
        transpose: Default::default(),
        original_bpm: None,
        warped: true,
        warped_to_bpm: Some(120.0),
    };
    let request = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio: HashMap::from([(clip_id, Arc::clone(&audio))]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Clip {
            track_id,
            clip_id,
            is_note_clip: false,
        },
        range_samples: (0, 8),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let mut live = vec![0.0; 8];
    PreparedPlaybackSource::new(
        vec![EngineClip {
            id: clip_id,
            audio,
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 8,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 8,
            linear_gain: 1.0,
            fades,
            playback_direction: vibez_core::track::ClipPlaybackDirection::Reverse,
            warp_markers,
        }],
        Vec::new(),
        Vec::new(),
    )
    .render_audio(&mut live, 0, 8, 1, None);

    let bounced = render_offline(&request);
    let pan = std::f32::consts::FRAC_1_SQRT_2;
    for (frame, expected) in live.into_iter().enumerate() {
        assert!((bounced.audio.channels[0][frame] - expected * pan).abs() < 1e-6);
        assert!((bounced.audio.channels[1][frame] - expected * pan).abs() < 1e-6);
    }
}

#[test]
fn mute_silences_master_bounce() {
    let mut track = bare_track("audio");
    track.mute = true;
    let tid = track.id;
    let audio = audio_of(100, 0.7);
    let cid = ClipId::new();
    let clip = ClipInfo {
        id: cid,
        track_id: tid,
        name: "c".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 100,
        source: None,
        file_path: None,
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
    };
    let mut clip_audio = HashMap::new();
    clip_audio.insert(cid, audio);

    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 100),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let out = render_offline(&req);
    assert!(out.audio.channels[0].iter().all(|&s| s.abs() < 1e-6));
}

#[test]
fn track_mode_ignores_mute() {
    let mut track = bare_track("audio");
    track.mute = true;
    let tid = track.id;
    let audio = audio_of(100, 0.5);
    let cid = ClipId::new();
    let clip = ClipInfo {
        id: cid,
        track_id: tid,
        name: "c".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 100,
        source: None,
        file_path: None,
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
    };
    let mut clip_audio = HashMap::new();
    clip_audio.insert(cid, audio);
    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Track(tid),
        range_samples: (0, 100),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let out = render_offline(&req);
    assert!(out.audio.channels[0].iter().any(|&s| s.abs() > 1e-4));
}

#[test]
fn clip_mode_isolates_single_clip() {
    let mut track = bare_track("audio");
    let tid = track.id;
    track.pan = DEFAULT_TRACK_PAN;
    let cid_a = ClipId::new();
    let cid_b = ClipId::new();
    let audio_a = audio_of(100, 0.3);
    let audio_b = audio_of(100, 0.9);
    let clip_a = ClipInfo {
        id: cid_a,
        track_id: tid,
        name: "a".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 100,
        source: None,
        file_path: None,
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
    };
    let clip_b = ClipInfo {
        id: cid_b,
        track_id: tid,
        name: "b".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 100,
        source: None,
        file_path: None,
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
    };
    let mut clip_audio = HashMap::new();
    clip_audio.insert(cid_a, audio_a);
    clip_audio.insert(cid_b, audio_b);

    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: vec![clip_a, clip_b],
        note_clips: Vec::new(),
        clip_audio,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Clip {
            track_id: tid,
            clip_id: cid_a,
            is_note_clip: false,
        },
        range_samples: (0, 100),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let out = render_offline(&req);
    let expected = 0.3 * std::f32::consts::FRAC_1_SQRT_2;
    assert!((out.audio.channels[0][0] - expected).abs() < 1e-4);
}

#[test]
fn synth_note_clip_produces_audio() {
    let tid = TrackId::new();
    let cid = ClipId::new();
    let track = TrackInfo {
        id: tid,
        name: "Synth".into(),
        gain: DEFAULT_TRACK_GAIN,
        pan: DEFAULT_TRACK_PAN,
        mute: false,
        solo: false,
        audio_input_route: Default::default(),
        input_monitoring: Default::default(),
        swing_offset: None,
        effects: Vec::new(),
        kind: TrackKind::Instrument(InstrumentKind::SubtractiveSynth),
        color_index: 0,
        instrument: Some(InstrumentKind::SubtractiveSynth),
        native_instrument: Some(InstrumentStateInfo::SubtractiveSynth { params: Vec::new() }),
        plugin_instrument: None,
        automation: Vec::new(),
        sends: Vec::new(),
    };
    let note_clip = NoteClipInfo {
        id: cid,
        track_id: tid,
        name: "p".into(),
        position_beats: 0.0,
        duration_beats: 1.0,
        start_marker_beats: None,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.25,
            duration_beats: 0.5,
        }],
    };
    let mut req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: Vec::new(),
        note_clips: vec![note_clip],
        clip_audio: HashMap::new(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Track(tid),
        range_samples: (0, 22_050),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let straight = render_offline(&req);
    assert!(straight.audio.channels[0].iter().any(|&s| s.abs() > 1e-3));
    req.swing = vibez_core::perform::SwingAmount::new(0.75);
    let swung = render_offline(&req);
    assert_ne!(straight.audio.channels, swung.audio.channels);
}

#[test]
fn warns_on_midi_track_with_no_native_instrument() {
    let tid = TrackId::new();
    let track = TrackInfo {
        id: tid,
        name: "Plugin Stub".into(),
        gain: DEFAULT_TRACK_GAIN,
        pan: DEFAULT_TRACK_PAN,
        mute: false,
        solo: false,
        audio_input_route: Default::default(),
        input_monitoring: Default::default(),
        swing_offset: None,
        effects: Vec::new(),
        kind: TrackKind::Midi,
        color_index: 0,
        instrument: None,
        native_instrument: None,
        plugin_instrument: None,
        automation: Vec::new(),
        sends: Vec::new(),
    };
    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: Vec::new(),
        note_clips: Vec::new(),
        clip_audio: HashMap::new(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Track(tid),
        range_samples: (0, 4_410),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let out = render_offline(&req);
    assert!(!out.warnings.is_empty());
}

#[test]
fn strict_render_uses_prepared_plugin_instrument_and_reports_progress() {
    let tid = TrackId::new();
    let cid = ClipId::new();
    let mut track = bare_track("Plugin Bass");
    track.id = tid;
    track.kind = TrackKind::Midi;
    track.plugin_instrument = Some(plugin_device("Surge XT"));
    let note_clip = NoteClipInfo {
        id: cid,
        track_id: tid,
        name: "bass".into(),
        position_beats: 0.0,
        duration_beats: 1.0,
        start_marker_beats: None,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
        notes: vec![MidiNote {
            pitch: 36,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 1.0,
        }],
    };
    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
        audio_clips: Vec::new(),
        note_clips: vec![note_clip],
        clip_audio: HashMap::new(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 2_048),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let mut plugins = OfflinePlugins::default();
    plugins
        .instruments
        .insert(tid, Box::new(ConstantPluginInstrument { active: false }));
    let mut progress = Vec::new();

    let result =
        render_offline_with_plugins(&req, &mut plugins, |value| progress.push(value)).unwrap();

    assert!(result.audio.channels[0].iter().any(|sample| *sample > 0.1));
    assert!(
        plugins.instruments.contains_key(&tid),
        "renderer must return the plugin for main-thread teardown"
    );
    assert_eq!(progress.first(), Some(&0));
    assert_eq!(progress.last(), Some(&100));
    assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[path = "render_plugin_tests.rs"]
mod plugin_tests;
