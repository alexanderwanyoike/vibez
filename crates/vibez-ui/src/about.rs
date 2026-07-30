//! What this build of vibez is, and where it came from.
//!
//! Both facts are fixed at compile time: the version by Cargo, the commit
//! by `build.rs`. Nothing here consults the filesystem or the environment
//! at runtime, so a binary that has been copied, packaged or renamed still
//! reports the source tree it was actually built from.

pub const APP_NAME: &str = "vibez";

/// Matches the LICENSE file at the repository root and the `license` field
/// in the workspace manifest; all three must be changed together.
pub const LICENSE: &str = "GPL-3.0-or-later";

/// What `build.rs` emits when `git` could not identify the build tree.
pub const UNKNOWN_COMMIT: &str = "unknown";

pub const REPOSITORY_URL: &str = "https://github.com/alexanderwanyoike/vibez";
pub const WEBSITE_URL: &str = "https://alexanderwanyoike.github.io/vibez/";
pub const RELEASES_URL: &str = "https://github.com/alexanderwanyoike/vibez/releases";

/// The version string shown in the About dialog.
///
/// An unresolvable commit is dropped rather than printed: "0.1.2 (unknown)"
/// reads to a user as something broken, while a bare "0.1.2" reads as a
/// release build, which is precisely what a tarball or packaged build is.
pub fn version_line(version: &str, commit: &str) -> String {
    let commit = commit.trim();
    if commit.is_empty() || commit == UNKNOWN_COMMIT {
        version.to_string()
    } else {
        format!("{version} ({commit})")
    }
}

/// The running build's version line.
pub fn build_version_line() -> String {
    version_line(env!("CARGO_PKG_VERSION"), env!("VIBEZ_GIT_HASH"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_names_the_commit_a_build_came_from() {
        assert_eq!(version_line("0.1.2", "a1b2c3d"), "0.1.2 (a1b2c3d)");
    }

    #[test]
    fn a_commit_no_build_script_could_resolve_is_dropped_not_shown() {
        assert_eq!(version_line("0.1.2", UNKNOWN_COMMIT), "0.1.2");
        assert_eq!(version_line("0.1.2", ""), "0.1.2");
        assert_eq!(version_line("0.1.2", "   "), "0.1.2");
    }

    #[test]
    fn the_version_line_survives_the_whitespace_git_output_carries() {
        assert_eq!(version_line("0.1.2", " a1b2c3d\n"), "0.1.2 (a1b2c3d)");
    }

    #[test]
    fn the_running_build_reports_its_own_cargo_version() {
        assert!(build_version_line().starts_with(env!("CARGO_PKG_VERSION")));
    }
}
