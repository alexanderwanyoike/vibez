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

Progress 2026-07-07 (stacked PR chain, MERGE BOTTOM-UP:
#22 -> #23 -> #24 -> #25 -> #26 -> #27; merging out of order
auto-closes the PRs above the gap when base branches are deleted):
- 3b-1 clips selection/move/resize/loop: DONE (PR #22).
- 3b-2 clips split/join/region ops + join helpers: DONE (PR #23).
  ArrangementCtx gained playhead_samples/playhead_beats;
  ArrangementAction gained close_context_menu.
- 4 piano roll: DONE (PR #24). PianoRollState slice (scroll_y,
  edit_mode); PianoRollMsg with 19 variants; quantize_note_clip +
  default_loop_end moved into the domain. Widget construction
  sites were wrapped in place with a paren/brace-matching script
  (Message::X{..} -> Message::PianoRoll(PianoRollMsg::X{..}))
  instead of constructor helpers; both approaches work, matching
  beats regex.
- 5a browser sync tranche: DONE (PR #25). BrowserState slice
  (library, drag-drop, DropboxUiState); 13 sync messages.
  BrowserAction routes persist-settings / expand-dropbox-root /
  drop-on-arrangement back through the router.
- undo-drops-plugins FIX: DONE (PR #26). domains/project.rs
  collect_plugin_reload_requests strips plugin devices from the
  snapshot into reload work orders; apply_snapshot feeds them to
  spawn_project_plugin_loads (the project-open pipeline). Live
  instances get their state captured pre-teardown so parameters
  survive undo exactly.
- 6 project domain: DONE (PR #27). ProjectState slice
  (file_menu_open, current_path, dirty, history); ProjectMsg
  Undo/Redo with pure stack bookkeeping (router passes
  snapshot_now in ctx, receives apply_snapshot in the action).

Still to do:
- 3b-3 clip async/warp orchestration: AddClipToTrack,
  ClipFileSelected/Decoded, warp messages (WarpClipToProject,
  ClipWarpReady, ClearClipWarp, RewarpAllClips, ClipAutoWarpReady,
  DetectClipBpm, ClipBpmDetected, SetClipNominalBpm, quantize
  audio), bounce. These spawn iced Tasks; keep the Task::perform
  in app.rs and move the state math into domain fns the arm calls.
- 5b browser async: dialogs, decode/preview, Dropbox HTTP,
  import/drop dispatch (dispatch_drop_on_arrangement and friends).
- 6b project async: save/load/export orchestration + the engine
  replay helpers (replay_track_to_engine, load handling).
- 7 services: plugin load pipeline (poll_plugin_loads + bg loaders),
  scanning, plugin window manager glue behind channel interfaces.
  Hardest and least mechanical; do it last, with the app running
  for smoke tests after every step (plugin teardown ordering is
  segfault territory, see apply_snapshot's comment).

Final count this session: app.rs ~9,100 lines (from 11,305),
60 UI unit tests (from zero), six domain modules (transport,
devices, arrangement, piano_roll, browser, project) plus the
shared EngineHandle seam in domains/mod.rs.
