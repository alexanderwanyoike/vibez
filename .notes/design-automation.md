# Design: automation + LFOs

Target: Ableton-style automation lanes on tracks, writing parameter
changes over time, for built-in devices AND plugins. Written as an
implementation spec for a successor session; engine constraints are
the part that must not be guessed.

## Data model (vibez-core)

```rust
pub struct AutomationLane {
    pub id: LaneId,                  // typed_id! like the others
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>, // sorted by beat
}
pub enum AutomationTarget {
    TrackGain, TrackPan,
    EffectParam { effect_id: EffectId, param_index: usize },
    InstrumentParam { param_index: usize },
    PluginParam { effect_id: Option<EffectId>, param_id: u32 }, // see plugin section
}
pub struct AutomationPoint { pub beat: f64, pub value: f32 }
// Linear interpolation between points; hold value past the last.
```
Lanes live on `TrackInfo` (serde `#[serde(default)]` for backcompat,
same trick as PluginDeviceInfo). UI mirror on `UiTrack`.

## Engine evaluation (the RT-critical part)

Per render block, per track, per lane: evaluate the lane at the
block-start beat (samples_per_beat already available via TempoMap)
and apply. NO allocation in the callback: lanes are Vec-stored on
EngineTrack, updated via commands like note clips (Add/Remove/
SetPoint commands, mirrored UI->engine like EngineNoteClip is).

Granularity: block-rate (512 samples ~ 11ms) is fine for v1; do NOT
attempt sample-accurate ramps first pass. Evaluate ONCE per block:
`lane.value_at(current_beat)` (binary search; points sorted).

Built-in application is trivial (set_param paths already exist on
EffectSlot/instrument). The split render at the arrangement loop
boundary (engine.rs process_multitrack) means evaluation must happen
per SEGMENT, not per callback: hook inside render_multitrack_segment.

## Plugin parameters (the hard 40%)

- CLAP: param events (CLAP_EVENT_PARAM_VALUE) pushed into the input
  event list per process() call. The wrapper already builds an event
  list for notes: extend it. Param ids come from clap_plugin_params
  (host currently answers `clap.params` with null: implement the host
  ext minimally: rescan callbacks can be no-ops v1).
- VST3: IParameterChanges input queues. A stub already exists
  (PARAM_CHANGES_STUB in vst3_host/instance.rs): replace with a real
  implementation: one queue per changed param per block, point at
  sample offset 0. Param ids/ranges enumerate via IEditController
  (getParameterCount/getParameterInfo: not yet wired: needed for the
  UI to list automatable params).
- IComponentHandler: implement it (currently missing: DPF plugins
  assert). beginEdit/performEdit/endEdit callbacks are how plugin GUI
  knob moves become recordable automation. Route to a pending queue
  like the GUI resize requests (same pattern, host_context.rs).

## UI

- Lane display under the track (collapsible), canvas like the
  timeline widgets; points draggable, double-click adds, Delete
  removes (route through DeleteKeyPressed context priority).
- Domain: extend arrangement (lanes are track data) or a new
  `automation` domain following REFACTORING-GUIDE.md.
- Record-arm from plugin GUIs comes later via IComponentHandler.

## Order of work

1. Core model + engine eval for TrackGain (audible end-to-end proof).
2. Built-in effect/instrument params.
3. UI lane editor.
4. CLAP param events, then VST3 queues + param enumeration.
5. IComponentHandler + write-from-GUI recording.
Each step has an engine test analog to stuck_note_tests (spy/RMS).
