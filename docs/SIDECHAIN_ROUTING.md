# Sidechain routing

Each supported external effect input has an independent source and tap selector
inside its device card. Compressor and Gate expose one stereo input. CLAP and
VST3 effects expose their declared mono/stereo auxiliary inputs, using the same
host controls and input meters. The plugin's own external detector setting must
also be enabled when its design requires that setting.

Tracks and return buses can supply trigger audio. Effects on tracks, buses and
Master can receive it. Master and Browser Audition are not source choices.

| Tap | Source effects | Volume and pan | Explicit mute |
| --- | --- | --- | --- |
| Before Effects | Excluded | Excluded | Trigger preserved |
| After Effects, default | Included | Excluded | Trigger preserved |
| After Fader | Included | Included | Trigger silenced |

A muted ghost source therefore continues triggering pre-fader routes. Solo keeps
required sources processing without adding their direct output to the audible
mix. During track solo, a non-soloed bus required as a detector source is also
inaudible; this suppresses that bus's wet return when it serves both purposes.
Soloing the bus explicitly auditions its full mix. This does not change saved
mute, solo or send settings.

Mono is copied into stereo; stereo is averaged into mono. Input meters measure
the adapted audio delivered to each input, independently of gain reduction or
whether a third-party plugin has selected external detection. Disconnecting a
built-in input restores detection from its main signal. A connected silent or
missing source continues delivering external silence.

Routes are saved with effect, input and source identities. Source deletion keeps
its original name and identity, marked missing. Missing plugins or incompatible
inputs retain inactive assignments instead of moving them to another input.
A compatible restored input that would introduce feedback retains its assignment
as inactive external silence. Established active routes win; the inactive decision
is saved so reopening or plugin load order cannot change the winner. Choosing a
valid source or tap deliberately reactivates that input.
Undo restores routing alongside the source or effect. Source and tap edits take
effect at a block boundary without recreating the receiving device.

## Graph and processing

`vibez-core::routing` describes source/sum, individual effect, after-effects and
after-fader nodes. Main, auxiliary, send and mix edges share one dependency
graph. The selectors and prepared engine graph use the same validation. Actual
cycles are excluded; a same-track Before Effects route remains valid.

`AudioEffect` and `PluginInstance` declare cached input descriptors and accept
external input blocks. Unsupported built-ins opt out by default. Compressor,
Gate and the format adapters share route preparation, layout adaptation,
persistence and presentation rather than owning separate routing systems.

Plans and buffers are prepared outside rendering. Runtime input delivery reuses
owned storage, and retired plans return to the UI for destruction. A plan
supports up to 64 external inputs per effect, 65,536 frames of prepared block
capacity and 256 MiB of host routing sample storage. The app prepares 4,096-frame
capacity; larger callbacks are segmented. These limits exclude plugin-owned DSP
memory. Preparation errors reject the new plan instead of silently clamping it.

Plugin delay compensation is a separate feature. Sidechain routing establishes
exact delivery for zero-latency paths; it does not align paths with different
device processing delays.

## Bounce

Track and clip Bounce keep their required sources, bus contributors and nested
sidechain dependencies. The renderer uses the playback graph and processes prior
project history before writing the requested range. Only the selected direct
output is captured for a stem, before return summing and Master processing.
Existing target mute/solo overrides for stems remain in place; source mute,
gain, pan and tap rules still apply.

The UI conservatively prepares potential plugins. The renderer resolves the
actual dependency closure from loaded input descriptors, so an inactive stale
assignment does not make an unrelated failed plugin necessary. A source tapped
Before Effects does not require inserts beyond that tap. Required plugin
failures identify the affected device, and isolated plugin owners return through
the main-thread export lifecycle.

## Validation

Run the deterministic host and engine coverage without audio hardware:

```sh
cargo test -p vibez-engine
cargo test -p vibez-plugin-host --test routing
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

`vibez-routing-fixture` builds loadable CLAP and VST3 modules using their SDK
bindings. The tests exercise the production loaders and renderer, independent
mono/stereo inputs, unsupported declarations, bus activation, all source taps,
muted/solo sources and exact note-frame delivery across both instrument formats.
They include one-frame and odd segments and count host Rust allocations during
input delivery. Offline tests cover selected-output isolation, bus dependencies,
missing assignments and exact partial-range history.

Optional independent-plugin checks use environment paths and remain ignored in
ordinary CI:

```sh
VIBEZ_LSP_CLAP=/path/to/lsp-plugins.clap \
VIBEZ_ZL_VST3=/path/to/ZL-Compressor.vst3 \
cargo test -p vibez-plugin-host --test installed_routing \
  independently_installed_effects -- --ignored --nocapture

VIBEZ_SURGE_CLAP=/path/to/Surge-XT.clap \
VIBEZ_SURGE_VST3=/path/to/Surge-XT.vst3 \
cargo test -p vibez-plugin-host --test installed_routing \
  independently_installed_instruments -- --ignored --nocapture
```

Measured on Linux x86_64 at 48 kHz with 64-frame blocks: LSP Sidechain Compressor
Mono/Stereo descriptor 1.0.34 and Sidechain Gate Stereo 1.0.33, ZL Compressor
module 0.5.0, and installed Surge XT 1.3.4 in both formats. With the test's stated
external detector settings, a 0.001 main signal changed as follows:

| Effect | Silent external input | External amplitude 1 | Recovered silence |
| --- | ---: | ---: | ---: |
| LSP Compressor Mono/Stereo | 0.001000001 | 0.000125893 | 0.001000001 |
| LSP Gate Stereo | 0.000001000 | 0.001000001 | 0.000001000 |
| ZL Compressor | 0.001000001 | 0.000085770 | 0.001000001 |

The tests select zero lookahead, no oversampling and disabled detector filtering
where those settings exist, print the actual parameter values and save opaque
plugin state for reproduction. Surge's default initialized patch, note 60 at
velocity 100, produced nonzero receiving-input meters with its source muted and
no audible source leakage. Its varying synth peaks are not a sample-timing
oracle. These checks establish measured routing/DSP behavior for those installed
builds, not listening-session, latency-compensation or universal compatibility
evidence. Deterministic fixtures remain the cross-platform release checks;
macOS and Windows independent builds require their own validation.

Bounce and Export both reject missing required media or devices before committing
their destination. A silent connected detector is intentional; missing detector
media is a failed render, so a successful Bounce never carries warning-only audio.
