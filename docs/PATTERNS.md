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
Commands carry or reserve their storage, and the callback never grows a
collection: devices are built on the sender's thread
(`EngineCommand::insert_built_in_effect`, `set_built_in_instrument`) at the
rate `EngineHandle::sample_rate` last forwarded, channels are prepared there
too (`EngineCommand::add_track`, `add_bus`) and move by pointer, and device
chains and channel lists are reserved at `DEVICE_CHAIN_CAPACITY` and
`CHANNEL_CAPACITY`, which the UI refuses to exceed.
Send storage is prepared with `CHANNEL_CAPACITY` slots, one per possible return
channel. Manual and automated edits share the same admission: existing targets
remain editable at capacity, while another target cannot grow callback storage.
Offline channel preparation preserves that reserved storage.
`engine_command_allocation_tests` measures every command; its list of commands
that still allocate may only shrink.

AddClip carries a required sender-prepared `PreparedClipInsertion` control.
AddNoteClip carries an opaque `PreparedNoteClipInsertion`, which constructs the
complete empty note clip, its cache and the same control outside the callback.
Following AddNote commands remain separate FIFO edits with an opaque
`PreparedNoteInsertion` owning the original MidiNote and the same control.
SetAutomationLane carries an opaque `PreparedAutomationLaneInsertion` owning
the original whole lane and one empty lane Vec in that same control.
All four operations share one pending insertion, mailbox implementation and
request/completion event family. Their typed target distinguishes ClipId from
LaneId without inventing a clip identity for a lane. Capacity is only a hint,
never a content limit or a content snapshot.
If current live clip/removal storage cannot accept the original delta, the
engine retains that delta and requests empty storage through
`ClipInsertionStorageRequested`. Current complete clip values move in order
into the prepared allocation; existing note caches and groove latches move
with them. Source construction and offline preparation reserve removal
staging from actual content, and live insertion installs sufficient live and
staging capacity together.

AddNote addresses the current first matching Arrange note clip. Its concrete
storage bundle owns the notes Vec, next-same-pitch clamp-cache Vec and
note-sort scratch Vec. All three capacities are checked before any allocation
or existing clamp value changes. Current notes, cache values and scratch move
together in caller order;
then the original note appends using the existing incremental clamp update.
Insertion preserves geometry, groove latch and sounding voices, and neither
flushes notes nor recalculates project length. Later index edits, removal or
Undo/replay remain later FIFO work; a canonical UI replacement cannot redirect
an already pending note to a future reused target. Public construction and
actual offline preparation retain complete caches before live growth.

SetAutomationLane resolves Master first, then the first Track or Bus, and
replaces the first matching LaneId before considering outer capacity. Matching
is by ID, so a replacement can change target or points without changing other
lane order or duplicate semantics. A new-ID append moves the current lane
values and their original point Vecs into sufficient prepared outer storage.
Only that empty old outer allocation moves into the complete control; lane
editing does not flush notes, reset devices/history or recalculate length.

Replacement and missing-channel paths preserve the original
`RetiredAutomationLane` event before emitting their complete control. Initial
admission needs two current retirement slots before popping the command, even
when eager storage is adequate. Under a full event ring the existing bounded
store flushes LIFO, so eventual control publication can precede the lane return.
A pending new-ID append or abandonment needs one current completion slot;
defensive pending replacement/rejection checks both slots before mutation.
There is no credit held while awaiting a reply. This operation uses one lane
Vec, without a parallel removal-staging Vec or a new mailbox.

The same prepared exchange returns all three displaced inner allocations in
the completed insertion owner. The callback neither grows nor discards live
sort scratch; source construction and offline preparation use the same bundle.

Current playback continues while one insertion waits, but later FIFO commands,
including Stop, Undo replay and NoteOff, wait for this exchange. Preparation
may therefore move an edit to a later callback boundary. Section and launcher
rendering restore the Arrange source before command drain, so a pending
insertion edits the current Arrange collection even if an already queued
musical source transition occurs while storage is prepared.

