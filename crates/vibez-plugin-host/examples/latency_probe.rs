use std::path::Path;

use vibez_plugin_host::clap_host::instance::ClapPluginInstance;
use vibez_plugin_host::vst3_host::instance::Vst3PluginInstance;
use vibez_plugin_host::PluginInstance;

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("Usage: latency_probe clap|vst3 PATH ID [PARAM_INDEX VALUE]".into());
    }
    let rate = 48000.0;
    let block = 512;
    let mut plugin: Box<dyn PluginInstance> = match args[0].as_str() {
        "clap" => Box::new(ClapPluginInstance::load(
            Path::new(&args[1]),
            &args[2],
            false,
            rate,
            block,
        )?),
        "vst3" => Box::new(Vst3PluginInstance::load(
            Path::new(&args[1]),
            &args[2],
            false,
            rate,
            block,
        )?),
        _ => return Err("Unknown plugin format".into()),
    };
    println!("Device: {} at {rate} Hz, max block {block}", plugin.name());
    for (index, descriptor) in plugin.param_descriptors_vec().iter().enumerate() {
        println!(
            "{index}: {} [{}..{}] = {}",
            descriptor.name,
            descriptor.min,
            descriptor.max,
            plugin.get_param(index)
        );
    }
    if args.len() == 5 {
        let index = args[3]
            .parse::<usize>()
            .map_err(|error| error.to_string())?;
        let value = args[4].parse::<f32>().map_err(|error| error.to_string())?;
        if !plugin.set_param(index, value) {
            return Err("Parameter was rejected".into());
        }
    }
    let mut silent = [0.0; 1024];
    for _ in 0..32 {
        plugin.process_audio(&mut silent, 2);
        silent.fill(0.0);
    }
    vibez_plugin_host::poll_clap_events();
    if plugin.reconfiguration_requested() {
        plugin.stop_for_reconfiguration();
        plugin.reconfigure_on_main_thread()?;
    }
    println!(
        "Reported processing latency: {} samples",
        plugin.latency_samples()
    );
    let mut first = None;
    let mut peak = (0usize, 0.0f32);
    for iteration in 0..128 {
        let mut samples = [0.0; 1024];
        if iteration == 0 {
            samples[0] = 0.0001;
            samples[1] = 0.0001;
        }
        plugin.process_audio(&mut samples, 2);
        for (frame, pair) in samples.chunks_exact(2).enumerate() {
            let magnitude = pair[0].abs().max(pair[1].abs());
            let position = iteration * 512 + frame;
            if magnitude > 1e-8 {
                first.get_or_insert(position);
            }
            if magnitude > peak.1 {
                peak = (position, magnitude);
            }
        }
    }
    println!(
        "Observed first nonzero sample: {first:?}; peak: {} at sample {}",
        peak.1, peak.0
    );
    plugin.stop_processing();
    Ok(())
}
