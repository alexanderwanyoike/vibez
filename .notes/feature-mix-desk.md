# Feature idea: Mix view as a mixing desk

Captured 2026-07-04, during warp-fix verification. Status: idea, not
scheduled. Explicitly a feature, not part of the current
stabilization pass.

## The idea

Make the Mix section a proper mixing desk, console metaphor rather
than Ableton's "EQ is just another device in the chain":

- Channel strips (already exist).
- A built-in PARAMETRIC EQ on every channel strip, part of the strip
  itself like a real desk, not a device you insert.
- No built-in graphical EQ. Graphical EQs remain supported via
  third-party plugins (e.g. LSP) for whoever wants one.

## Why it's differentiating

Every software DAW treats EQ as an insert. A desk-native parametric
EQ per strip matches how people actually mix (reach for the channel
EQ first) and fits the throughput thesis in PRIME_DIRECTION.md:
fewer menus between "this sounds muddy" and the cut.

## Open questions

- Band count / layout for the strip EQ (classic 4-band sweepable mid
  desk EQ vs full parametric).
- DSP: vibez-dsp already has an EQ effect; promote/rework it into the
  strip, or new implementation?
- Where does it sit in signal flow relative to the insert chain
  (desk convention: EQ post-inserts, pre-fader; decide explicitly).
- Interaction with the knob-feel fix (dogfood bug #2): the strip EQ
  will live or die on knob feel.

## Precondition being tested now

How well VST3/CLAP hosting holds up in a real tune (LSP plugins as
the guinea pig). If third-party EQs host cleanly, the "no built-in
graphical EQ" stance is safe.
