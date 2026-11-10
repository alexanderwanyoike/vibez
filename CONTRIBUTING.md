# Contributing to vibez

## Branching

```
main <- dev <- feature/your-thing
```

- **main** is release-quality. Only `dev` merges into it, and releases are
  tagged from it (`v*` tags trigger the release pipeline).
- **dev** is the integration branch and the default branch for pull requests.
- **feature/…** (or `fix/…`, `docs/…`) branches off `dev` and comes back via PR.

Stacked feature PRs use `feature/<topic>-<layer>` names and may target another
feature branch while their shared foundation is under review. Merge that
foundation first, then retarget the dependent PR to `dev`. CI tests stacked
feature PRs on all three platforms.

## Requirements for a PR

- Follows [CODING_STANDARDS.md](CODING_STANDARDS.md), the rules code review
  checks
- CI green on all three platforms: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets --exclude vibez-plugin-host -- -D warnings`
  and `cargo clippy -p vibez-plugin-host -- -D warnings`
- `cargo fmt --all -- --check` clean
- New logic comes with unit tests; UI logic belongs in a domain module
  (see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)) so it is testable
  without the GUI
- Every source file states its single responsibility in its `//!` header, and
  splits when that sentence no longer covers it (see
  [docs/PATTERNS.md](docs/PATTERNS.md#8-naming-and-language))
- Nothing that allocates, locks, or does I/O in the audio callback

## CI scope

Feature PRs and prototype epics run workspace tests on Linux, macOS and Windows.
Every pull request also runs Clippy and Format on Linux, so lint and format
failures surface before a feature lands on `dev`. The `codex/epic-*` pattern
includes future epics without editing the workflow. The four-runner Check job runs only for `dev`/`main` pushes
and promotion PRs from those branches, where Clippy and Format run as well.
Application/package builds and Windows executable-resource
validation run only on version-tag releases. Manual release dispatch is removed
so an arbitrary feature branch cannot publish release packages.

## Releasing

Merge `dev` into `main`, then tag:

```sh
git tag v0.x.y && git push origin v0.x.y
```

The release workflow builds the .deb, AppImage, tar.gz, both macOS .dmg
bundles, and the Windows installer, and attaches them to the GitHub Release.
