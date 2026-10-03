use std::path::{Path, PathBuf};

use core_foundation::bundle::CFBundle;
use core_foundation::url::CFURL;

pub(crate) fn open(path: &Path) -> Result<CFBundle, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("Invalid plugin bundle {path:?}: {e}"))?;
    let url =
        CFURL::from_path(&path, true).ok_or_else(|| format!("Invalid bundle URL: {path:?}"))?;
    CFBundle::new(url).ok_or_else(|| format!("Invalid macOS plugin bundle: {path:?}"))
}

pub(crate) fn executable(path: &Path) -> Result<PathBuf, String> {
    // CFBundleExecutable may differ from the bundle name, and Contents/MacOS
    // can also contain helper executables. Let CoreFoundation resolve it.
    let executable = open(path)?
        .executable_url()
        .and_then(|url| url.to_path())
        .ok_or_else(|| format!("No executable declared by plugin bundle: {path:?}"))?;
    if !executable.is_file() {
        return Err(format!(
            "Plugin bundle executable is missing: {executable:?}"
        ));
    }
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_declared_executable_instead_of_bundle_name_or_helper() {
        let root = std::env::temp_dir().join(format!("vibez-bundle-{}.clap", std::process::id()));
        std::fs::create_dir_all(root.join("Contents/MacOS")).unwrap();
        std::fs::write(root.join("Contents/MacOS/Helper"), []).unwrap();
        std::fs::write(root.join("Contents/MacOS/ActualPlugin"), []).unwrap();
        std::fs::write(
            root.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleExecutable</key><string>ActualPlugin</string>
<key>CFBundlePackageType</key><string>BNDL</string></dict></plist>"#,
        )
        .unwrap();
        let resolved = executable(&root).unwrap();
        assert_eq!(
            resolved,
            root.canonicalize()
                .unwrap()
                .join("Contents/MacOS/ActualPlugin")
        );
        std::fs::remove_file(&resolved).unwrap();
        assert!(executable(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
