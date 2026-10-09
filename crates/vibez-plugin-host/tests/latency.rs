mod support;

#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

fn state(actual: u32, reported: u32, probe: bool) -> Vec<u8> {
    [actual, reported, u32::from(probe)]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
}

fn impulse(instance: &mut dyn vibez_plugin_host::PluginInstance, expected: usize) {
    instance.reset();
    let mut rendered = Vec::new();
    for (block, frames) in [1, 7, 31, 64].into_iter().cycle().take(32).enumerate() {
        let mut main = vec![0.0; frames * 2];
        if block == 0 {
            main[0] = 1.0;
            main[1] = -0.5;
        }
        let allocations =
            support::allocation::count_allocations(|| instance.process_audio(&mut main, 2));
        assert_eq!(allocations, 0);
        rendered.extend(main);
    }
    for (frame, values) in rendered.chunks_exact(2).enumerate() {
        assert_eq!(
            values,
            if frame == expected {
                &[1.0, -0.5]
            } else {
                &[0.0, 0.0]
            },
            "frame {frame}"
        );
    }
}

#[test]
fn loadable_formats_cache_latency_after_activation_and_measure_exact_delays() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load(format, 64);
        assert_eq!(instance.latency_samples(), 0);
        assert!(instance.load_state(&state(137, 137, false)));
        assert!(instance.reconfiguration_requested(), "{format}");
        assert_eq!(instance.latency_samples(), 0);
        instance.stop_for_reconfiguration();
        instance.reconfigure_on_main_thread().unwrap();
        assert_eq!(instance.latency_samples(), 137);
        assert!(!instance.reconfiguration_requested());
        impulse(instance.as_mut(), 137);

        for delay in [521, 137] {
            assert!(instance.load_state(&state(delay, delay, false)));
            assert!(instance.reconfiguration_requested());
            assert!(
                instance.reconfigure_on_main_thread().is_err(),
                "processing must stop before activation"
            );
            instance.stop_for_reconfiguration();
            instance.reconfigure_on_main_thread().unwrap();
            assert_eq!(instance.latency_samples(), delay);
            impulse(instance.as_mut(), delay as usize);
        }
        instance.stop_processing();
    }
}

#[test]
fn loadable_adapter_rejects_latency_reactivation_on_an_unrelated_thread() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load(format, 64);
        instance = std::thread::spawn(move || {
            assert!(instance.reconfigure_on_main_thread().is_err());
            instance
        })
        .join()
        .unwrap();
        assert!(instance.load_state(&state(137, 137, false)));
        instance.reconfigure_on_main_thread().unwrap();
        assert_eq!(instance.latency_samples(), 137);
    }
}

#[test]
fn loadable_instrument_reports_and_applies_its_processing_delay() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load_instrument(format, 64);
        assert!(instance.load_state(&state(137, 137, false)));
        instance.reconfigure_on_main_thread().unwrap();
        assert_eq!(instance.latency_samples(), 137);
        instance.note_on_at(60, 100, 7);
        let mut rendered = Vec::new();
        for _ in 0..4 {
            let mut block = [0.0; 128];
            instance.process_audio(&mut block, 2);
            rendered.extend(block);
        }
        for (frame, values) in rendered.chunks_exact(2).enumerate() {
            assert_eq!(
                values,
                if frame < 144 {
                    &[0.0, 0.0]
                } else {
                    &[0.75, 0.75]
                }
            );
        }
        instance.stop_processing();
    }
}

#[test]
fn loadable_formats_receive_target_audio_context_at_varied_rates() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        for rate in [44100, 48000, 96000] {
            let mut instance = fixture.load_at_rate(format, 64, rate as f64);
            let bytes: Vec<u8> = [0u32, 0, 2]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            assert!(instance.load_state(&bytes));
            instance.reconfigure_on_main_thread().unwrap();
            instance.set_audio_context(vibez_core::audio_context::DeviceAudioContext {
                musical_sample: 800,
                continuous_sample: 4000,
                sample_rate: rate,
                bpm: 123.0,
                playing: true,
            });
            let mut output = [0.0; 62];
            instance.process_audio(&mut output, 2);
            for (frame, sample) in output.chunks_exact(2).enumerate() {
                assert_eq!(sample, &[(800 + frame) as f32; 2]);
            }
            instance.stop_processing();
        }
    }
}

