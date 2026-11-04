# Coding standards

The checkable rules for code in this repository. Reviewers read this file
first and cite violations as "CODING_STANDARDS.md rule N". Each rule is stated
here once or summarised with a link to its canonical home in
[ARCHITECTURE](docs/ARCHITECTURE.md), [PATTERNS](docs/PATTERNS.md) or
[CONTRIBUTING](CONTRIBUTING.md), which wins if they disagree.

## 1. Tooling enforces these, so reviews skip them

[CI](.github/workflows/ci.yml) runs `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --exclude vibez-plugin-host -- -D warnings`,
`cargo clippy -p vibez-plugin-host -- -D warnings` (exclusion explained in the
[README](README.md#contributing)) and `cargo test --workspace` on Linux, macOS
and Windows. Do not report formatting or anything Clippy flags.

## 2. Code lives in the layer that owns it

- Placement follows the [crate map](docs/ARCHITECTURE.md#crate-map) and
  [domains and one router](docs/ARCHITECTURE.md#the-ui-domains-and-one-router).
  State math and domain logic live in `crates/vibez-ui/src/domains/`; the
  router (`app/update.rs`, `app/actions.rs`) only routes, applies `Action`s and
  runs async Tasks; views and widgets only render and emit messages.
- Domains never touch iced, spawn tasks or reach into another domain's state,
  and talk outward only through `EngineHandle`, `Ctx` and `Action`
  ([pattern 1](docs/PATTERNS.md#1-the-action-pipeline-mvu),
  [pattern 3](docs/PATTERNS.md#3-ports-and-adapters-enginehandle)).
- Crates depend downward. `vibez-core` depends on no vibez crate; dsp,
  instruments, project and dropbox depend only on core; the engine adds dsp
  and instruments; audio-io and plugin-host sit on the engine; only `vibez-ui`
  depends on iced. New crates are cut along dependency direction, never size.
- Reviews cite the named patterns, among them value objects
  ([4](docs/PATTERNS.md#4-value-objects-own-invariants)), staleness guards
  ([5](docs/PATTERNS.md#5-async-round-trips-and-staleness-guards)) and
  timeline routing ([6](docs/PATTERNS.md#6-timeline-location-routing)). A PR
  that introduces a new convention adds it to PATTERNS in the same PR.
- App preferences live in `ui.json` and never enter project bytes or undo.

## 3. The audio thread never allocates, locks or does I/O

Canonical homes are [the audio engine](docs/ARCHITECTURE.md#the-audio-engine)
and [pattern 2](docs/PATTERNS.md#2-engine-commands-and-events). In anything
reachable from `AudioEngine::process_block`, flag:

- allocation, including Vec growth past reserved capacity, `collect`, clones
  of heap data, `String` or `format!`, `Box::new`, HashMap inserts, and
  dropping the last owner of heap data (owners return to the UI as events);
- locks, blocking waits, logging, file or network I/O;
- an `EngineCommand` whose storage is not prepared by the sender or that is
  unsafe to drop on the audio thread (the `engine_command_allocation_tests`
  allowlist may only shrink);
- a cosmetic event that retries, an authoritative event pushed lossily, or an
  event missing the [time domain](docs/PATTERNS.md#time-domains) its consumer
  follows.

Sample accuracy is correctness. Musical boundaries (launches, mute changes,
automation steps, loop wraps) land on their exact sample by splitting the
buffer, never at the buffer start or the 60 fps UI tick. Gain, mute, bypass and
routing changes ramp so nothing clicks. Every note-on gets exactly one
note-off. A held note remembers its target so later target, mode or Section
changes cannot redirect its release, and stop, focus loss and source
transitions release what they started without doubling it.

## 4. Architecture findings are fix requests, not observations

Report as required changes: duplication the PR introduces (even two copies)
where existing code or a small extraction would serve; transitional bridges
(Deref impls, type aliases, re-exports, shims) without a removal commitment in
the same PR or a named follow-up; domain logic in a view, widget or router
module; reach-arounds of the `EngineHandle`, message and action doors; a new
abstraction overlapping an existing one (`ValueDrag`, `TimelineGeometry`,
`TrackedRequest`, the shared timeline editor) without a stated reason;
speculative generality with no named second consumer, and its inverse, a
single-implementation strategy trait; bool or Option clusters where a phase
enum fits. Taste and product judgement stay observations.

## 5. No-op suppression and one undo step per gesture

See [pattern 7](docs/PATTERNS.md#7-undo). A control set to its current value
returns a default action before any engine send, dirtying or destructive side
effect. One gesture is one undo step (`UndoGestureId`); long-lived edits use
one Project Transaction ([projects](docs/ARCHITECTURE.md#projects-undo-and-warping)).

## 6. Names and files

See [pattern 8](docs/PATTERNS.md#8-naming-and-language). Names use producer
vocabulary (Clip, Section, Capture, Warp, Audition, Perform) with Ableton's
semantics where the concept exists. Every source file, tests included, opens
with a one-sentence `//!` responsibility and splits when it stops covering the
file; a sentence that needs "and" is a split request. Files are named for their
domain, never a mechanism (`helpers`, `utils`, `async`, `misc`, `common`). Size
only prompts the question, past roughly 800 lines (1,500 for tests).

## 7. Shortcuts route by focus

A shortcut acts on the editor the producer last explicitly interacted with,
never on what is visible or selected
([clipboard example](docs/ARCHITECTURE.md#shared-content-ownership)). A command
never changes its own target, so a second press cannot escalate. Rendering
("what can I show?") and routing ("where is the user working?") are two named
views over one shared set of conditions, never one merged predicate or two
hand-kept copies. Ableton is the reference when behaviour is unclear.

## 8. Comments explain only the non-obvious why

Comments record constraints, invariants and rationale the code cannot express
(a ground rule of [PATTERNS](docs/PATTERNS.md)). Flag comments that narrate the
code, or restate a rationale whose canonical home is a type or message doc or
one of the docs above.

## 9. Borrowed algorithms cite their source

DSP and engine code that follows an open-source implementation names the
project, the file or algorithm, and its licence in the `//!` header (see
`crates/vibez-dsp/src/auto_pan.rs`). No separate provenance documents.

## 10. Project format stays backward compatible

New saved fields use `#[serde(default)]` so older projects load, default to
the old behaviour so they sound the same, and use `skip_serializing_if` at
their default where practical. A new persisted property is a value object
([pattern 4](docs/PATTERNS.md#4-value-objects-own-invariants)). Tests cover the
legacy default and a save and reopen through a real project file, legacy JSON
and the `.vzp` container where both apply
(`crates/vibez-project/src/compensation_project_tests.rs`).

## 11. Tests prove the claims

New logic gets unit tests in its domain, driven by state, messages and the
recorded `EngineHandle` commands, without the GUI. A behaviour fix lands with a
regression test that fails without it. Every claim in the PR's Tested section
maps to a test. Prefer behavioural tests to frozen snapshots, in proportion to
the code.

## 12. Commits are atomic

Each commit does exactly one thing. A failing test is its own commit, before
the commit that makes it pass. Refactors, renames and formatting never share a
commit with a behaviour change. Messages use the log's prefixes (`feat`,
`fix`, `test`, `refactor`, `docs`, `chore`, `ci`), optionally scoped, as in
`fix(engine): ...`. Branching follows [CONTRIBUTING](CONTRIBUTING.md#branching).

## 13. PR descriptions

Every PR body uses `## Background`, `## Tasks` (a checklist), `## Tested` and
an optional `## Tech Debt`, in that order, and meets the
[PR requirements](CONTRIBUTING.md#requirements-for-a-pr).

## 14. UI changes are looked at

Tests and the dev gallery (`cargo run -p vibez-ui --example <device>
--features dev-gallery`) do not prove a layout. UI changes are checked in the
running app with real audio, in both themes, at narrow widths, minimum panel
height and interface scales 0.75 and 1.5. The PR states what was looked at.

## 15. Prose

Docs, comments, commit messages and PR bodies never use the em dash character
(U+2014). Colons introduce real lists and labels only, never a setup and
reveal such as "the fix: do X".

## Code-smell baseline

A general smell catalogue (Fowler) applies only where this file and the linked
docs are silent. The repository deliberately endorses shapes it would flag:

- the central `match` over `Message` in `app/update.rs` (Repeated Switches) is
  the MVU router of [pattern 1](docs/PATTERNS.md#1-the-action-pipeline-mvu);
- `Ctx` and `Action` are data-only (Data Class) as a domain's narrow interfaces;
- `ArrangementState` is a thin `TimelineEditorAdapter` (Middle Man) so the
  shared editor never branches on Arrange versus Section
  ([shared content](docs/ARCHITECTURE.md#shared-content-ownership));
- the automation override protocol is parameter-shaped, not mute-specific
  (Speculative Generality), because gain and pan are its named next consumers
  ([architecture](docs/ARCHITECTURE.md#the-ui-domains-and-one-router)).
