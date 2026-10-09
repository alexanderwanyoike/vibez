use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Fixture {
    pub root: PathBuf,
    pub clap: PathBuf,
    pub vst3: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let deps = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let name = format!(
            "{}vibez_routing_fixture{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let binary = deps.join(name);
        assert!(
            binary.exists(),
            "Fixture library missing: {}",
            binary.display()
        );
        let root = std::env::temp_dir().join(format!(
            "vibez-route-fixture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let clap = root.join("Probe.clap");
        std::fs::copy(&binary, &clap).unwrap();
        let vst3 = root.join("Probe.vst3");
        #[cfg(target_os = "linux")]
        let module = vst3.join("Contents/x86_64-linux/Probe.so");
        #[cfg(target_os = "windows")]
        let module = vst3.join("Contents/x86_64-win/Probe.vst3");
        #[cfg(target_os = "macos")]
        let module = vst3.join("Contents/MacOS/Probe");
        std::fs::create_dir_all(module.parent().unwrap()).unwrap();
        std::fs::copy(&binary, module).unwrap();
        #[cfg(target_os="macos")]
        std::fs::write(vst3.join("Contents/Info.plist"),r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>Probe</string><key>CFBundleIdentifier</key><string>vibez.fixture.routing</string><key>CFBundlePackageType</key><string>BNDL</string></dict></plist>"#).unwrap();
        Self { root, clap, vst3 }
    }
    pub fn load(
        &self,
        format: &str,
        frames: u32,
    ) -> Box<dyn vibez_plugin_host::instance::PluginInstance> {
        if format == "clap" {
            Box::new(
                vibez_plugin_host::clap_host::instance::ClapPluginInstance::load(
                    &self.clap,
                    vibez_routing_fixture::CLAP_ID,
                    false,
                    48000.0,
                    frames,
                )
                .unwrap(),
            )
        } else {
            let plugins = vibez_plugin_host::vst3_host::scanner::scan_vst3(&self.vst3).unwrap();
            Box::new(
                vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load(
                    &self.vst3,
                    &plugins[0].id.uid,
                    false,
                    48000.0,
                    frames,
                )
                .unwrap(),
            )
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[allow(dead_code)]
pub fn path_is_fixture(path: &Path) -> bool {
    path.exists()
}
