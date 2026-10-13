//! User preset files retain versioned sound settings across save and reopen.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_project::device_preset::{load_device_preset, save_device_preset};

#[test]
fn saved_device_preset_reopens_with_the_same_sound_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("quiet-reference.vzpreset");
    let preset = DevicePreset::new(
        DevicePresetKind::Effect(EffectType::Gain),
        "Quiet reference",
        [("gain".to_string(), 0.5)].into_iter().collect(),
    )
    .unwrap();

    save_device_preset(&path, &preset).unwrap();

    assert_eq!(load_device_preset(&path).unwrap(), preset);
}
