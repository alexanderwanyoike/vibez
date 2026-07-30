//! Bakes the short commit of the build tree into the binary so the About
//! dialog can name the exact source a build came from.
//!
//! Release tarballs, distro packages and crates.io builds ship without a
//! `.git` directory, and some build machines have no `git` at all. Every
//! failure path here therefore degrades to `UNKNOWN_COMMIT` instead of
//! aborting the build: a missing commit is a cosmetic loss in one dialog,
//! never a reason for vibez to fail to compile.

use std::path::PathBuf;
use std::process::Command;

/// Must stay in step with `crate::about::UNKNOWN_COMMIT`; a build script
/// cannot import from the crate it builds.
const UNKNOWN_COMMIT: &str = "unknown";

fn main() {
    let commit = short_commit().unwrap_or_else(|| UNKNOWN_COMMIT.to_string());
    println!("cargo:rustc-env=VIBEZ_GIT_HASH={commit}");

    // Watch HEAD and the branch ref so the baked hash follows both a
    // checkout and a fresh commit on the current branch. Paths that do not
    // exist are skipped: naming a missing file makes cargo rebuild this
    // crate on every single invocation.
    for path in head_watch_paths() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn short_commit() -> Option<String> {
    let hash = git(&["rev-parse", "--short=7", "HEAD"])?;
    (!hash.is_empty()).then_some(hash)
}

/// Files whose content changes whenever the checked-out commit changes.
fn head_watch_paths() -> Vec<PathBuf> {
    let mut refs = vec!["HEAD".to_string()];
    if let Some(branch) = git(&["symbolic-ref", "--quiet", "HEAD"]) {
        refs.push(branch);
    }

    refs.iter()
        .filter_map(|reference| git(&["rev-parse", "--git-path", reference]))
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

/// Trimmed stdout of a successful `git` run, or `None` when git is absent,
/// this is not a repository, or the command failed.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}