The App always consumes each request with `request.fulfill()` outside the
callback, including when project reset or Undo has made its target stale.
Public native/library event consumers have the same progress obligation.
Fulfillment allocates synchronously and uses the control's separate one-slot
reply mailbox, never an EngineCommand queued behind the waiting insertion.
Allocation follows ordinary Vec allocation/abort policy. A native caller may
explicitly abandon a request by dropping it unfulfilled; the engine then
returns the complete original payload with `Abandoned`, leaves content intact
and resumes FIFO once a completion credit is available. App target staleness
must never deliberately choose abandonment.

Every `ClipInsertionCompleted` owner is retained or destroyed off callback.
It carries rejected content, displaced/unused Vec allocations and the final
mailbox endpoints/control even on immediate or missing-target paths. The
callback reserves each actual request/completion phase independently; no
retirement credit stays held merely because the caller has not fulfilled yet.

Events come in two classes, and the class determines the push policy:

- **Authoritative** (Capture consumes them, state depends on them): enter
  `present_event`, or `present_event_at` when a resumed Clip packet must retain
  its original physical release time. Both use the same bounded queue. Due
  retained events precede new due events, including when UI capacity returns.
  Full PDC uses `present_event_after` for actual path delays; it delegates to
  the same absolute physical-clock entry. Future events do not block an immediate
  source event. Clip resync snapshots
  wait for earlier due acknowledgements; consumers still handle staleness.
- **Cosmetic** (meters, pad flashes): pushed with `let _ =` and deliberately
  lossy. A cosmetic event must never retry, because a retry backlog competes
  with authoritative events for ring slots after a UI stall.

### Time domains

With plugin delay compensation the engine renders ahead of what the listener
hears, so every event names the clock its consumer follows
(`EngineEvent::time_domain`, an exhaustive match):

- **Source**: the render position. Clip and Section recorders and their
  lifecycle events use it, so a recorded note lands where the player intended
  and cannot fall behind its count-in or arrive after its take has closed.
- **Heard**: the render position plus compensated output latency. Playheads,
  transitions, Capture, mute and automation feedback, and live-input pad
  feedback use it, so the screen follows the speakers.
- **Untimed**: acknowledgements, failures, meters and ownership returns.

Only heard events wait for output latency (`present_event_after`, or a future
deadline for `present_event_at`); debug builds assert it there and in the Note
Repeat pair.

`SectionSourceRefreshed` acknowledges engine consumption and returns displaced
or rejected source ownership. It stays Untimed and enters `present_event`
without an output-latency delay; UI disposal does not change playback or Capture.
The separate Heard `SectionCaptureSource` retains its path timing and source
coordinates. `ClipSourceRefreshed` stays Heard because its consumers update
launcher state and Capture at the audible edit boundary.

Cancelling the queue (Stop, Play, a presentation fault) keeps
Capture events that are already due, replaces pending transitions with their
ownership returns, and drops other pending heard feedback.

### Output envelopes

`OutputGate` owns the previous typed `OutputPermit`, the last stereo frame and
three independent 64-frame loss, resume and plan-morph counters. Processing
readiness remains the engine's decision; the gate consumes its result and never
authorizes processing. Recovery may arrive before loss finishes, and a changed
plan may start a morph while recovery is pending, so these envelopes cannot be
exclusive states. Loss runs before resume, then morph blends the result before
the last-frame update. A changed signal path starts a morph only when playback
is running and the previous permit was Audible. Play clears only the morph;
pending loss and resume progress remain intact. The owner uses fixed inline
storage and introduces no callback allocation or ownership transfer.

## 3. Ports and adapters: EngineHandle

Removed devices stop processing on the audio thread before their unique owner
is sent to the UI for destruction. Channel owners use preallocated pool slots
with an Acquire/Release handoff. Clip audio replacement also returns its displaced
sample owner to the UI; rejected replacements return their incoming owner.
Each replacement reserves one retirement slot before the command is consumed.
Device disposal uses 32 preallocated pending retirement slots.
State events and source owners use a separate 2048-entry
presentation queue, ordered by physical due time and insertion order. Command
admission reserves its bounded body before taking ownership; track-sized cleanup
retains unprocessed queued owners and mute intents in place and pauses later
commands. Runtime boundary emitters reserve before changing state. Clip packets
reuse their transferred Vec for resumable acknowledgements and owner return.
Neither store grows or destroys undelivered owners in the callback.

