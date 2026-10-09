use super::*;
use vibez_core::routing::{ExternalInputId, SidechainAssignment, SourceTap};

fn clip(
    track: TrackId,
    audio: Arc<DecodedAudio>,
    position: u64,
    duration: u64,
) -> (ClipInfo, Arc<DecodedAudio>) {
    let info = ClipInfo {
        id: ClipId::new(),
        track_id: track,
        name: "Probe".into(),
        position,
        source_offset: 0,
        start_marker: None,
        duration,
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
    (info, audio)
}
fn receiving_effect(source: TrackId, tap: SourceTap) -> EffectInfo {
    EffectInfo {
        id: EffectId::new(),
        effect_type: EffectType::Gate,
        bypass: false,
        params: vec![-20.0, 0.1, 50.0, 10.0],
        plugin: None,
        sidechains: vec![SidechainAssignment {
            input_id: ExternalInputId(0),
            input_name: "Sidechain".into(),
            source,
            source_name: "Ghost".into(),
            tap,
        }],
    }
}
fn request(source: TrackInfo, mut receiving: TrackInfo, tap: SourceTap) -> BounceRequest {
    receiving.effects.push(receiving_effect(source.id, tap));
    let receiver_id = receiving.id;
    let (main, main_audio) = clip(receiver_id, audio_of(8192, 0.01), 0, 8192);
    let (trigger, trigger_audio) = clip(source.id, audio_of(8192, 1.0), 0, 8192);
    let assets = [(main.id, main_audio), (trigger.id, trigger_audio)]
        .into_iter()
        .collect();
    BounceRequest {
        tracks: vec![receiving, source],
        master: None,
        buses: vec![],
        audio_clips: vec![main, trigger],
        note_clips: vec![],
        clip_audio: assets,
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Track(receiver_id),
        range_samples: (0, 4096),
        bpm: 120.0,
        sample_rate: 44100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    }
}

#[test]
fn track_bounce_renders_muted_source_without_exporting_it_at_all_taps() {
    for tap in [
        SourceTap::BeforeEffects,
        SourceTap::AfterEffects,
        SourceTap::AfterFader,
    ] {
        let mut ghost = bare_track("Ghost");
        ghost.mute = true;
        let bass = bare_track("Bass");
        let req = request(ghost, bass, tap);
        let output =
            render_offline_with_plugins(&req, &mut OfflinePlugins::default(), |_| {}).unwrap();
        let tail = &output.audio.channels[0][4000..];
        if tap == SourceTap::AfterFader {
            assert!(tail.iter().all(|sample| sample.abs() < 1e-6));
        } else {
            assert!(tail.iter().all(|sample| *sample > 0.006 && *sample < 0.008));
        }
    }
}

#[test]
fn partial_bounce_preserves_prior_detector_context_and_exact_range() {
    let mut ghost = bare_track("Ghost");
    ghost.mute = true;
    let mut req = request(ghost, bare_track("Bass"), SourceTap::AfterEffects);
    let trigger = &mut req.audio_clips[1];
    trigger.duration = 1024;
    req.range_samples = (0, 4096);
    let full = render_offline(&req);
    req.range_samples = (2048, 4096);
    let partial = render_offline(&req);
    assert_eq!(partial.audio.num_frames(), 2048);
    for channel in 0..2 {
        assert_eq!(
            partial.audio.channels[channel],
            full.audio.channels[channel][2048..4096]
        );
    }
    assert!(partial.audio.channels[0]
        .iter()
        .any(|sample| *sample > 0.001));
}

#[test]
fn bus_sources_include_contributing_tracks_transitively_in_track_bounce() {
    let ghost = bare_track("Ghost");
    let mut req = request(ghost, bare_track("Bass"), SourceTap::AfterEffects);
    let mut bus = bare_track("Detector bus");
    let bus_id = bus.id;
    req.tracks[1].sends.push((bus_id, 1.0));
    req.tracks[0].effects[0].sidechains[0].source = bus_id;
    bus.mute = true;
    req.buses.push(bus);
    let ids = potential_dependency_ids(&req);
    assert!(ids.contains(&req.tracks[1].id));
    assert!(ids.contains(&bus_id));
    let output = render_offline(&req);
    assert!(output.audio.channels[0][4000..]
        .iter()
        .all(|sample| *sample > 0.006 && *sample < 0.008));
}

#[test]
fn clip_bounce_filters_selected_track_clips_but_keeps_external_source_material() {
    let mut ghost = bare_track("Ghost");
    ghost.mute = true;
    let mut req = request(ghost, bare_track("Bass"), SourceTap::AfterEffects);
    let chosen = req.audio_clips[0].id;
    req.mode = BounceMode::Clip {
        track_id: req.tracks[0].id,
        clip_id: chosen,
        is_note_clip: false,
    };
    let (unselected, audio) = clip(req.tracks[0].id, audio_of(8192, 1.0), 0, 8192);
    req.clip_audio.insert(unselected.id, audio);
    req.audio_clips.push(unselected);
    let output = render_offline(&req);
    assert!(output.audio.channels[0][4000..]
        .iter()
        .all(|sample| *sample > 0.006 && *sample < 0.008));
}

#[test]
fn missing_source_never_switches_a_gate_back_to_main_detection() {
    let mut req = request(
        bare_track("Ghost"),
        bare_track("Bass"),
        SourceTap::AfterEffects,
    );
    let removed = req.tracks.pop().unwrap();
    req.audio_clips.retain(|clip| clip.track_id != removed.id);
    let replacement = bare_track("Ghost");
    assert_ne!(replacement.id, removed.id);
    req.tracks.push(replacement);
    let output = render_offline(&req);
    assert!(output.audio.channels[0]
        .iter()
        .all(|sample| sample.abs() < 1e-6));
    req.tracks[0].effects[0].sidechains.clear();
    req.clip_audio
        .insert(req.audio_clips[0].id, audio_of(8192, 0.5));
    let disconnected = render_offline(&req);
    assert!(disconnected.audio.channels[0][4000] > 0.3);
}

#[test]
fn before_effects_dependency_does_not_require_an_unreached_source_plugin() {
    let mut source = bare_track("Ghost");
    source.mute = true;
    source.effects.push(EffectInfo {
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![],
        plugin: Some(plugin_device("Unavailable source insert")),
        sidechains: vec![],
    });
    let mut req = request(source, bare_track("Bass"), SourceTap::BeforeEffects);
    let output = render_offline_with_plugins(&req, &mut OfflinePlugins::default(), |_| {}).unwrap();
    assert!(output.audio.channels[0][4000] > 0.006);
    req.tracks[0].effects[0].sidechains[0].tap = SourceTap::AfterEffects;
    let error = render_offline_with_plugins(&req, &mut OfflinePlugins::default(), |_| {})
        .err()
        .unwrap();
    assert!(error.contains("Unavailable source insert"));
}

#[test]
fn unavailable_receiver_input_does_not_make_its_stale_source_required() {
    let mut source = bare_track("Bad synth");
    source.kind = TrackKind::Midi;
    source.plugin_instrument = Some(plugin_device("Unavailable synth"));
    let source_id = source.id;
    let mut req = request(source, bare_track("Bass"), SourceTap::AfterEffects);
    req.tracks[0].effects[0].sidechains[0].input_id = ExternalInputId(99);
    assert!(potential_dependency_ids(&req).contains(&source_id));
    let mut plugins = OfflinePlugins::default();
    plugins
        .instrument_failures
        .insert(source_id, "Missing library".into());
    assert!(render_offline_with_plugins(&req, &mut plugins, |_| {}).is_ok());
    req.tracks[0].effects[0].sidechains[0].input_id = ExternalInputId(0);
    let error = render_offline_with_plugins(&req, &mut plugins, |_| {})
        .err()
        .unwrap();
    assert!(error.contains("Missing library"));
}
