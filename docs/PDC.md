# Plugin delay compensation

The timing plan extends the shared sidechain graph. Each device contributes its
reported processing latency; incoming main, external-input, send and mix paths
receive the waiting needed to meet at the same processing point. Built-in and
hosted instruments and effects participate in the same calculation.

Host bypass preserves a device's applied latency while passing delayed dry
audio. Removing the device removes its contribution. The shared device readout
shows cached latency in samples and milliseconds at the current sample rate:
480 samples means 10.00 ms at 48 kHz or 5.00 ms at 96 kHz. An explicit zero is
valid; an unavailable device report is displayed as unavailable.

Runtime plans are rebuilt from current device metadata rather than a saved
latency table. CLAP and VST3 adapters cache reports during their permitted
activation lifecycle. A restart request stops processing on its processing
thread, returns the instance for main-thread reconfiguration, and prepares a
replacement plan before resuming. Preparation rejects excessive reports instead
of clamping them into an apparently aligned configuration.

## Reduced Latency Monitoring

Full compensation is the default for new and legacy projects. Reduced Latency
Monitoring is one project setting under Settings > Project > Monitoring. It is
saved in both legacy JSON and the current project container, shared by Arrange
and Perform, and carried by project snapshots for Undo and Redo.

Eligibility follows actual live routes. Perform keeps its remembered playable
Instrument target when an unrelated Audio track is selected. An open USB MIDI
input can also have a different selected playable Instrument target; both routes
are included without treating ordinary selection as a live input route.

| Audio monitoring | Eligible for reduced monitoring |
| --- | --- |
| Off | No |
| Auto | The armed hardware-input track |
| On | The active monitored or armed hardware-input track |

Return buses, Master, unmonitored tracks and internal resampling destinations do
not acquire an exception merely through selection. A monitored track's clips and
live signal share its device chain and its accepted timing exception. Required
main/detector alignment remains in place, and sends and return paths remain
compensated. This mode does not remove processing delay inside the live chain.
Offline rendering uses full compensation regardless of the saved monitoring
preference.

## Storage and graph limits

These are checked implementation bounds, not measured hardware latency or a
monitoring preference. A rejected plan reports its failure rather than silently
reducing a declared delay.

| Resource | Limit |
| --- | ---: |
| One device's reported latency | 1,048,576 frames |
| Accumulated graph path latency | 4,194,304 frames |
| Combined compensation delay/history/control accounting | 134,217,728 float-equivalent samples, 512 MiB |
| Shared routing sample storage | 256 MiB |
| Prepared routing block capacity | 65,536 frames |
| Supported external inputs per effect | 64 mono/stereo inputs |

The compensation budget accounts for delay lines, bypass history, presentation
and channel clocks, control history and scratch storage. Plugin-owned DSP memory
is separate. The app prepares 4,096-frame graph capacity; offline processing uses
512-frame blocks. The deterministic checks include one-frame and odd segments,
so a device's negotiated block-size promise must cover those calls.

## Validation commands

Project and monitoring coverage can run without sound hardware:

```sh
cargo test -p vibez-project compensation
cargo test -p vibez-ui compensation
cargo clippy -p vibez-project -p vibez-ui --all-targets -- -D warnings
cargo fmt --all -- --check
```

The focused project checks cover new/legacy defaults, JSON and container reopen,
separate monitoring/bypass Undo and Redo edits, remembered Instrument and USB
routes, Audio On/Auto/Off eligibility, and cached readouts. Readout checks preserve
the same sample report during host bypass and recompute milliseconds when the
sample rate changes; they do not measure sound-card round-trip latency.

Native timing coverage uses the real engine and loadable format fixtures:

```sh
cargo test -p vibez-engine compensation
cargo test -p vibez-engine independent_clip_wraps
cargo test -p vibez-engine render
cargo test -p vibez-plugin-host --test latency
cargo test -p vibez-plugin-host --test compensation
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Executed native checks include controlled 137- and 521-frame delays, parallel
cancellation across both plugin formats, instrument latency, main/detector
arrival alignment, host bypass, restart lifecycle, fresh activation at
44.1/48/96 kHz, and allocation checks around host rendering. Negative controls
use uncompensated or incorrectly reported delay to prove the sample oracle can
detect an error. Clip-boundary checks exercise effect, gain, pan, send and musical
context timing with a 137-frame source path and a 521-frame mix path.
Additional controlled cases retain the natural 137-frame reduced-monitoring
onset after cold Arrange start and seek against a 521-frame mix. A late
bar-quantized Clip request skips its missed boundary, and delayed Section note
notifications use the previously audible Section context. Native fixture checks
process and stop on a separate audio thread while reactivation and destruction
remain on the original main thread, with zero recorded lifecycle violations for
both formats. The presentation-event queue has 2,048 preallocated slots.

Independent Linux adapter probes at 48 kHz with a 512-frame maximum block size
recorded the following installed-plugin impulse observations:

| Installed plugin and setting | Report | First nonzero frame | Peak frame |
| --- | ---: | ---: | ---: |
| LSP Sidechain Compressor Stereo, lookahead parameter 18 = 0.5 | 480 | 480 | 480 |
| ZL Compressor, lookahead parameter 27 = 1 | 960 | 960 | 960 |
| ZL Compressor, oversampling parameter 26 = 1 | 76 | 40 | 76 |
| LSP Sidechain Multiband Compressor, default settings | 0 | 0 | 0 |

Those observations establish adapter report/response evidence for the installed
builds. A real processor's first nonzero transient can precede its declared
latency, as the oversampling case shows; arbitrary filters and compressors are
not pure-delay cancellation oracles.

The deterministic Bounce checks load CLAP and VST3 instruments and effects in all
four format pairings with 137-frame detector latency and a 521-frame receiver.
They compare every sample, preserve first/last impulses and intentional silence,
retain earlier detector state, and compare a partial 1,000..1,100 range with the
same full render. An incorrect 522-frame report fails that oracle.

Capture stores heard-time events and the applied plan's per-track source offsets.
Section/Clip transitions publish when their relevant audio reaches output; early
Section contributions are retained even when Capture starts or stops before the
common mix boundary. A full-PDC Bounce of materialized Section audio and live
notes checks every sample against the heard relationship. This replay oracle
uses full compensation. Turning Reduced Latency Monitoring on again applies its
current live-route exception to the resulting Arrange track as well, so it can
make that direct branch earlier again; compensated returns remain independent.

Native rate-sensitive devices retain their constructor rate, and hosted devices
retain their activation rate and processing-thread identity. Reuse at another
rate or on a different processing thread fails the applied configuration check,
suppresses project playback/monitoring, identifies the cause and preserves project
state and owners. Matching an old latency report does not make that configuration
valid. Recreate wrong-rate devices or reopen the saved project at the chosen rate.
A hosted instance must stop on its original processing thread before main-thread
reactivation; if stream replacement has already removed that worker, restart the
host after saving the project. Automatic live migration between output workers
or sample rates is outside this guarded recovery path.

The fixtures and installed adapter probes above do not establish complete Bounce
sessions for the installed third-party processors. Cross-platform CI belongs to
the final integrated head; local native checks alone do not establish Windows or
macOS hardware behavior.