Native sampler and drum pad media edits reserve one device-retirement slot
before command admission. A single `RetiredInstrumentMedia` event returns both
the displaced decoded audio and its name; rejected edits return incoming owners
through the same path. Drum pad state resolves its display name on the sender,
so source paths and provenance never enter the callback.

Sounding drum voices can retain older audio after a pad replacement. The UI
deduplicates returned sample guards by allocation identity and reaps a guard
only when it is the last strong owner. Callback sources use strong references,
never Weak upgrades. App shutdown closes plugin editors and destroys the audio
stream before releasing these guards and sample caches.

Clip removal first extracts every matching ID into prepared storage owned by
its `PreparedPlaybackSource`, preserving survivor order and making deletion
audible atomically even when more than 32 owners must return. Retirement
checks its bounded credit before moving each complete audio or note clip;
later commands wait while source storage still holds pending owners. All four
resident source homes participate. A Section or Clip source transfer carries
any pending storage with the whole source, so an owner is never returned twice.
The still-allocating `AddClip` and `AddNoteClip` paths maintain this storage's
capacity from actual collection lengths. Preparing those insertions off-thread
remains separate Card 04 work; removal never grows either collection.

Warp marker edits return the displaced or rejected marker Vec to the UI for
destruction through an Untimed event. Command admission reserves one retirement
slot before taking that owner; a missing target does not free incoming storage.

`PresentationState` owns Normal/Faulted health independently of its singular
`TerminalNotifications` aggregate. The aggregate retains one Capture stop, one
named cause, one playback-stop acknowledgement and one discovering overflow
owner. Capture stop and cause retain the first request, including when Seek
moves the clock backwards. Fault entry is idempotent. Play resumes health and
cancels only the inline playback-stop acknowledgement; it retains the other
slots and does not repair compensation readiness or device ownership.

Presentation exhaustion stops playback and Capture and converts canceled
Section/Clip notifications into owner retirement. After already-due accepted
Capture start/data drain, delivery proceeds through Capture stop, named cause,
playback stop, overflow owner and then normal queued feedback. Each slot clears
only after successful publication; a Full response restores the exact overflow
owner without cloning it. These owners survive resumed presentation health.
Packet admission subtracts pending inline notifications from available UI slots;
the same slot cannot admit both a terminal event and a transferred owner.
Subsequent packet publication returns owners without reasserting playing state. Capture-only cancellation preserves continuing-playback Section,
Clip and transport acknowledgements, and immediate recording feeds.
Cancelled Capture offset owners retain their original physical deadlines so
retirement cannot advance another still-delayed public state.

A Section recording stop freezes its original coordinates and optional prepared
source in the recording lifecycle when every presentation return path is full.
Publication checks capacity before taking this owner and retries it before older
ordinary retirements, without holding a retirement credit while waiting. A later
Arm waits at the command FIFO before admission until that stop can return; Play
remains independent. Exhaustion still faults presentation at the discovering
boundary. If the receiver never recovers, non-audio engine teardown releases the
retained source.

Every engine starts with a caller-prepared master-only routing plan and keeps
one non-optional active plan. The first project publication parks that bootstrap
owner in one fixed engine slot until non-audio engine teardown; it emits no
retirement event and starts no plan morph. Subsequent publications return the
displaced project plan through the existing bounded retirement path. This
preserves startup event order without destroying the bootstrap in the callback.

`CompensationPlanState` owns applied-plan validity independently of an
outstanding structural-plan expectation. Effect-chain command admission marks
only the expectation; invalidating a rejected replacement preserves that wait.
Play and automation-only graph publication repair neither fact. Only admitted
full routing publication or a successful current device resume accepts both.
Prepared graph retirement and device handoff owners keep their existing,
separate lifetimes, including when a full UI ring blocks command admission.

`DeviceHandoffState` separately owns the one pending metadata token, issued
identity, typed recovery scope, typed project suspension and retained parameter
storage. The stopped device Box stays in the event/main-thread/command protocol.
An identity candidate is checked before device discovery; only a found, stopped
device consumes it. Metadata is recorded before publication. A successful
project-restart transfer suspends processing; local recovery leaves that
suspension clear. Full publication restores the same stopped Box and consumes
the issued identity, so its later retry uses the next identity.

