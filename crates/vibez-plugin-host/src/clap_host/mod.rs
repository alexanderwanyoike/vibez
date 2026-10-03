pub mod host_impl;
pub mod instance;
pub mod scanner;

pub(crate) fn module_path(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "macos")]
    if path.is_dir() {
        return crate::macos_bundle::executable(path);
    }
    Ok(path.to_path_buf())
}
