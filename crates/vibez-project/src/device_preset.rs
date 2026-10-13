//! Device preset files preserve portable builtin sound settings.

use std::io;
use std::path::Path;

use vibez_core::device_preset::DevicePreset;

pub fn load_device_preset(_path: &Path) -> io::Result<DevicePreset> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Device preset loading is not implemented",
    ))
}

pub fn save_device_preset(_path: &Path, _preset: &DevicePreset) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Device preset saving is not implemented",
    ))
}
