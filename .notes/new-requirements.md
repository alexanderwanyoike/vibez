# New Requirements

Captured 2026-04-18, arising from early Dropbox sample-library testing.

## 1. ISO / CUE archive browsing in the Dropbox tree

### Motivation

A large fraction of the Megalodon library is shipped as disc images
(`.iso` for data CDs, `.cue` + `.bin` for audio CDs). Right now the
Dropbox browser sees these as opaque files. The user has no way to
reach the samples inside without downloading the image, mounting it,
and importing files manually.

### Proposed shape

Treat a disc image as a virtual folder in the Dropbox tree.

1. **Detection**: entries with `.iso`, `.cue`, or `.bin` extensions are
   rendered with a distinct "archive" glyph and are expandable.
2. **On expand**: show a spinner on the tree row. Download the file to
   an archive cache directory (see below). Bytes-progress in the status
   bar if feasible; otherwise indeterminate spinner is fine.
3. **Parse once downloaded**:
   - `.iso` → ISO 9660 (with Joliet fallback). List directories and
     extract individual files on demand. Hand-rolled parser or small
     crate; ~200 lines either way.
   - `.cue` + `.bin` → parse the text cue sheet, expose each audio
     track as a virtual `Track NN.wav` entry. Extract on click to a
     PCM 16-bit / 44.1 kHz stereo blob.
4. **Click a file inside an archive**: the same audition / import
   pipeline as regular Dropbox files. The archive member's virtual
   path replaces the raw `path_lower` in the `MediaSourceRef`.
5. **Project portability**: clips imported from archive members carry
   the archive's Dropbox `path_lower` plus an internal `member_path`
   so re-opening a project on a fresh machine re-downloads the archive
   and re-extracts the named member.

### Cache layout

Split by purpose to keep clear-buttons meaningful:

- `~/.config/vibez/` — settings (unchanged).
- `~/.cache/vibez/dropbox/` — downloaded files (existing).
- `~/.cache/vibez/archives/` — downloaded disc images + extracted
  members. Keyed by Dropbox `(path_lower, rev)` like everything else.
- Optional v1.1: a size-watermark warning ("20 GB used"), not
  automatic eviction.

### Cache-management UI

A **Cache** tab in Settings:

- List of caches with total sizes:
  - Dropbox downloads (X GB, Y files)
  - Archive extractions (X GB, Y files)
- "Clear" button per cache.
- All clears manual in v1. No LRU eviction yet.

### Dependencies

- Probably one small ISO 9660 crate, or a hand-rolled module under
  `vibez-dropbox::archive`. The cue-sheet parser is trivial (~100
  lines).
- No new runtime deps expected.

### Estimate

~1 day of focused work.

---

## 2. Sync + Quantize

Electronic music production requires aligning audio and MIDI to a
grid and to each other. Current vibez state: snap grid exists for the
piano roll, but nothing actually quantizes, time-stretches, or
detects tempo.

Sized honestly below. These are separable, and the cheap pieces
deliver a lot of the perceived value.

### MIDI quantize

**Estimate**: half a day.

Right-click a note clip → Quantize. Snap `start_beat` of each note to
the nearest grid position (the existing snap grid) with:

- **Strength** (0-100%): 100% is full snap, 50% pulls notes halfway,
  0% is no-op.
- **Swing** (0-100%): delays every odd 8th/16th by a fraction of the
  grid unit. We already implemented this as `PhraseMutator::Swing`
  under `vibez-core::phrase_variation`; lift the logic.

Deliverable: a small UI panel on the clip properties for Strength,
Swing, Grid, and an Apply button.

### Clip nominal BPM + offline resample on import

**Estimate**: 1-2 days.

Add `nominal_bpm: Option<f64>` on `ClipInfo` / `UiClip`. When set, at
import or on demand, compute `ratio = project_bpm / clip_bpm` and run
the existing `rubato` resampler to produce a tempo-matched copy of the
clip's audio. The resampled buffer replaces `UiClip.audio`; the clip
then plays back tempo-locked.

Caveats:
- Resample shifts pitch too. Perfect for drums, acceptable for most
  loops, noticeable on melodic content.
- Project-tempo changes after import require a re-resample; the
  resampler is fast enough that this can run on tempo commit.

UI: clip properties grows a BPM field and a tap-tempo button. Import
pipeline detects existing BPM tags (filename conventions like
`loop_128bpm.wav`, WAV ACID chunk) if straightforward.

### Auto BPM detection

**Estimate**: 1-2 days. Optional.

Energy-based onset detection (with high-frequency weighting for
transient sensitivity), autocorrelation over the onset envelope, peak
pick → BPM suggestion. Works well on percussive content, flaky on
pads and long tones.

Surfaced as a "?" button next to the manual BPM field: click to fill
with a detected suggestion; user can override.

### Pitch-preserving time-stretch (warp)

**Estimate**: a week or more. Deferred until actually needed.

This is the Ableton-style feature where audio plays at the project
tempo without the pitch following tempo. Requires a phase vocoder or
WSOLA running either offline (simpler) or inside the audio callback
(much harder and more useful). Candidates:

- **rubberband** C++ bindings: excellent quality, LGPL, adds a native
  dep. Stretchy's or CR's bindings exist but are thin.
- Hand-rolled WSOLA: ~500-800 lines, good for drums/loops, weaker on
  solo melodic content.
- Pure-Rust phase vocoder crate: nothing battle-tested as of now.

Not a day-one item for sample-first drum-heavy workflows. Resample
alone (with pitch following) serves the "drum loop tempo-match" case.
Revisit once the user has hit its limits on real projects.

### Warp markers, transient-based slicing, audio quantize

**Estimate**: week+ each, layered on top of pitch-preserving stretch.
Out of scope for the current phase.

These are the advanced audio-alignment features (manual anchor points
on audio that snap to grid, slice-based auto-quantize for drum
loops). Valuable, not critical.

### Recommended shipping order

Each line is independently shippable.

1. **ISO / CUE browsing + Cache tab** — unlocks Megalodon properly.
2. **MIDI quantize** — cheap win, directly improves phrase workflow.
3. **Clip nominal BPM + offline resample** — gets tempo-matched drum
   loops without a real time-stretcher. Limited but immediately
   useful.
4. **Auto BPM detection** — quality-of-life.
5. **Pitch-preserving time-stretch (warp)** — defer until it's missed.

---

## Open questions

- Should imported archive members keep their Dropbox reference
  (re-extract on load) or be baked out to `LocalFile` copies in the
  user's project folder? The former is lighter on disk; the latter is
  portable without auth.
- Cache-size budget: is a soft warning fine for v1, or do we want
  actual LRU eviction before shipping?
- BPM-tag detection: should we parse the WAV ACID chunk / filename
  conventions at import, or always force manual entry to start?
