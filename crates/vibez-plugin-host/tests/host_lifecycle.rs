mod support;

use vibez_plugin_host::PluginInstance;

#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

fn state(actual: u32, reported: u32, flags: u32) -> Vec<u8> {
    [actual, reported, flags]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
}

fn load(
    fixture: &support::Fixture,
    format: &str,
    bytes: &[u8],
    strict: bool,
) -> Result<Box<dyn PluginInstance>, String> {
    if format == "clap" {
        use vibez_plugin_host::clap_host::instance::ClapPluginInstance;
        let partial =
            ClapPluginInstance::load_partial(&fixture.clap, vibez_routing_fixture::CLAP_ID, false)?;
        ClapPluginInstance::init_on_main_thread_with_state(
            partial,
            48000.0,
            64,
            Some(bytes),
            strict,
        )
        .map(|plugin| Box::new(plugin) as Box<dyn PluginInstance>)
    } else {
        use vibez_plugin_host::vst3_host::instance::Vst3PluginInstance;
        let info = vibez_plugin_host::vst3_host::scanner::scan_vst3(&fixture.vst3)?;
        let partial = Vst3PluginInstance::load_partial(&fixture.vst3, &info[0].id.uid, false)?;
        Vst3PluginInstance::init_on_main_thread_with_state(
            partial,
            48000.0,
            64,
            Some(bytes),
            strict,
        )
        .map(|plugin| Box::new(plugin) as Box<dyn PluginInstance>)
    }
}

fn audit(fixture: &support::Fixture, format: &str) -> libloading::Library {
    unsafe {
        libloading::Library::new(if format == "clap" {
            &fixture.clap
        } else {
            &fixture.module
        })
        .unwrap()
    }
}
fn count(audit: &libloading::Library, symbol: &[u8]) -> u32 {
    unsafe { audit.get::<unsafe extern "C" fn() -> u32>(symbol).unwrap()() }
}

#[test]
fn saved_state_precedes_first_activation_and_activation_notifications_do_not_loop() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let audit = audit(&fixture, format);
        let before = count(&audit, b"fixture_activations\0");
        let mut plugin = load(&fixture, format, &state(137, 137, 1 << 19), true).unwrap();
        assert_eq!(plugin.latency_samples(), 137);
        assert_eq!(count(&audit, b"fixture_activations\0"), before + 1);
        assert!(!plugin.reconfiguration_requested());
        plugin.reconfigure_on_main_thread().unwrap();
        assert_eq!(count(&audit, b"fixture_activations\0"), before + 2);
        assert!(
            !plugin.reconfiguration_requested(),
            "{format} activation notification loop"
        );
    }
}

#[test]
fn rejected_saved_state_is_strict_or_uses_only_one_default_activation() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let audit = audit(&fixture, format);
        let before = count(&audit, b"fixture_activations\0");
        let invalid = state(4097, 137, 0);
        let error = load(&fixture, format, &invalid, true).err().unwrap();
        assert!(error.contains("rejected its saved state"), "{error}");
        assert_eq!(count(&audit, b"fixture_activations\0"), before);
        let plugin = load(&fixture, format, &invalid, false).unwrap();
        assert_eq!(plugin.latency_samples(), 0);
        assert_eq!(count(&audit, b"fixture_activations\0"), before + 1);
        drop(plugin);
        assert_eq!(count(&audit, b"fixture_lifecycle_errors\0"), 0);
    }
}

#[test]
fn refused_optional_vst_handler_preserves_compatible_load_and_dsp() {
    let fixture = support::Fixture::new();
    let plugins = vibez_plugin_host::vst3_host::scanner::scan_vst3(&fixture.vst3).unwrap();
    let mut plugin = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load(
        &fixture.vst3,
        &plugins[7].id.uid,
        false,
        48000.0,
        64,
    )
    .unwrap();
    let mut output = [1.0; 128];
    assert_eq!(
        support::allocation::count_allocations(|| plugin.process_audio(&mut output, 2)),
        0
    );
    assert_eq!(output, [1.0; 128]);
}

#[test]
fn start_failure_has_retained_static_cause_and_silent_allocation_free_output() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut plugin = load(&fixture, format, &state(0, 0, 1 << 18), true).unwrap();
        let mut output = [1.0; 128];
        assert_eq!(
            support::allocation::count_allocations(|| plugin.process_audio(&mut output, 2)),
            0
        );
        assert_eq!(output, [0.0; 128]);
        assert!(!plugin.processing_configuration_valid());
        assert_eq!(
            plugin.take_processing_error(),
            Some(if format == "clap" {
                "CLAP start_processing failed"
            } else {
                "VST3 setProcessing(true) failed"
            })
        );
        assert_eq!(plugin.take_processing_error(), None);
        plugin.load_state(&state(0, 0, 0));
        plugin.reconfigure_on_main_thread().unwrap();
        plugin.process_audio(&mut [1.0; 128], 2);
        assert!(plugin.processing_configuration_valid());
    }
}

