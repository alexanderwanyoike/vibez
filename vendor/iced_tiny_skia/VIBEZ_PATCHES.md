# iced_tiny_skia 0.13.0, patched for vibez

This is the published `iced_tiny_skia` 0.13.0 crate (MIT, see `LICENSE`),
wired in through `[patch.crates-io]` in the workspace `Cargo.toml`. iced falls
back to this software renderer when no usable GPU is found, and 0.13.0 drops
canvas shapes that the GPU renderer draws. iced 0.14 fixes these upstream; the
patches below backport those fixes so vibez can stay on iced 0.13.

Delete this directory and the `[patch.crates-io]` entry when vibez moves to
iced 0.14 or later.

## Patches
