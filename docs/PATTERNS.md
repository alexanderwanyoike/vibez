# The Vibez pattern language

This is the map of the house conventions. `ARCHITECTURE.md` explains what the
pieces are; this document explains the recurring shapes they are built from,
so that any change can be recognised as an instance of a named pattern rather
than reverse-engineered from examples. Reviews cite these names. A PR that
introduces a new convention adds it here in the same PR.

Two ground rules frame everything below:

- The audio callback is lock-free and allocation-free. Anything reachable
  from `AudioEngine::process` must neither allocate nor block, and every
  `EngineCommand` variant must be safe to drop on the audio thread.
- Comments state the why the code cannot express (constraints, invariants,
  rationale). Never what the code does.

## 1. The action pipeline (MVU)

All UI behaviour flows one way:

```
widget/canvas event
  -> Message (message.rs)
  -> routing (app/update.rs)
  -> domain update (domains/<domain>/..., pure, no iced)
  -> Action struct (e.g. ArrangementAction: status, mark_dirty, follow-up requests)
  -> app-layer applier (app/actions.rs) performs cross-domain effects
```

Domains never touch iced, never spawn tasks, and never reach into another
domain's state; they return everything they want done inside their `Action`.
The applier is the only place cross-domain choreography happens.

## 2. Engine commands and events

UI to engine is an `rtrb` ring of `EngineCommand`; engine to UI is a ring of
`EngineEvent`, drained once per frame tick. Commands carry `Arc`s so the
audio thread never allocates or frees large buffers it uniquely owns.
Events come in two classes, and the class determines the push policy:

- **Authoritative** (Capture consumes them, state depends on them): pushed
  normally; consumers own staleness handling.
- **Cosmetic** (meters, pad flashes): pushed with `let _ =` and deliberately
  lossy. A cosmetic event must never retry, because a retry backlog competes
  with authoritative events for ring slots after a UI stall.

## 3. Ports and adapters: EngineHandle

Domain code sends engine commands through the `EngineHandle` trait, never
through a concrete channel. The adapters are the vocabulary:

- `EngineTx` — the real command queue (Arrange, live edits)
- `DiscardingEngine` — the null object, for edits to non-resident content
- `TimelineResultEngine` — Arrange sends, Section discards, used by
  `with_timeline_editor_at`

If an edit must behave differently against resident and non-resident content,
that difference lives in the adapter choice, not in branches inside the
domain logic.

## 4. Value objects own invariants

Every persisted clip property is a type whose constructor and `Deserialize`
enforce its invariant, and whose methods own the maths call sites would
otherwise re-derive:

- `ClipTimeline` (frame and beat flavours) — play-time to source-time
  mapping, occurrence iteration, start-marker clamping
- `ClipFades` / `FadeCurve` — fade lengths, curve law (`gain_for` is the one
  gain law for both rendering and drawing), crossfade links and their
  equal-power complementarity
- `TransientMarkers` — canonical ordering, Suggested/Authored semantics
- `WarpMarkers` — strict double monotonicity, boundary anchors
- `ClipGainDb`, `ClipTranspose`, `TransientSensitivity` — range clamping

The rule: a new persisted property is a new value object (or an extension of
the owning one), with the invariant in the constructor. Call sites compose;
they never re-implement the mapping. Five hand-copies of loop math taught us
this the expensive way.

## 5. Async round-trips and staleness guards

Heavy work leaves the UI thread through `run_off_ui_thread` /
`spawn_blocking`, and returns as a `Message` carrying everything needed to
detect that the world moved on. On arrival, the handler verifies currency
before applying:

- **Arc identity** (`Arc::ptr_eq` against an `expected_audio`) when validity
  is tied to a specific buffer
- **Semantic comparison** (`has_same_audible_geometry`) when a pure move
  should not invalidate the result
- **Revision tokens** (monotonic counters) for debounced streams such as the
  transpose wheel
