//! Headless VST3 host probe: loads a plugin, reports its buses,
//! pushes audio through process_audio, and checks whether the plugin
//! actually processes (output differs from input / reverb tail decays)
//! plus whether createView works. Everything runs on the main thread,
//! matching the DAW's post-two-phase thread model.
//!
//!   cargo run -p vibez-plugin-host --example vst3_probe -- <bundle.vst3> [uid]

use std::path::PathBuf;

use vibez_plugin_host::vst3_host::instance::Vst3PluginInstance;
use vibez_plugin_host::PluginInstance;

fn rms(buf: &[f32]) -> f32 {
    (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().expect("usage: vst3_probe <bundle.vst3> [uid]"));
    let uid = args.next();

    // Discover class uid via the scanner if not given.
    let uid = uid.unwrap_or_else(|| {
        let infos = vibez_plugin_host::vst3_host::scanner::scan_vst3(&path).expect("scan failed");
        let info = infos.first().expect("no audio classes in bundle");
        println!("class: {} uid={}", info.name, info.id.uid);
        info.id.uid.clone()
    });

    let mut inst =
        Vst3PluginInstance::load(&path, &uid, false, 48_000.0, 512).expect("load failed");
    println!("loaded: {}", inst.name());

    // ── audio probe ──
    const FRAMES: usize = 512;
    const CH: usize = 2;
    let mut had_tail = false;
    let mut altered = false;
    for block in 0..12 {
        let mut buf = vec![0.0f32; FRAMES * CH];
        let feeding = block < 6;
        if feeding {
            for (i, frame) in buf.chunks_mut(CH).enumerate() {
                let t = (block * FRAMES + i) as f32 / 48_000.0;
                let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
                frame[0] = s;
                frame[1] = s;
            }
        }
        let input_rms = rms(&buf);
        let input_copy = buf.clone();
        inst.process_audio(&mut buf, CH);
        let output_rms = rms(&buf);
        if buf != input_copy {
            altered = true;
        }
        if !feeding && output_rms > 1e-6 {
            had_tail = true;
        }
        println!(
            "block {block:2} {} in_rms={input_rms:.4} out_rms={output_rms:.4}{}",
            if feeding { "feed   " } else { "silence" },
            if buf == input_copy {
                "  (passthrough)"
            } else {
                ""
            },
        );
    }
    println!("processing altered audio: {altered}");
    println!("tail after input stopped: {had_tail}");

    // ── createView probe ──
    let ctrl = inst.controller_ptr();
    if ctrl.is_null() {
        println!("createView: NO CONTROLLER");
    } else {
        type CreateViewFn =
            unsafe extern "system" fn(*mut std::ffi::c_void, *const u8) -> *mut std::ffi::c_void;
        unsafe {
            let vtbl = *(ctrl as *const *const *const std::ffi::c_void);
            let create_view: CreateViewFn = std::mem::transmute(*vtbl.add(17));
            let view = create_view(ctrl, b"editor\0".as_ptr());
            println!("createView(\"editor\") -> {view:p}");
            if !view.is_null() {
                type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
                let vvtbl = *(view as *const *const *const std::ffi::c_void);
                let release: ReleaseFn = std::mem::transmute(*vvtbl.add(2));
                release(view);
            }
        }
    }

    drop(inst);
    println!("clean teardown OK");
}
