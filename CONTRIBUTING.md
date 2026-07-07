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

## Releasing

Merge `dev` into `main`, then tag:

```sh
git tag v0.x.y && git push origin v0.x.y
```

The release workflow builds the .deb, AppImage, tar.gz, both macOS .dmg
bundles, and the Windows installer, and attaches them to the GitHub Release.
