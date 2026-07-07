# The domain-extraction playbook

Written 2026-07-07 after extracting three domains (transport,
devices, arrangement-tracks) in PRs #16, #17, #20. Follow this recipe
to finish the remaining tranches. Read `domains/transport.rs` (small)
and `domains/devices.rs` (shared-model style) as the worked examples.

## The pattern

Each domain in `crates/vibez-ui/src/domains/` owns:
- a state slice struct (defined in `state.rs`, field on `AppState`),
- a `XMsg` enum (variants moved out of the top-level `Message`),
- `update(&mut self, msg, engine: &mut impl EngineHandle, ...) -> XAction`,
- unit tests against `test_support::RecordingEngine`.

Cross-domain rules:
- Read-only facts other domains must provide come in via a small
  `XCtx` struct computed by the router (see transport's).
- Effects the domain cannot perform (GUI teardown, selection focus,
  status text) go OUT via the `XAction` struct; app.rs applies them
  in `apply_x_action`.
- Tracks are the shared model: domains that edit track-owned data
  receive `&mut [UiTrack]` explicitly (devices) or own the list
  (arrangement).
- `XMsg::marks_dirty()` classifies undo-worthy edits; app.rs's dirty
  check calls it instead of enumerating variants.

## The recipe (per tranche)

1. Branch off main. Move fields into the slice struct in `state.rs`.
   Build: the compiler enumerates every usage site.
2. Sweep usages: sed the single-line `self.state.field` patterns,
   then a MULTILINE regex for `self.state\n.field` chains (fmt splits
   them), then hand-fix init literals. Commit when green.
3. Write the domain module: move handler bodies verbatim from app.rs
   arms into `update`, converting `self.state.*` to slice fields and
   `self.send_command` to `engine.send`. Port cross-domain touches to
   Ctx/Action. Write tests.
4. `Message` migration:
   - Remove the variants; add `X(XMsg)`.
   - For TUPLE variants add arity-preserving constructor helpers on
     `Message` (e.g. `Message::set_track_gain(t, g)`) and sed call
     sites to the helper names: this stays parenthesis-balanced.
     Unit variants sed directly to `Message::X(XMsg::Variant)`.
   - Struct-literal variants (`Msg::Foo { .. }` construction sites):
     few sites; convert by hand.
5. Delete the old app.rs arms with a brace-matcher script; REQUIRE
   the start line to contain `=>` or you will eat an innocent
   multi-line construction argument (this happened; audit deletions
   against `git show HEAD:...` if the count is off by one).
6. Install the router arm + `apply_x_action`, fix the keep-menu and
   dirty `matches!` lists (add `|| matches!(&message, Message::X(m)
   if m.marks_dirty())`).
7. Verify HONESTLY: `cargo test --workspace` must show the expected
   COUNT of `test result: ok` lines AND zero `test result: FAILED`.
   Grepping only for FAILED reads zero when a test target fails to
   COMPILE. Then clippy (zero errors, zero unused warnings), fmt,
   release build, in-app smoke test.

## Known traps (all hit tonight)

- iced: `Length::Fill` child inside a shrink-width parent collapses
  the container (backgrounds/borders vanish). Cards use computed
  fixed widths.
- Never blind-regex Rust: a `\bKEY_WIDTH\b` sweep once rewrote a
  constant inside its own accessor method into infinite recursion
  (silent stack-overflow crash, no panic output).
- Tuple-variant regex rewrites leave unbalanced parens at every call
  site; use the constructor-helper trick instead.
- Panel/viewport geometry: fix the CONTAINER first, then content.
  Device strip is fixed-height (Ableton model); text that can wrap
  inside fixed-height cards must be hard-truncated to one line.

## Remaining tranches

- 3b arrangement clips: audio clip add/move/resize/split/join/
  duplicate/delete, time selection, ToggleClipLoop/SetClipLoopRegion,
  warp orchestration messages (WarpClipToProject, ClipWarpReady,
  ClearClipWarp, quantize, nominal BPM). Task-returning handlers can
  stay in app.rs calling domain fns for the state math if Tasks prove
  awkward (see devices' async note).
- 4 piano roll: note clip CRUD, note editing/selection/nudge,
  loop-region messages, HalveNoteClip/DoubleNoteClip/CropNoteClip.
- 5 browser: sample browser, preview, Dropbox, drag-drop dispatch.
- 6 project: save/load/undo/export/snapshots (also fix: snapshots
  drop plugin devices; see dogfood #22 note).
- 7 services: plugin load pipeline (poll_plugin_loads + bg loaders),
  scanning, plugin window manager glue behind channel interfaces.
