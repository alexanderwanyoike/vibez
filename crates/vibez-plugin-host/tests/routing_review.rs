//! Review regressions through the loadable CLAP and VST3 fixtures.

#[allow(dead_code)]
mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;
use vibez_plugin_host::{
    clap_host::instance::ClapPluginInstance,
    instance::PluginInstance,
    vst3_host::{instance::Vst3PluginInstance, scanner::scan_vst3},
};

#[test]
fn declared_surround_main_uses_first_pair_without_enabling_surround_aux() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut plugin: Box<dyn PluginInstance> = if format == "clap" {
            Box::new(
                ClapPluginInstance::load(
                    &fixture.clap,
                    "vibez.fixture.surround",
                    false,
                    48000.0,
                    64,
                )
                .unwrap(),
            )
        } else {
            let classes = scan_vst3(&fixture.vst3).unwrap();
            Box::new(
                Vst3PluginInstance::load(&fixture.vst3, &classes[6].id.uid, false, 48000.0, 64)
                    .unwrap(),
            )
        };
        assert_eq!(
            plugin
                .external_inputs()
                .iter()
                .map(|input| input.channels)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        for channels in [1, 2, 6] {
            let mut samples = vec![0.0; channels * 7];
            for frame in samples.chunks_exact_mut(channels) {
                frame[0] = 0.25;
                if channels > 1 {
                    frame[1] = 0.75;
                }
                for extra in frame.iter_mut().skip(2) {
                    *extra = 8.0;
                }
            }
            let unsupported = [9.0; 6 * 7];
            let input = vibez_core::routing::ExternalInputBlock {
                id: vibez_core::routing::ExternalInputId(if format == "clap" { 13 } else { 3 }),
                channels: 6,
                samples: &unsupported,
                connected: true,
            };
            assert_eq!(
                support::allocation::count_allocations(|| plugin.process_with_inputs(
                    &mut samples,
                    channels,
                    &[input]
                )),
                0
            );
            assert!(plugin.take_processing_error().is_none());
            for frame in samples.chunks_exact(channels) {
                assert_eq!(frame[0], 0.25);
                if channels > 1 {
                    assert_eq!(frame[1], 0.75);
                }
                assert!(frame.iter().skip(2).all(|sample| *sample == 0.0));
            }
        }
        plugin.stop_processing();
    }
}

#[test]
fn refused_optional_buses_keep_main_processing_and_declared_indices() {
    let fixture = support::Fixture::new();
    let plugins = scan_vst3(&fixture.vst3).unwrap();
    for (class, expected_inputs) in [(2, vec![2]), (3, vec![1, 2])] {
        let mut plugin =
            Vst3PluginInstance::load(&fixture.vst3, &plugins[class].id.uid, false, 48000.0, 64)
                .unwrap();
        assert_eq!(
            plugin
                .external_inputs()
                .iter()
                .map(|input| input.id.0)
                .collect::<Vec<_>>(),
            expected_inputs
        );
        let mut output = [0.25; 14];
        assert_eq!(
            support::allocation::count_allocations(|| plugin.process_audio(&mut output, 2)),
            0
        );
        assert_eq!(
            output, [0.25; 14],
            "optional bus failure corrupted main processing"
        );
        assert!(plugin.take_processing_error().is_none());
        plugin.stop_processing();
    }
    assert!(
        Vst3PluginInstance::load(&fixture.vst3, &plugins[4].id.uid, false, 48000.0, 64).is_err()
    );
}

#[test]
fn full_clap_parameter_queue_does_not_mutate_host_or_plugin_value() {
    let fixture = support::Fixture::new();
    let mut plugin = ClapPluginInstance::load(
        &fixture.clap,
        vibez_routing_fixture::CLAP_ID,
        false,
        48000.0,
        64,
    )
    .unwrap();
    let mut accepted = 0;
    while plugin.set_param(0, 0.25) {
        accepted += 1;
        assert!(accepted <= 4096);
    }
    assert!(accepted > 0);
    assert!(!plugin.set_param(0, 0.75));
    assert_eq!(plugin.get_param(0), 0.25);
    let library = unsafe { libloading::Library::new(&fixture.clap).unwrap() };
    let delivered: libloading::Symbol<
        unsafe extern "C" fn(*const clap_sys::plugin::clap_plugin) -> f64,
    > = unsafe { library.get(b"vibez_fixture_parameter").unwrap() };
    assert_eq!(unsafe { delivered(plugin.plugin_ptr()) }, 0.0);
    plugin.process_audio(&mut [0.0; 14], 2);
    assert_eq!(unsafe { delivered(plugin.plugin_ptr()) }, 0.25);
    assert!(plugin.set_param(0, 0.5));
    plugin.process_audio(&mut [0.0; 14], 2);
    assert_eq!(unsafe { delivered(plugin.plugin_ptr()) }, 0.5);
    plugin.stop_processing();
}

