use super::*;

#[test]
fn macos_module_entry_receives_a_bundle_and_exit_is_paired() {
    let root = std::env::temp_dir().join(format!("vibez-module-{}.vst3", std::process::id()));
    let contents = root.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    let source = root.join("fixture.rs");
    std::fs::write(&source, r#"
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
static ENTRIES: AtomicUsize = AtomicUsize::new(0);
static EXITS: AtomicUsize = AtomicUsize::new(0);
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFGetTypeID(value: *const c_void) -> usize;
    fn CFBundleGetTypeID() -> usize;
}
#[no_mangle]
pub unsafe extern "C" fn bundleEntry(bundle: *const c_void) -> bool {
    if bundle.is_null() || CFGetTypeID(bundle) != CFBundleGetTypeID() { return false; }
    ENTRIES.fetch_add(1, Ordering::SeqCst);
    true
}
#[no_mangle]
pub extern "C" fn bundleExit() -> bool { EXITS.fetch_add(1, Ordering::SeqCst); true }
#[no_mangle]
pub extern "C" fn counts() -> usize { ENTRIES.load(Ordering::SeqCst) * 10 + EXITS.load(Ordering::SeqCst) }
"#).unwrap();
    let executable = contents.join("MacOS/ActualPlugin");
    let compiler = std::process::Command::new("rustc")
        .args([
            "--crate-type",
            "cdylib",
            "--crate-name",
            "vibez_module_fixture",
        ])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiler.status.success(),
        "{}",
        String::from_utf8_lossy(&compiler.stderr)
    );
    std::fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>ActualPlugin</string>
<key>CFBundlePackageType</key><string>BNDL</string></dict></plist>"#,
    )
    .unwrap();
    assert_eq!(
        crate::macos_bundle::executable(&root).unwrap(),
        executable.canonicalize().unwrap()
    );
    unsafe {
        let observer = libloading::Library::new(&executable).unwrap();
        let counts = observer
            .get::<unsafe extern "C" fn() -> usize>(b"counts\0")
            .unwrap();
        for iteration in 1..=2 {
            let lib = libloading::Library::new(&executable).unwrap();
            let module = Vst3Module::init(lib, &root).unwrap();
            assert_eq!(counts(), iteration * 10 + iteration - 1);
            drop(module);
            assert_eq!(counts(), iteration * 11);
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}