Taking restoration metadata does not resume processing. Retained parameters
apply before target insertion and suspension clears at the existing caller
boundary. All 2048 parameter slots share one engine-lifetime allocation:
duplicates replace values in first-seen order, overflow is reported and consumed,
and replay still attempts later setters after a rejection. Invalidation clears
intent without freeing storage. A stale reply retires its exact old owners and
leaves a newer token, queued intent and applied graph unchanged. Prepared effect
storage and plan retirement retain their separate bounded ownership paths.

Project processing samples fresh, private Copy facts at each original decision
boundary and immediately consumes a pure mute projection. Suspension, plan
blockers and presentation fault retain their lazy order; an owned blocker means
resident demand was not sampled. Otherwise the live scan visits Tracks, buses
and Master, effects before instrument, requested before optional recovery, and
stops at the first nonlocal request. Facts are stack values, never cached engine
health or a simultaneous snapshot of hosted atomics. Recursive graph children,
wider-layout stereo reentry and the final post-render gate each observe again.

Each actual Capture Start command independently samples local failure after
earlier drained commands and owner edits. A current local handoff token takes
priority; otherwise resident instruments precede effects. Pure admission keeps
the local-failure reason before applied-plan invalidity. Structural wait,
nonlocal restart, presentation fault and stopped transport do not independently
refuse Capture.

`CaptureLifecycle` owns current Idle/Active activity independently of the
aggregate Clear/ClosureRequested local-failure episode. Accepted Start changes
activity and rearms local closure after its fresh admission succeeds: a device
may fail again before validation observes recovery. Stop and the existing
closure paths end only activity; refused Start leaves the episode unchanged.
None takes ownership of pending notifications. An older retained stop can
therefore coexist with a newly active Capture.

Validation consumes its already observed local-failure fact in a pure
first-close decision, performs the existing local closure, then records that
fact at the original assignment boundary. An admitted clear observation also
rearms the aggregate episode; a skipped validation leaves it unchanged. This
eligibility is aggregate local-failure state, with no per-device identity or
universal notification deduplication. Play, Seek, Unload, effect reset and
current device Resume do not implicitly rearm it.
Local closure followed by a global invalid action retains both original stop
acknowledgements; public Stop and refused Start keep their independent replies.

PresentationState remains the singular terminal/timestamp/owner authority.
Stop, Play and Arrange Seek inspect activity before lazily checking pending
stops. Performance rebasing retains its narrower pending-stop-only check.
Neither notification delivery nor an accepted recording feed mutates current
Capture activity.

Validation keeps activation rate, configuration validity, metadata/local
recovery and latency reads at their original lazy points. Local recovery skips
latency sampling; accumulated local Capture closure still precedes a later
global invalid action. Handoff discovery samples requests before removal/stop,
while scope samples the transferred owner's recovery answer only after stop.
Offline segments retain validation and graph entry separately, followed by the
strict instrument-first activation/request/valid query only after cached
processing errors have been consumed. Its typed facts produce the original
first diagnostic; Bounce gains no realtime final gate or local-recovery
exemption. Preparation latency reports and public held-owner accessors stay
independent live queries.

The test-only quiescent Readiness observer retains its original diagnostic scan
before gate-owned checks, then reuses that typed resident answer for the pure
mute result instead of querying devices twice. This deliberately reduces only
the diagnostic helper's duplicate scan. Production gate suppliers always
sample live demand at that gate and never accept the quiescent adapter's facts.

Structural channel/effect changes must be followed by a new plan prepared
outside the callback. Send topology
includes every configured Send target, including zero-level sends and empty
Send automation lanes. Gains are read at render time; a silent connection still
participates in feedback validation.

Domain code sends engine commands through the `EngineHandle` trait, never
through a concrete channel. The adapters are the vocabulary:

- `EngineTx` — the real command queue (Arrange, live edits)
- `DiscardingEngine` — the null object, for edits to non-resident content
- `TimelineResultEngine` — Arrange sends, non-resident Section/Clip edits discard, used by
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