#[test]
fn native_processing_failure_is_cached_once_without_callback_logging() {
    let fixture = support::Fixture::new();
    let plugins = scan_vst3(&fixture.vst3).unwrap();
    let mut plugin =
        Vst3PluginInstance::load(&fixture.vst3, &plugins[5].id.uid, false, 48000.0, 64).unwrap();
    let mut output = [0.25; 14];
    plugin.process_audio(&mut output, 2);
    assert_eq!(output, [0.0; 14]);
    assert_eq!(
        plugin.take_processing_error(),
        Some("VST3 process returned failure")
    );
    assert!(plugin.take_processing_error().is_none());
    plugin.process_audio(&mut output, 2);
    assert!(plugin.take_processing_error().is_none());
    plugin.stop_processing();
}

#[test]
fn actual_engine_delivers_the_cached_native_failure_to_its_ui_consumer() {
    use vibez_core::{
        id::{EffectId, TrackId},
        routing::{RoutingChannel, RoutingEffect},
    };
    use vibez_engine::{
        commands::EngineCommand,
        engine::{AudioEngine, AudioProcessBlock},
        events::EngineEvent,
        routing::PreparedRouting,
    };
    let fixture = support::Fixture::new();
    let plugins = scan_vst3(&fixture.vst3).unwrap();
    let plugin =
        Vst3PluginInstance::load(&fixture.vst3, &plugins[5].id.uid, false, 48000.0, 64).unwrap();
    let track = TrackId::new();
    let effect = EffectId::new();
    let model = RoutingChannel {
        id: track,
        is_bus: false,
        sends: vec![],
        effects: vec![RoutingEffect {
            id: effect,
            inputs: plugin.external_inputs().to_vec(),
            assignments: vec![],
            inactive_inputs: vec![],
        }],
    };
    let master = RoutingChannel {
        id: TrackId::MASTER,
        is_bus: true,
        sends: vec![],
        effects: vec![],
    };
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(EngineCommand::AddTrack(track, "Failing probe".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            effect: Box::new(
                vibez_plugin_host::wrappers::effect::PluginEffectWrapper::new(Box::new(plugin)),
            ),
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            PreparedRouting::prepare(&[model, master], 64).unwrap(),
        ))
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    while events.pop().is_ok() {}
    for _ in 0..vibez_core::constants::RING_BUFFER_CAPACITY * 2 {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    }
    assert_eq!(
        support::allocation::count_allocations(
            || engine.process_block(AudioProcessBlock::new(&mut [0.0; 14], 2))
        ),
        0
    );
    // Only one slot is freed: the retained authoritative cause must win it
    // before any meter or cursor event can refill the ring.
    events.pop().unwrap();
    assert_eq!(
        support::allocation::count_allocations(
            || engine.process_block(AudioProcessBlock::new(&mut [0.0; 14], 2))
        ),
        0
    );
    let mut failures = 0;
    while let Ok(event) = events.pop() {
        if let EngineEvent::DeviceProcessingFailed {
            track_id,
            effect_id,
            reason,
        } = event
        {
            assert_eq!(track_id, track);
            assert_eq!(effect_id, Some(effect));
            assert_eq!(reason, "VST3 process returned failure");
            failures += 1;
        }
    }
    assert_eq!(failures, 1);
    engine.process_block(AudioProcessBlock::new(&mut [0.0; 14], 2));
    while let Ok(event) = events.pop() {
        assert!(!matches!(event, EngineEvent::DeviceProcessingFailed { .. }));
    }
}
