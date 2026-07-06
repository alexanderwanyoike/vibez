# Roadmap: the DAW Alex would replace Ableton with

Captured 2026-07-06 at the end of the stabilization phase (dogfood
bugs #1-#22 all closed, user producing tracks). This is the wishlist
in Alex's own ordering, seeded with implementation notes from the
session that built the current foundation. Whoever picks these up:
read `.notes/PRIME_DIRECTION.md` and the memory notes first; the
decision filter is "does this help finish more usable electronic
tracks per day?"

## 1. Architecture refactor (agreed FIRST, before new features)
Goals in Alex's words: maintainable, isolate bugs, allow testing.
app.rs is ~11k lines holding update(), all views, and orchestration.
Sketch already recorded in PRIME_DIRECTION amendment: domain-split
Message enum with per-domain controllers owning state slices;
extract view families to widgets/ (device cards already moved);
async pipelines (plugin load, warp, project IO) become services with
channel interfaces, unit-testable without iced. Start with a plan
for sign-off, not code.

## 2. Automation + LFOs
Lanes on tracks writing param changes over time. Engine side: the
per-block command pattern already exists; automation is a per-block
param schedule evaluated in EngineTrack::render. Plugin params need
the VST3 IParameterChanges queues (a discard stub exists in
vst3_host/instance.rs to build on) and CLAP param events in
process(); the host already builds event lists for notes.
IComponentHandler must be implemented for GUI-write-back (DPF
plugins already assert about it; see dogfood #22 note).

## 3. Recording in (audio + MIDI in)
cpal input streams exist in the stack. Engine needs an input ring +
punch-in clip writer. MIDI in via midir (already a workspace dep!):
idle instrument rendering (PR #10) means live input will sound while
stopped: that groundwork is done.

## 4. MIDI out
midir output; engine note scheduler already produces timed events
per block (mixer.rs); a MidiOutTrack kind could forward them.

## 5/6. Review all instruments and effects
The cards are honest now (PR #11/#12) but the DSP deserves a pass:
synth filter quality, sampler interpolation, effect algorithm
quality. Keep the "plugins do the deep work" strategy; built-ins are
bread and butter.

## 7. Velocity
Piano roll has velocity visualization already; needs editing (drag
on the velocity lane) and instruments honoring it properly
(SpyInstrument tests show the plumbing carries velocity end-to-end).

## 8-11. Mix view: parametric EQ per strip, bus view, sends
The mix-desk concept is in `.notes/feature-mix-desk.md`. Engine
needs bus tracks (sum groups) and per-strip EQ as a built-in device;
sends = per-track tap into bus inputs with gain. The parametric EQ
sending to busses = send knob per band? Clarify with Alex. A
performance view (clip launcher?) also listed: clarify scope.

## 12. Grid sizes / bars+phrases view
Arrangement ruler currently fixed; Ableton shows adaptive bar/phrase
divisions by zoom. timeline.rs draws the ruler; make divisions a
function of zoom level (1/4 bar .. 8 bars).

## 13. Plugin and sample marketplace
Big. Out of local scope; needs a server story. The sample browser +
Dropbox integration are the seeds.

## 14. Controller support
MIDI learn: midir input -> map CC to Message::SetEffectParam /
SetInstrumentParam / mixer params. The single knob widget
(EffectKnobWidget) makes visual feedback uniform.

## Known small debts
- Undo drops plugin devices (snapshots carry built-ins only).
- DPF fComponentHandler assert noise (ties into automation work).
- Knob drag-feel tuning vs Ableton if it ever bothers again.
- PR #4 umbrella: merge feature branch to main.
