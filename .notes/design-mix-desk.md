# Design: mix desk (buses, sends, per-strip parametric EQ)

Extends `.notes/feature-mix-desk.md` (the original idea) into an
implementation spec. Alex's vision: the Mix view is a mixing desk;
every strip has a parametric EQ; deep tools stay third-party plugins.

## Open questions for Alex before building

1. "The parametric EQ can send to busses": per-BAND sends (unusual,
   powerful: e.g. send only the highs to reverb) or normal per-strip
   sends alongside the EQ? Assume per-strip sends v1 unless he says
   otherwise.
2. Performance view: clip launcher (Ableton session view) or a
   performance-oriented mixer layout? Do not guess: ask.

## Engine: bus graph

```rust
// EngineTrack gains:
pub output: OutputTarget,          // Master | Bus(BusId)
pub sends: Vec<Send>,              // post-fader taps
pub struct Send { pub bus: BusId, pub gain: f32, pub enabled: bool }
```
Buses are EngineTracks of a new kind (no clips/instrument; effects +
gain/pan yes), summed AFTER regular tracks in process order:
1. render regular tracks -> accumulate into their target bus buffer
   (or master) AND into each enabled send's bus buffer at send gain.
2. render buses in index order (bus->bus sends only allowed to
   HIGHER-index buses to keep the graph acyclic and one-pass).
3. buses sum to master.
RT constraints: bus buffers preallocated on AddBus; no allocation in
the callback; commands mirror track commands (AddBus, SetSend...).

Serialization: TrackInfo gains `output` + `sends`; a `buses:
Vec<BusInfo>` lands on Project (serde defaults for backcompat, with
a roundtrip + pre-bus schema test like plugin_devices_roundtrip).

## Parametric EQ (vibez-dsp)

Per-strip built-in, NOT an effect slot: 4 bands v1, each band:
biquad (RBJ cookbook), types low-shelf / bell / high-shelf (band 1
LP/HP option), freq/gain/Q. State: [Biquad; 4] per channel,
recompute coefficients on param change (command), process in chain
position 0 before effect slots. dsp tests: white-noise spectrum
before/after (FFT via existing onset tooling or plain goertzel at
band centers).

## UI (Mix workspace exists already)

- Strip: input meter, 4-band EQ mini-display (reuse the AdsrScope
  drawing approach: curve = sum of band responses, log-x), sends
  knobs (param_column), fader, pan, mute/solo, output selector.
- EQ detail: click the mini-display opens a large editor in the
  detail strip: draggable band handles on the curve (the piano-roll
  canvas interaction patterns apply).
- New `mix` domain module per REFACTORING-GUIDE.md.

## Order of work

1. Engine buses + sends + tests (RMS through a bus == direct).
2. TrackInfo/Project schema + persistence tests.
3. Mix strips get output selector + send knobs.
4. Biquad EQ in dsp + engine wiring + tests.
5. Strip mini-curve + detail editor.
6. Per-band sends only if Alex confirms (question 1).
