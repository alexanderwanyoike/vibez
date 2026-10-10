# Plugin delay compensation

The timing plan extends the shared sidechain graph. Each device contributes its
reported processing latency; incoming main, external-input, send and mix paths
receive the waiting needed to meet at the same processing point. Built-in and
hosted instruments and effects participate in the same calculation.

Host bypass preserves a device's applied latency while passing delayed dry
audio. Removing the device removes its contribution. The shared device readout
shows cached latency in samples and milliseconds at the current sample rate:
480 samples means 10.00 ms at 48 kHz or 5.00 ms at 96 kHz. An explicit zero is
valid; an unavailable device report is displayed as unavailable. Bypassed inserts
skip wet DSP. Unbypass resets paused wet history and keeps aligned dry audio during
the reported preparation delay, then fades wet processing back over 64 samples.

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

Eligibility follows actual live routes. The keyboard/pad Instrument route is
active in the Perform workspace's Instrument mode. Its remembered playable
target survives unrelated Audio-track selection; remembering a target in
Arrange, Mix, Sections mode or Track Mutes mode does not make it monitored.
An open USB MIDI input follows its existing selected playable Instrument
destination. MIDI delivery and timing eligibility share that destination
resolver. Ordinary selection without a configured live route does not republish
timing, and full compensation ignores live-target changes. Genuine configured
input-target changes keep following the existing adapter and coordinated timing
transition; eligibility does not flicker for individual notes or silent blocks.

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
and channel clocks, control history and scratch storage. A reduced direct path
retains its full waiting capacity, so disabling the exception can use audio
already in history. Plugin-owned DSP memory is separate. The app prepares 4,096-frame graph capacity; offline processing uses
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
both formats. The presentation-event queue has 2,048 preallocated slots. Overflow
stops playback, closes Capture and retains owners for UI disposal without
invalidating a sound timing plan. Play can restart once queue capacity returns.
Ready events drain in one batch bounded by available UI slots.

Compatible plan changes retain clock, control and audio history and crossfade
changed direct output waits over 64 samples. Changes requiring new processing
timing fade around the necessary preparation interval. Running connection,
Instrument selection and monitoring-setting tests check continuous constant
output through the real App publication path. Those tests supply the MIDI
adapter's connection state without opening a hardware MIDI port.

Effect control ramps use intervals of at most 64 samples, scaled to about
1.33 ms at normal device rates. Musical context changes and history boundaries
still split processing at their actual sample. Idle effects process whole stable
blocks, and gain, pan, mute and Send envelopes retain per-frame evaluation.

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

Clip and Section recorders retain immediate source-delivery coordinates, so a
note cannot fall behind its source count-in or arrive after that take has closed.
Capture consumes separate heard notifications and the applied plan's per-track
source offsets.
Section/Clip transitions publish when their relevant audio reaches output; early
Section contributions are retained even when Capture starts or stops before the
common mix boundary. A full-PDC Bounce of materialized Section audio and live
notes checks every sample against the heard relationship. This replay oracle
uses full compensation. Turning Reduced Latency Monitoring on again applies its
current live-route exception to the resulting Arrange track as well, so it can
make that direct branch earlier again; compensated returns remain independent.

Native rate-sensitive devices retain their constructor rate, and hosted devices
retain their activation configuration. Sequential worker replacement at a
compatible rate, block size and layout preserves device state, delay history and
cached latency while the engine remains exclusively owned. CLAP's audio-thread
role belongs to the current call, rather than one permanent OS thread.

Wrong-rate reuse and invalid activation configuration suppress project
playback/monitoring, identify the cause and preserve project state and owners.
A transient native start/process failure silences only its affected device.
The host allows two automatic main-thread reactivation attempts per instance;
it never retries invalid native DSP in the audio callback. An exhausted device
remains locally silent until explicit reload. Compatible recovery preserves the
other paths' delay and clock history.
Matching an old latency report does not make that configuration valid. Recreate
wrong-rate devices or reopen the saved project at the chosen rate; automatic live
sample-rate migration remains outside this guarded recovery path. Bounce checks
the same applied configuration before DSP and rejects incompatible reuse while
returning its device owners.

Stream teardown takes the engine on the UI owner thread after excluding any
in-flight callback. Retained callback clones then observe an empty slot. Hosted
processors stop before main-thread deactivation and destruction, with a truthful
exclusive CLAP audio role for the stop call. VST3 permits stopping on the UI or
processing thread.

The fixtures and installed adapter probes above do not establish complete Bounce
sessions for the installed third-party processors. Cross-platform CI belongs to
the final integrated head; local native checks alone do not establish Windows or
macOS hardware behavior.

Device reconfiguration keeps an explicit in-flight owner identity. The UI rejects
an obsolete owner before reactivation when reset, removal or replacement has
already been queued; the engine independently rejects its delayed return. Those
returns retire the device, unused prepared routing and storage on main. Effect
bypass and order edits made during the handoff remain current. Main prepares
restoration capacity before returning the device so a valid insert beside the
held slot does not allocate or release a buffer in the callback.
