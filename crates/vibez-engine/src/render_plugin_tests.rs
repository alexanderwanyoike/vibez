use super::*;

struct EchoPluginEffect {
    delay: [f32; 1024],
    cursor: usize,
}

impl EchoPluginEffect {
    fn new() -> Self {
        Self {
            delay: [0.0; 1024],
            cursor: 0,
        }
    }
}

impl vibez_dsp::effect::AudioEffect for EchoPluginEffect {
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
        0.0
    }
    fn process(&mut self, buffer: &mut [f32], _channels: usize) {
        for sample in buffer {
            let delayed = self.delay[self.cursor];
            self.delay[self.cursor] = *sample + delayed * 0.5;
            *sample += delayed;
            self.cursor = (self.cursor + 1) % self.delay.len();
        }
    }
    fn reset(&mut self) {
        self.delay.fill(0.0);
        self.cursor = 0;
    }
}

fn echo_render_request() -> BounceRequest {
    BounceRequest {
        tracks: vec![bare_track("Vox")],
        master: None,
        buses: Vec::new(),
        audio_clips: Vec::new(),
        note_clips: Vec::new(),
        clip_audio: HashMap::new(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 4096),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    }
}

fn sampler_echo_fixture() -> (BounceRequest, Vec<Command>) {
    let mut request = echo_render_request();
    request.tracks[0].kind = TrackKind::Midi;
    let tid = request.tracks[0].id;
    let sample = audio_of(128, 0.5);
    let mut commands = Vec::new();
    request.tracks[0].instrument = Some(InstrumentKind::Sampler);
    request.tracks[0].native_instrument = Some(InstrumentStateInfo::Sampler {
        params: Vec::new(),
        source: None,
    });
    commands.push(Command::AddInstrumentTrack(
        tid,
        "Vox".into(),
        InstrumentKind::Sampler,
    ));
    commands.push(Command::LoadSamplerSample {
        track_id: tid,
        sample: Arc::clone(&sample),
        sample_name: "Vox".into(),
    });
    request
        .sampler_audio
        .insert(tid, (Arc::clone(&sample), "Vox".into()));
    let clip = NoteClipInfo {
        id: ClipId::new(),
        track_id: tid,
        name: "Vocal hits".into(),
        position_beats: 0.0,
        duration_beats: 1.0,
        start_marker_beats: None,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: Default::default(),
        notes: vec![0.0, 0.07]
            .into_iter()
            .map(|start_beat| MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat,
                duration_beats: 0.01,
            })
            .collect(),
    };
    commands.push(Command::AddNoteClip {
        track_id: tid,
        clip_id: clip.id,
        position_beats: clip.position_beats,
        duration_beats: clip.duration_beats,
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: clip.groove_grid,
    });
    for note in &clip.notes {
        commands.push(Command::AddNote {
            track_id: tid,
            clip_id: clip.id,
            note: *note,
        });
    }
    request.note_clips.push(clip);
    (request, commands)
}

