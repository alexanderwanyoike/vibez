use vibez_core::routing::ExternalInputBlock;
use vibez_plugin_host::instance::PluginInstance;

fn configure(plugin: &mut dyn PluginInstance, settings: &[(&str, f32)]) {
    let parameters = plugin.param_descriptors_vec();
    for &(name, value) in settings {
        let index = parameters
            .iter()
            .position(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("{} has no parameter {name}", plugin.name()));
        assert!(plugin.set_param(index, value));
    }
}

fn steady_level(plugin: &mut dyn PluginInstance, trigger: f32) -> f32 {
    let input = plugin
        .external_inputs()
        .iter()
        .find(|input| input.supported())
        .unwrap()
        .clone();
    let samples = vec![trigger; 64 * input.channels];
    let mut level = 0.0;
    for _ in 0..750 {
        let mut audio = [0.001; 128];
        plugin.process_with_inputs(
            &mut audio,
            2,
            &[ExternalInputBlock {
                id: input.id,
                channels: input.channels,
                samples: &samples,
                connected: true,
            }],
        );
        level = audio.iter().map(|sample| sample.abs()).sum::<f32>() / audio.len() as f32;
    }
    level
}

fn measure(
    mut plugin: Box<dyn PluginInstance>,
    settings: &[(&str, f32)],
    gate: bool,
    state_dir: &std::path::Path,
) {
    configure(plugin.as_mut(), settings);
    let silent = steady_level(plugin.as_mut(), 0.0);
    let triggered = steady_level(plugin.as_mut(), 1.0);
    let recovered = steady_level(plugin.as_mut(), 0.0);
    println!("{}: rate=48000 block=64 settings={settings:?} silent={silent:.9} trigger={triggered:.9} recovered={recovered:.9}",plugin.name());
    assert!(silent.is_finite() && triggered.is_finite());
    if gate {
        assert!(triggered > silent * 10.0 && triggered > 0.0005);
    } else {
        assert!(triggered < silent * 0.5 && silent > 0.0005);
    }
    assert!(
        (recovered - silent).abs() < 0.0001,
        "Detector failed to recover"
    );
    plugin.stop_processing();
    if let Some(state) = plugin.save_state() {
        std::fs::write(
            state_dir.join(format!("{}.state", plugin.name().replace(['/', '\\'], "_"))),
            state,
        )
        .unwrap();
    }
    plugin.deactivate();
}

#[test]
#[ignore = "Requires VIBEZ_LSP_CLAP and VIBEZ_ZL_VST3 installed plugin paths"]
fn independently_installed_effects_respond_to_external_audio() {
    let lsp = std::env::var_os("VIBEZ_LSP_CLAP").expect("Set VIBEZ_LSP_CLAP");
    let zl = std::env::var_os("VIBEZ_ZL_VST3").expect("Set VIBEZ_ZL_VST3");
    let state_dir = std::env::var_os("VIBEZ_PLUGIN_VALIDATION_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("vibez-plugin-routing-validation"));
    std::fs::create_dir_all(&state_dir).unwrap();
    for id in [
        "in.lsp-plug.sc_compressor_mono",
        "in.lsp-plug.sc_compressor_stereo",
        "in.lsp-plug.sc_gate_stereo",
    ] {
        let plugin = vibez_plugin_host::clap_host::instance::ClapPluginInstance::load(
            std::path::Path::new(&lsp),
            id,
            false,
            48000.0,
            64,
        )
        .unwrap();
        let version = unsafe { std::ffi::CStr::from_ptr((*(*plugin.plugin_ptr()).desc).version) }
            .to_string_lossy();
        println!("{id} descriptor_version={version}");
        let gate = id.contains("gate");
        let mut settings = vec![
            ("Sidechain mode", 0.0),
            ("Sidechain lookahead", 0.0),
            ("Sidechain listen", 0.0),
            ("Sidechain reactivity", 0.0),
            ("High-pass filter mode", 0.0),
            ("Low-pass filter mode", 0.0),
            ("Makeup gain", 0.0),
            ("Dry/Wet balance", 1.0),
        ];
        if gate {
            settings.extend([
                ("Sidechain input", 0.5),
                ("Curve threshold", -20.0),
                ("Attack", 0.0),
                ("Release", 0.0),
                ("Reduction", -60.0),
            ]);
        } else {
            settings.extend([
                ("Sidechain type", 2.0 / 3.0),
                ("Attack threshold", -20.0),
                ("Attack time", 0.0),
                ("Release time", 0.0),
                ("Ratio", 0.5),
            ]);
        }
        measure(Box::new(plugin), &settings, gate, &state_dir);
    }
    let module_info = std::fs::read_to_string(
        std::path::Path::new(&zl).join("Contents/Resources/moduleinfo.json"),
    )
    .unwrap();
    let version = module_info
        .lines()
        .find(|line| line.trim_start().starts_with("\"Version\""))
        .unwrap();
    println!("ZL module_version={version}");
    let plugin = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load(
        std::path::Path::new(&zl),
        "ABCDEF01-9182FAEB-5A6C6975-436F6D70",
        false,
        48000.0,
        64,
    )
    .unwrap();
    measure(
        Box::new(plugin),
        &[
            ("External Side", 1.0),
            ("Side Out", 0.0),
            ("Threshold (dB)", 0.5),
            ("Ratio", 0.5),
            ("Attack", 0.0),
            ("Release", 0.0),
            ("Lookahead", 0.5),
            ("Oversample", 0.0),
            ("Makeup Gain", 0.5),
            ("Wet", 1.0),
        ],
        false,
        &state_dir,
    );
}
