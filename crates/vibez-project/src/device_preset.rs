//! Device preset files preserve portable builtin sound settings.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use vibez_core::device_preset::DevicePreset;

pub const MAX_DEVICE_PRESET_BYTES: u64 = 256 * 1024;

pub fn load_device_preset(path: &Path) -> io::Result<DevicePreset> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_DEVICE_PRESET_BYTES + 1)
        .read_to_end(&mut bytes)?;
    check_size(bytes.len())?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn save_device_preset(path: &Path, preset: &DevicePreset) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(preset)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    check_size(bytes.len())?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(".vibez-preset-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    staged.write_all(&bytes)?;
    staged.flush()?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn check_size(bytes: usize) -> io::Result<()> {
    if bytes as u64 > MAX_DEVICE_PRESET_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Device preset exceeds the {MAX_DEVICE_PRESET_BYTES}-byte limit"),
        ));
    }
    Ok(())
}