#[test]
fn exclusive_main_teardown_stops_processing_and_clap_callback_dispatch_restores_main_role() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let audit = audit(&fixture, format);
        let before = count(&audit, b"fixture_main_callbacks\0");
        let plugin = load(&fixture, format, &state(0, 0, 1 << 20), true).unwrap();
        let plugin = std::thread::spawn(move || {
            let mut plugin = plugin;
            let mut output = [1.0; 128];
            assert_eq!(
                support::allocation::count_allocations(|| plugin.process_audio(&mut output, 2)),
                0
            );
            if format == "clap" {
                vibez_plugin_host::clap_host::host_impl::poll_clap_events();
            }
            plugin
        })
        .join()
        .unwrap();
        assert_eq!(count(&audit, b"fixture_main_callbacks\0"), before);
        if format == "clap" {
            vibez_plugin_host::clap_host::host_impl::poll_clap_events();
            assert_eq!(count(&audit, b"fixture_main_callbacks\0"), before + 1);
        }
        // The joined worker cannot retain an in-flight process call at this boundary.
        drop(plugin);
        assert_eq!(count(&audit, b"fixture_lifecycle_errors\0"), 0, "{format}");
        vibez_plugin_host::clap_host::host_impl::poll_clap_events();
    }
}

#[test]
fn vst_io_changed_refreshes_ports_and_reactivates_declared_event_buses() {
    let fixture = support::Fixture::new();
    let mut plugin = fixture.load("vst3", 64);
    plugin.load_state(&state(0, 0, (1 << 21) | (1 << 16)));
    assert!(plugin.reconfiguration_requested());
    plugin.reconfigure_on_main_thread().unwrap();
    assert_eq!(plugin.external_inputs()[0].channels, 2);
    let audit = audit(&fixture, "vst3");
    let before = count(&audit, b"fixture_event_activations\0");
    let mut instrument = fixture.load_instrument("vst3", 64);
    assert_eq!(count(&audit, b"fixture_event_activations\0"), before + 1);
    instrument.load_state(&state(137, 137, 0));
    instrument.reconfigure_on_main_thread().unwrap();
    assert_eq!(count(&audit, b"fixture_event_activations\0"), before + 2);
    instrument.note_on_at(60, 100, 0);
    for _ in 0..3 {
        instrument.process_audio(&mut [0.0; 128], 2);
    }
}

#[test]
fn sequential_worker_migration_preserves_delay_history_and_fatal_failure_latching() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let audit = audit(&fixture, format);
        let mut plugin = load(&fixture, format, &state(137, 137, 0), true).unwrap();
        let mut first = [0.0; 128];
        first[..2].copy_from_slice(&[1.0, -0.5]);
        plugin.process_audio(&mut first, 2);
        assert_eq!(first, [0.0; 128]);
        for index in 0..2 {
            plugin = std::thread::spawn(move || {
                let mut output = [0.0; 128];
                assert_eq!(
                    support::allocation::count_allocations(|| plugin.process_audio(&mut output, 2)),
                    0
                );
                for (frame, sample) in output.chunks_exact(2).enumerate() {
                    assert_eq!(
                        sample,
                        if index == 1 && frame == 9 {
                            &[1.0, -0.5]
                        } else {
                            &[0.0, 0.0]
                        }
                    );
                }
                plugin
            })
            .join()
            .unwrap();
        }
        assert!(plugin.processing_configuration_valid());
        plugin.set_audio_context(vibez_core::audio_context::DeviceAudioContext {
            sample_rate: 96000,
            musical_sample: 0,
            continuous_sample: 0,
            bpm: 120.0,
            playing: true,
        });
        plugin = std::thread::spawn(move || {
            assert!(!plugin.processing_configuration_valid());
            let mut output = [1.0; 128];
            plugin.process_audio(&mut output, 2);
            assert_eq!(output, [0.0; 128]);
            plugin
        })
        .join()
        .unwrap();
        assert!(!plugin.processing_configuration_valid());
        drop(plugin);
        assert_eq!(count(&audit, b"fixture_lifecycle_errors\0"), 0);
    }
}

#[test]
fn concurrent_clap_restart_request_survives_activation_without_an_announcement_loop() {
    let fixture = support::Fixture::new();
    let mut plugin = load(
        &fixture,
        "clap",
        &state(137, 137, (1 << 19) | (1 << 23)),
        true,
    )
    .unwrap();
    assert!(
        plugin.reconfiguration_requested(),
        "external producer request must remain queued"
    );
    plugin.reconfigure_on_main_thread().unwrap();
    assert_eq!(plugin.latency_samples(), 137);
    assert!(
        !plugin.reconfiguration_requested(),
        "synchronous announcement must not loop"
    );
    plugin.load_state(&state(521, 521, (1 << 19) | (1 << 23)));
    plugin.reconfigure_on_main_thread().unwrap();
    assert!(
        plugin.reconfiguration_requested(),
        "mid-restart producer request must remain queued"
    );
    assert_eq!(plugin.latency_samples(), 521);
    plugin.reconfigure_on_main_thread().unwrap();
    assert!(!plugin.reconfiguration_requested());
}