fn audio_echo_fixture() -> (BounceRequest, Vec<Command>) {
    let mut request = echo_render_request();
    let tid = request.tracks[0].id;
    let sample = audio_of(128, 0.5);
    let mut commands = Vec::new();
    commands.push(Command::AddTrack(tid, "Vox".into()));
    for position in [0, 1536] {
        let clip = ClipInfo {
            id: ClipId::new(),
            track_id: tid,
            name: "Vocal hit".into(),
            position,
            source_offset: 0,
            start_marker: None,
            duration: 128,
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
        commands.push(Command::AddClip {
            track_id: tid,
            clip_id: clip.id,
            audio: Arc::clone(&sample),
            position,
            source_offset: 0,
            start_marker: 0,
            duration: 128,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            linear_gain: 1.0,
            fades: Default::default(),
            playback_direction: Default::default(),
            warp_markers: Default::default(),
        });
        request.clip_audio.insert(clip.id, Arc::clone(&sample));
        request.audio_clips.push(clip);
    }
    (request, commands)
}

fn assert_export_preserves_insert_echo(
    (mut request, setup_commands): (BounceRequest, Vec<Command>),
) {
    let track = &mut request.tracks[0];
    let tid = track.id;
    let kind = track.kind;
    let effect_id = EffectId::new();
    track.effects.push(EffectInfo {
        inactive_sidechains: Default::default(),
        sidechains: Vec::new(),
        id: effect_id,
        effect_type: EffectType::Gain,
        bypass: false,
        params: Vec::new(),
        plugin: Some(plugin_device("Echo")),
    });
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(Command::SetBpm(request.bpm)).unwrap();
    for command in setup_commands {
        commands.push(command).unwrap();
    }
    commands
        .push(Command::AddPluginEffect {
            track_id: tid,
            effect_id,
            effect: Box::new(EchoPluginEffect::new()),
            position: None,
        })
        .unwrap();
    // Recording mode disables end-of-content auto-stop without looping.
    // This lets the live path cover the export range after the last hit;
    // no recording input or output capture is attached to these blocks.
    commands
        .push(Command::SetArrangementRecording(true))
        .unwrap();
    commands.push(Command::Play).unwrap();
    let mut live = vec![0.0; request.range_samples.1 as usize * CHANNELS];
    for block in live.chunks_mut(BLOCK_FRAMES * 2) {
        engine.process_block(AudioProcessBlock::new(block, 2));
        while events.pop().is_ok() {}
    }
    assert!(
        live[1024..2048].iter().any(|sample| *sample > 0.01),
        "live playback must contain the first echo after the source stops"
    );
    let mut plugins = OfflinePlugins::default();
    plugins
        .effects
        .insert(effect_id, Box::new(EchoPluginEffect::new()));
    let exported = render_offline_with_plugins(&request, &mut plugins, |_| {}).unwrap();
    assert!(exported.warnings.is_empty());
    for (frame, expected) in live.as_chunks::<2>().0.iter().enumerate() {
        for (channel, expected) in expected.iter().enumerate() {
            assert!((exported.audio.channels[channel][frame] - expected).abs() < 1e-6,
                "{kind:?} insert echo differs at frame {frame}, channel {channel}: export={}, live={expected}",
                exported.audio.channels[channel][frame]);
        }
    }
}

#[test]
fn export_preserves_sampler_insert_echo_between_notes() {
    assert_export_preserves_insert_echo(sampler_echo_fixture());
}

#[test]
fn export_preserves_audio_insert_echo_between_clips() {
    assert_export_preserves_insert_echo(audio_echo_fixture());
}

#[test]
fn strict_render_fails_when_declared_plugin_was_not_prepared() {
    let mut track = bare_track("Plugin Bass");
    track.kind = TrackKind::Midi;
    track.plugin_instrument = Some(plugin_device("Surge XT"));
    let req = BounceRequest {
        master: None,
        buses: Vec::new(),
        tracks: vec![track],
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

    let mut plugins = OfflinePlugins::default();
    let error = match render_offline_with_plugins(&req, &mut plugins, |_| {}) {
        Ok(_) => panic!("missing plugin must fail the strict render"),
        Err(error) => error,
    };

    assert!(error.contains("Surge XT"));
    assert!(error.contains("not prepared"));
}

#[test]
fn strict_render_uses_plugin_effects_on_tracks_buses_and_master() {
    fn plugin_effect() -> EffectInfo {
        EffectInfo {
            inactive_sidechains: Default::default(),
            sidechains: Vec::new(),
            id: EffectId::new(),
            effect_type: EffectType::Gain,
            bypass: false,
            params: Vec::new(),
            plugin: Some(plugin_device("Scale")),
        }
    }

    let mut track = bare_track("audio");
    let tid = track.id;
    let cid = ClipId::new();
    let track_fx = plugin_effect();
    track.effects.push(track_fx.clone());
    let mut bus = bare_track("Return");
    let bus_fx = plugin_effect();
    bus.effects.push(bus_fx.clone());
    track.sends.push((bus.id, 1.0));
    let mut master = bare_track("Master");
    master.id = TrackId::MASTER;
    let master_fx = plugin_effect();
    master.effects.push(master_fx.clone());
    let clip = ClipInfo {
        id: cid,
        track_id: tid,
        name: "audio".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 64,
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
    let req = BounceRequest {
        master: Some(master),
        buses: vec![bus],
        tracks: vec![track],
        audio_clips: vec![clip],
        note_clips: Vec::new(),
        clip_audio: HashMap::from([(cid, audio_of(64, 1.0))]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 64),
        bpm: 120.0,
        sample_rate: 44_100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    let mut plugins = OfflinePlugins::default();
    plugins
        .effects
        .insert(track_fx.id, Box::new(ScalePluginEffect(0.5)));
    plugins
        .effects
        .insert(bus_fx.id, Box::new(ScalePluginEffect(0.5)));
    plugins
        .effects
        .insert(master_fx.id, Box::new(ScalePluginEffect(0.5)));

    let result = render_offline_with_plugins(&req, &mut plugins, |_| {}).unwrap();

    // Track: 1 * .5. Dry + return(.5) = .75 before centered pan,
    // then master .5.
    let expected = 0.75 * std::f32::consts::FRAC_1_SQRT_2 * 0.5;
    assert!((result.audio.channels[0][10] - expected).abs() < 1e-3);
    assert_eq!(
        plugins.effects.len(),
        3,
        "every effect must be returned for main-thread teardown"
    );
}

#[test]
fn effect_chain_applied_during_bounce() {
    let tid = TrackId::new();
    let cid = ClipId::new();
    let mut track = bare_track("audio");
    track.id = tid;
    // Gain of 0.5 halves the bounce output
    track.effects.push(EffectInfo {
        inactive_sidechains: Default::default(),
        sidechains: Vec::new(),
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![0.5],
        plugin: None,
    });
    let audio = audio_of(100, 1.0);
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
    let peak = out.audio.channels[0]
        .iter()
        .map(|s| s.abs())
        .fold(0.0_f32, f32::max);
    let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!((peak - expected).abs() < 1e-3, "peak {peak}");
}
