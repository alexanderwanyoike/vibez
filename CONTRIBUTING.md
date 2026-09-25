# Contributing to vibez

## Branching

```
main <- dev <- feature/your-thing
```

- **main** is release-quality. Only `dev` merges into it, and releases are
  tagged from it (`v*` tags trigger the release pipeline).
- **dev** is the integration branch and the default branch for pull requests.
- **feature/…** (or `fix/…`, `docs/…`) branches off `dev` and comes back via PR.

## Requirements for a PR

- CI green on all three platforms: `cargo test --workspace` and
  `cargo clippy --workspace -- -D warnings`
- `cargo fmt` clean
- New logic comes with unit tests; UI logic belongs in a domain module
  (see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)) so it is testable
  without the GUI
- No source file over 1,000 lines; split along the existing seams instead
- Nothing that allocates, locks, or does I/O in the audio callback

## CI scope

Feature PRs and prototype epics run workspace tests on Linux, macOS and Windows.
The `codex/epic-*` pattern includes future epics without editing the workflow.
Check, Clippy and Format also run for `dev`/`main` pushes and promotion PRs from
those branches. Application/package builds and Windows executable-resource
validation run only on version-tag releases. Manual release dispatch is removed
so an arbitrary feature branch cannot publish release packages.

## Releasing

Merge `dev` into `main`, then tag:

```sh
git tag v0.x.y && git push origin v0.x.y
```

The release workflow builds the .deb, AppImage, tar.gz, both macOS .dmg
bundles, and the Windows installer, and attaches them to the GitHub Release.
