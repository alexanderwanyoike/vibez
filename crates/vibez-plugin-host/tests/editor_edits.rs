//! A VST3 editor's parameter changes reach the processor through the host,
//! as plugins such as NIH-plug's require.

#[allow(dead_code)]
mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

use vibez_plugin_host::instance::PluginInstance;
use vibez_plugin_host::vst3_host::{instance::Vst3PluginInstance, scanner::scan_vst3};

const EDITOR_CLASS: usize = 8;

struct Editor(libloading::Library);

impl Editor {
    fn edit(&self, value: f64) -> i32 {
        unsafe {
            self.0
                .get::<unsafe extern "C" fn(f64) -> i32>(b"fixture_editor_edit")
                .unwrap()(value)
        }
    }
    fn preset(&self, value: f64) -> i32 {
        unsafe {
            self.0
                .get::<unsafe extern "C" fn(f64) -> i32>(b"fixture_editor_preset")
                .unwrap()(value)
        }
    }
    fn processor_value(&self) -> f64 {
        unsafe {
            self.0
                .get::<unsafe extern "C" fn() -> f64>(b"fixture_processor_value")
                .unwrap()()
        }
    }
}

fn editor_plugin(fixture: &support::Fixture) -> (Vst3PluginInstance, Editor) {
    let plugins = scan_vst3(&fixture.vst3).unwrap();
    let plugin = Vst3PluginInstance::load(
        &fixture.vst3,
        &plugins[EDITOR_CLASS].id.uid,
        false,
        48000.0,
        64,
    )
    .unwrap();
    let editor = Editor(unsafe { libloading::Library::new(&fixture.module).unwrap() });
    (plugin, editor)
}

fn process(plugin: &mut Vst3PluginInstance) {
    let mut block = [0.0f32; 128];
    plugin.process_audio(&mut block, 2);
}

fn assert_near(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "{what}: {actual}, expected {expected}"
    );
}

#[test]
fn an_editor_change_during_processing_reaches_the_processor_and_host_readback() {
    let fixture = support::Fixture::new();
    let (mut plugin, editor) = editor_plugin(&fixture);
    process(&mut plugin);
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let audio = {
        let running = running.clone();
        std::thread::spawn(move || {
            while running.load(std::sync::atomic::Ordering::Acquire) {
                process(&mut plugin);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            process(&mut plugin);
            plugin
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(editor.edit(0.65), 0, "the host accepts the edit");
    std::thread::sleep(std::time::Duration::from_millis(5));
    running.store(false, std::sync::atomic::Ordering::Release);
    let plugin = audio.join().unwrap();
    assert_near(editor.processor_value(), 0.65, "processor");
    assert_near(plugin.get_param(0) as f64, 0.65, "host readback");
}

#[test]
fn a_preset_loaded_in_the_editor_reaches_the_processor() {
    let fixture = support::Fixture::new();
    let (mut plugin, editor) = editor_plugin(&fixture);
    process(&mut plugin);
    assert_eq!(editor.preset(0.25), 0);
    process(&mut plugin);
    assert_near(editor.processor_value(), 0.25, "processor");
    assert_near(plugin.get_param(0) as f64, 0.25, "host readback");
}

#[test]
fn a_burst_of_editor_changes_arrives_as_its_last_value_without_allocating() {
    let fixture = support::Fixture::new();
    let (mut plugin, editor) = editor_plugin(&fixture);
    process(&mut plugin);
    for step in 0..=1000 {
        assert_eq!(editor.edit(step as f64 / 1000.0), 0);
    }
    let allocations = support::allocation::count_allocations(|| process(&mut plugin));
    assert_eq!(
        allocations, 0,
        "the audio thread drains edits without allocating"
    );
    assert_near(editor.processor_value(), 1.0, "processor");
}

#[test]
fn saved_state_recalls_an_editor_change() {
    let fixture = support::Fixture::new();
    let (mut plugin, editor) = editor_plugin(&fixture);
    process(&mut plugin);
    assert_eq!(editor.edit(0.8), 0);
    process(&mut plugin);
    let state = plugin.save_state().unwrap();
    let value = f64::from_le_bytes(state[12..20].try_into().unwrap());
    assert_near(value, 0.8, "saved processor state");
}
