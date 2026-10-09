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