- **Location checks** (`active_timeline_location()`) when results route to
  Arrange or a Section

A stale result is either dropped with a status message, or — when the domain
state still wants the work — requeued from *current* state
(`apply_clip_transpose_success`). Silently applying a stale result is always
wrong.

## 6. Timeline-location routing

Background results that edit Arrange or one Section route through
`with_timeline_editor_at(location, apply)`, which owns the three-way
dispatch (Arrange / selected Section with commit-and-refresh / background
Section with editor rebuild). Never hand-roll this match; it has drifted
before and was unified for that reason.

## 7. Undo

Undo is whole-project snapshots. The conventions:

- One user gesture is one undo step: continuous inputs carry an
  `UndoGestureId` minted at press and reused across moves; the history
  coalesces snapshots by gesture id.
- Messages whose outcome decides whether anything changed are listed in
  `defers_project_edit()`: the snapshot is taken before routing but pushed
  only if the domain reports `mark_dirty`.
- **No-op suppression**: a handler compares the requested value with current
  state and early-returns `ArrangementAction::default()` before any side
  effect — before the engine send, before dirtying, and before anything
  destructive that a real change would justify (for example a crossfade
  unlink). Wiggling a control must never dirty the project.

## 8. Naming and language

Types and messages use the producer vocabulary — Clip, Section, Capture,
Warp, Audition, Perform — and follow Ableton's semantics where the concept
exists there. Files are named for the domain they serve; mechanism names
(`helpers`, `utils`) are banned because they cannot refuse new code. Every
file opens with a `//!` stating its single responsibility in one sentence;
when the sentence no longer covers the file, the file splits.

---

## Trace 1: a fade drag, from pixel to speaker

1. `widgets/timeline/fade_drag.rs` — press hits `fade_control_hit`
   (distance-based resolution between overlapping controls), mints an
   `UndoGestureId`, and each move converts pixels to frames with an
   edge-snap window (pattern 5's cousin: the snap uses the exact inverse of
   the drawn position so an unmoved pointer cannot jitter).
2. It emits `Message::Arrangement(SetAudioClipFade { .. })` wrapped
   `.in_undo_gesture(id)` (pattern 7).
3. `app/update.rs` sees the message in `defers_project_edit()`, takes a
   snapshot, and routes to the arrangement domain.
4. The domain handler suppresses no-ops, consults
   `crossfade_candidate_for_fade` (equal overlap links a crossfade pair),
   otherwise applies `ClipFades::with_fade_in/out` — the value object clamps
   the pair (pattern 4) — and sends `EngineCommand::SetClipFades` through its
   `EngineHandle` (pattern 3).
5. The engine drains the command between callbacks and re-clamps
   defensively; the callback multiplies each frame by
   `fades.gain_at(clip_frame, duration)` — allocation-free, with the linear
   fast path.
6. The returned `ArrangementAction` marks dirty; the app pushes the deferred
   snapshot under the gesture id — one undo step for the whole drag.

## Trace 2: transient analysis, from click to markers

1. The Analyse button commits the sensitivity text field
   (`commit_sensitivity_input`), then dispatches
   `detect_clip_transients_async` via `spawn_blocking`, capturing
   `expected_audio` (pattern 5).
2. Detection runs the aubio-port pipeline in vibez-core; the genuine-attack
   check is a predicate *inside* peak acceptance so rejected candidates never
   claim the refractory — which is what makes the sensitivity knob monotone.
3. Completion arrives as `Message::ClipTransientsDetected`; the handler
   verifies `Arc::ptr_eq(&clip.audio, &expected_audio)` and drops stale
   results with a status line.
4. Current results route through `with_timeline_editor_at` (pattern 6) to
   `ReplaceDetectedTransientMarkers`, where `TransientMarkers`
   canonicalises and preserves Authored markers (pattern 4).
5. The snapshot taken before routing is pushed only because the action
   marked dirty; an idempotent re-run reports "(no change)" and pushes
   nothing (pattern 7).