#[test]
fn format_restart_refreshes_input_metadata_while_deactivated() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load(format, 64);
        assert_eq!(instance.external_inputs()[0].channels, 1);
        let bytes: Vec<u8> = [137u32, 137, 1 << 16]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(instance.load_state(&bytes));
        instance.reconfigure_on_main_thread().unwrap();
        assert_eq!(instance.external_inputs()[0].channels, 2);
        assert_eq!(instance.latency_samples(), 137);
        impulse(instance.as_mut(), 137);
        instance.stop_processing();
    }
}

#[test]
fn loadable_latency_restart_stops_on_the_processing_thread_and_destroys_on_main() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let module = if format == "clap" {
            fixture.clap.clone()
        } else {
            fixture.module.clone()
        };
        let audit = unsafe { libloading::Library::new(module).unwrap() };
        let errors = unsafe {
            audit
                .get::<unsafe extern "C" fn() -> u32>(b"fixture_lifecycle_errors\0")
                .unwrap()
        };
        let mut instance = fixture.load(format, 64);
        instance.load_state(&state(137, 137, false));
        instance.reconfigure_on_main_thread().unwrap();
        for delay in [521, 137] {
            instance = std::thread::spawn(move || {
                let mut block = [0.0; 128];
                instance.process_audio(&mut block, 2);
                instance.stop_for_reconfiguration();
                instance
            })
            .join()
            .unwrap();
            instance.load_state(&state(delay, delay, false));
            instance.reconfigure_on_main_thread().unwrap();
        }
        drop(instance);
        assert_eq!(
            unsafe { errors() },
            0,
            "{format} stop/destroy thread violation"
        );
    }
}

#[test]
fn failed_reactivation_invalidates_cached_processing_even_when_latency_still_matches() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load(format, 64);
        instance.load_state(&state(137, 137, false));
        instance.reconfigure_on_main_thread().unwrap();
        assert!(instance.processing_configuration_valid());
        let fail: Vec<_> = [137u32, 137, 1 << 17]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        instance.load_state(&fail);
        instance.stop_for_reconfiguration();
        assert!(instance.reconfigure_on_main_thread().is_err());
        assert_eq!(instance.latency_samples(), 137);
        assert!(!instance.processing_configuration_valid());
        instance.load_state(&state(137, 137, false));
        instance.reconfigure_on_main_thread().unwrap();
        assert!(instance.processing_configuration_valid());
    }
}

#[test]
fn reuse_at_another_rate_or_processing_thread_fails_until_sanctioned_stop_and_reactivation() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load(format, 64);
        instance.process_audio(&mut [0.0; 128], 2);
        instance = std::thread::spawn(move || {
            assert!(!instance.processing_configuration_valid());
            let mut output = [1.0; 128];
            instance.process_audio(&mut output, 2);
            assert!(output.iter().all(|&sample| sample == 0.0));
            instance
        })
        .join()
        .unwrap();
        assert!(!instance.processing_configuration_valid());
        instance.stop_for_reconfiguration();
        instance.reconfigure_on_main_thread().unwrap();
        assert!(instance.processing_configuration_valid());
        instance.set_audio_context(vibez_core::audio_context::DeviceAudioContext {
            musical_sample: 0,
            continuous_sample: 0,
            sample_rate: 96000,
            bpm: 120.0,
            playing: true,
        });
        assert!(!instance.processing_configuration_valid());
        let mut output = [1.0; 128];
        instance.process_audio(&mut output, 2);
        assert!(output.iter().all(|&sample| sample == 0.0));
        instance.stop_for_reconfiguration();
        instance.reconfigure_on_main_thread().unwrap();
        assert!(instance.processing_configuration_valid());
    }
}

#[test]
fn preparation_cannot_relabel_an_active_device_as_activated_at_another_rate() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instance = fixture.load(format, 64);
        instance.prepare(96000.0, 64);
        assert_eq!(instance.activation_sample_rate(), Some(48000));
        assert!(!instance.processing_configuration_valid());
        instance.stop_for_reconfiguration();
        instance.reconfigure_on_main_thread().unwrap();
        assert!(instance.processing_configuration_valid());
        assert_eq!(instance.activation_sample_rate(), Some(48000));
    }
}
