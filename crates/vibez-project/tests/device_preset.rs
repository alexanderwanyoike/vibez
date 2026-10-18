//! User preset files retain versioned sound settings across save and reopen.

use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
use vibez_core::effect::EffectType;
use vibez_project::device_preset::{
    load_device_preset, save_device_preset, MAX_DEVICE_PRESET_BYTES,
};

fn preset_scratch() -> tempfile::TempDir {
    let scratch = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(scratch).unwrap();
    tempfile::Builder::new()
        .prefix("device-preset-")
        .tempdir_in(scratch)
        .unwrap()
}

fn gain_preset(name: &str, gain: f32) -> DevicePreset {
    DevicePreset::new(
        DevicePresetKind::Effect(EffectType::Gain),
        name,
        [("gain".to_string(), gain)].into_iter().collect(),
    )
    .unwrap()
}

#[test]
fn saved_device_preset_reopens_with_the_same_sound_settings() {
    let directory = preset_scratch();
    let path = directory.path().join("quiet-reference.vzpreset");
    let preset = gain_preset("Quiet reference", 0.5);

    save_device_preset(&path, &preset).unwrap();

    assert_eq!(load_device_preset(&path).unwrap(), preset);
}

#[test]
fn oversized_valid_json_is_rejected_at_the_file_boundary() {
    let directory = preset_scratch();
    let path = directory.path().join("oversized.vzpreset");
    let preset = gain_preset("Reference", 1.0);
    let mut bytes = serde_json::to_vec(&preset).unwrap();
    bytes.resize(MAX_DEVICE_PRESET_BYTES as usize, b' ');
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(load_device_preset(&path).unwrap(), preset);

    bytes.push(b' ');
    std::fs::write(&path, bytes).unwrap();

    assert_eq!(
        load_device_preset(&path).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn malformed_preset_file_reports_invalid_data() {
    let directory = preset_scratch();
    let path = directory.path().join("unfinished.vzpreset");
    std::fs::write(&path, br#"{"format_version":1,"parameters":{"gain":0.5}"#).unwrap();

    assert_eq!(
        load_device_preset(&path).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn unsupported_preset_version_is_rejected_when_opening_a_file() {
    let directory = preset_scratch();
    let path = directory.path().join("future.vzpreset");
    let mut document = serde_json::to_value(gain_preset("Reference", 1.0)).unwrap();
    document["format_version"] = serde_json::json!(2);
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

    assert_eq!(
        load_device_preset(&path).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn saving_over_an_existing_preset_publishes_complete_replacement() {
    let directory = preset_scratch();
    let path = directory.path().join("reference.vzpreset");
    save_device_preset(&path, &gain_preset("Quiet reference", 0.5)).unwrap();
    let replacement = gain_preset("Unity reference", 1.0);

    save_device_preset(&path, &replacement).unwrap();

    assert_eq!(load_device_preset(&path).unwrap(), replacement);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn failed_publication_preserves_destination_contents_and_removes_staging() {
    let directory = preset_scratch();
    let destination = directory.path().join("existing");
    std::fs::create_dir(&destination).unwrap();
    let existing_file = destination.join("keep.vzpreset");
    let original = gain_preset("Original", 0.25);
    save_device_preset(&existing_file, &original).unwrap();

    assert!(save_device_preset(&destination, &gain_preset("Replacement", 1.0)).is_err());

    assert_eq!(load_device_preset(&existing_file).unwrap(), original);
    assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 1);
    let remaining: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(remaining, vec![destination]);
}
