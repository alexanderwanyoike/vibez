//! Native dialogs and off-thread file I/O for portable device presets.

use super::run_off_ui_thread;
use std::path::PathBuf;
use vibez_core::device_preset::DevicePreset;

pub(in crate::app) async fn load_device_preset_dialog() -> Result<Option<DevicePreset>, String> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Load Device Preset")
        .add_filter("Vibez Device Preset", &["vzpreset"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_path_buf();
    run_off_ui_thread("Load device preset", move || {
        vibez_project::device_preset::load_device_preset(&path)
    })
    .await?
    .map(Some)
    .map_err(|error| error.to_string())
}

pub(in crate::app) async fn save_device_preset_dialog(
    preset: DevicePreset,
) -> Result<Option<DevicePreset>, String> {
    let file_name: String = preset
        .name()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Save Device Preset")
        .set_file_name(format!("{file_name}.vzpreset"))
        .add_filter("Vibez Device Preset", &["vzpreset"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    save_device_preset_at(file.path().to_path_buf(), preset)
        .await
        .map(Some)
}

pub(in crate::app) async fn save_device_preset_at(
    mut path: PathBuf,
    preset: DevicePreset,
) -> Result<DevicePreset, String> {
    run_off_ui_thread("Save device preset", move || {
        if path.extension().is_none() {
            path.set_extension("vzpreset");
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| "The preset file name must be valid text".to_string())?;
        let saved = DevicePreset::new(preset.kind(), name, preset.parameters().clone())
            .map_err(|error| error.to_string())?;
        vibez_project::device_preset::save_device_preset(&path, &saved)
            .map_err(|error| error.to_string())?;
        Ok(saved)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::effect::EffectType;

    #[tokio::test]
    async fn saving_without_an_extension_uses_the_file_name_as_preset_name() {
        let directory = tempfile::Builder::new()
            .prefix("vibez-device-preset-")
            .tempdir_in(std::env::current_dir().unwrap())
            .unwrap();
        let preset = crate::domains::device_presets::factory_presets(EffectType::Gain)[1].clone();

        let saved = save_device_preset_at(directory.path().join("Quiet bass"), preset.clone())
            .await
            .unwrap();
        let loaded = vibez_project::device_preset::load_device_preset(
            &directory.path().join("Quiet bass.vzpreset"),
        )
        .unwrap();

        assert_eq!(loaded, saved);
        assert_eq!(loaded.name(), "Quiet bass");
        assert_eq!(loaded.kind(), preset.kind());
        assert_eq!(loaded.parameters(), preset.parameters());
    }
}
