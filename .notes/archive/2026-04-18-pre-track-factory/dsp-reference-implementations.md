# Open Source DSP Reference Implementations for Vibez DAW

## Instruments

### 1. FM Synth

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **OctaSine** | Rust | AGPL-3.0 | [github.com/greatest-ape/OctaSine](https://github.com/greatest-ape/OctaSine) |
| **Dexed** | C++ (JUCE) | GPL-3.0 (msfa core: Apache-2.0) | [github.com/asb2m10/dexed](https://github.com/asb2m10/dexed) |
| **web-synth FM** | Rust (WASM) | MIT | [github.com/Ameobea/web-synth](https://github.com/Ameobea/web-synth) |
| **libfmsynth** | C | MIT | [github.com/Themaister/libfmsynth](https://github.com/Themaister/libfmsynth) |

**Best references:**
- **OctaSine** — production-quality 4-operator FM synth in pure Rust (VST2/CLAP). Clean operator/modulation matrix code, DSP core isolated from GUI.
- **Dexed** — canonical DX7 emulation. The `msfa` core (Apache-2.0) has all 32 DX7 algorithms. Most battle-tested FM reference in existence.
- **libfmsynth** — tiny dependency-free C library, pure FM synthesis math. Ideal for understanding the raw algorithm.

### 2. Subtractive Synth

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Surge XT** | C++ | GPL-3.0 | [github.com/surge-synthesizer/surge](https://github.com/surge-synthesizer/surge) |
| **Actuate** | Rust (nih-plug) | GPL-3.0 | [github.com/ardura/Actuate](https://github.com/ardura/Actuate) |
| **amsynth** | C++ | GPL-2.0+ | [github.com/amsynth/amsynth](https://github.com/amsynth/amsynth) |
| **FunDSP** | Rust | MIT / Apache-2.0 | [github.com/SamiPerttu/fundsp](https://github.com/SamiPerttu/fundsp) |

**Best references:**
- **Surge XT** — professional-grade hybrid synth. World-class `SurgeVoice` and filter implementations.
- **Actuate** — Rust subtractive/additive synth + sampler built with nih-plug. SVF filters, ADSR, FM options, LFOs. Directly relevant architecture.
- **FunDSP** — Rust DSP library with Moog ladder filters, oscillators, envelopes. Great for individual building blocks. Dual MIT/Apache-2.0.

### 3. Wavetable Synth

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Vital / Vitalium** | C++ (JUCE) | GPL-3.0 | [github.com/mtytel/vital](https://github.com/mtytel/vital) |
| **Surge XT** | C++ | GPL-3.0 | [github.com/surge-synthesizer/surge](https://github.com/surge-synthesizer/surge) |
| **Yazz** | Rust | GPL-3.0 | [github.com/icsga/Yazz](https://github.com/icsga/Yazz) |

**Best references:**
- **Vital** — gold standard for wavetable synthesis. Spectral warping, morphing, unison, modulation engine. DSP in `src/synthesis/`.
- **Yazz** — Rust wavetable synth, 3 oscillators per voice, 32-voice polyphony. Smaller codebase, easier to study.

### 4. Sampler

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **HISE** | C++ (JUCE) | GPL-3.0 | [github.com/christophhart/HISE](https://github.com/christophhart/HISE) |
| **soakyaudio/sampler** | Rust | MIT | [github.com/soakyaudio/sampler](https://github.com/soakyaudio/sampler) |
| **rust-sampler (SFZ)** | Rust | MIT | [github.com/emurray2/rust-sampler](https://github.com/emurray2/rust-sampler) |
| **Actuate** | Rust (nih-plug) | GPL-3.0 | [github.com/ardura/Actuate](https://github.com/ardura/Actuate) |

**Best references:**
- **HISE** — full toolkit for sample-based instruments. Multi-sample mapping, velocity zones, round-robin, disk streaming.
- **soakyaudio/sampler** — full-featured Rust sampler with pitch shifting and voice management. Clean code, MIT licensed.

### 5. Drum Machine (Multi-Sampler)

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **KrumSampler** | C++ (JUCE) | GPL-3.0 | [github.com/krismakesstuff/KrumSampler](https://github.com/krismakesstuff/KrumSampler) |
| **jdrummer** | C++ (JUCE) | N/A | [github.com/jmantra/jdrummer](https://github.com/jmantra/jdrummer) |
| **rudiments** | Rust | MIT | [github.com/jonasrmichel/rudiments](https://github.com/jonasrmichel/rudiments) |

**Best references:**
- **KrumSampler** — purpose-built drum sampler with drag-and-drop, per-pad controls, pad grid. Clean architecture.
- **rudiments** — Rust drum machine with step sequencing. Simple and readable, good Rust baseline.

---

## Effects

### 6. Compressor

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **sndfilter** | C | 0BSD | [github.com/velipso/sndfilter](https://github.com/velipso/sndfilter) |
| **CTAGDRC** | C++ (JUCE) | GPL-3.0 | [github.com/p-hlp/CTAGDRC](https://github.com/p-hlp/CTAGDRC) |
| **Airwindows** | C++ | MIT | [github.com/airwindows/airwindows](https://github.com/airwindows/airwindows) |
| **lamb-rs** | Rust (nih-plug) | GPL-3.0 | [github.com/magnetophon/lamb-rs](https://github.com/magnetophon/lamb-rs) |

**Best references:**
- **sndfilter** (`compressor.c`) — 0BSD (public-domain equivalent), pure C, no deps. Derived from Chromium's DynamicsCompressorKernel. Covers RMS detection, envelope, knee, attack/release, makeup gain. Extremely clean and readable.
- **CTAGDRC** — detailed docs explaining DSP math: time constants, smoothing, gain reduction. Clean JUCE plugin.

### 7. Reverb

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **VitaliumVerb** | Rust (nih-plug) | GPL-3.0 | [github.com/BillyDM/vitalium-verb](https://github.com/BillyDM/vitalium-verb) |
| **mverb** | C++ | BSD-3-Clause | [github.com/martineastwood/mverb](https://github.com/martineastwood/mverb) |
| **sndfilter** | C | 0BSD | [github.com/velipso/sndfilter](https://github.com/velipso/sndfilter) |

**Best references:**
- **VitaliumVerb** — Rust port of Vital's reverb. Detailed blog post by BillyDM on porting process. Best Rust reverb reference.
- **mverb** — single header (`mverb.h`), Dattorro plate reverb. BSD-3-Clause, trivially readable.
- **sndfilter** (`reverb.c`) — Freeverb-style, 0BSD license.

### 8. Bit Crusher

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Airwindows (DeRez2)** | C++ | MIT | [github.com/airwindows/airwindows](https://github.com/airwindows/airwindows) |
| **distortion-rs** | Rust + C++ | MIT | [github.com/jatinchowdhury18/distortion-rs](https://github.com/jatinchowdhury18/distortion-rs) |

**Best references:**
- **Airwindows DeRez2** — single-file bitcrusher/sample-rate reducer, MIT. Smooth transitions.
- Core algorithm: `output = floor(input * levels) / levels` for bit reduction + hold-and-skip for sample rate reduction.

### 9. Distortion

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Airwindows** | C++ | MIT | [github.com/airwindows/airwindows](https://github.com/airwindows/airwindows) |
| **CHOW** | C++ (JUCE) | GPL-3.0 | [github.com/Chowdhury-DSP/CHOW](https://github.com/Chowdhury-DSP/CHOW) |
| **synfx-dsp** | Rust | MIT / Apache-2.0 | [docs.rs/synfx-dsp](https://docs.rs/synfx-dsp) |

**Best references:**
- **Airwindows** — dozens of distortion variants (Drive, Density, Spiral, Tape), each single-file MIT.
- **synfx-dsp** — waveshaping functions in Rust with documented sources. MIT/Apache-2.0.

### 10. Flanger

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Audio-Effects** | C++ (JUCE) | GPL-3.0 | [github.com/juandagilc/Audio-Effects](https://github.com/juandagilc/Audio-Effects) |
| **Calf Studio Gear** | C++ | LGPL-2.1 | [github.com/calf-studio-gear/calf](https://github.com/calf-studio-gear/calf) |
| **FunDSP** | Rust | MIT / Apache-2.0 | [github.com/SamiPerttu/fundsp](https://github.com/SamiPerttu/fundsp) |

**Best references:**
- **Audio-Effects** — textbook companion (Reiss & McPherson). Clean, well-commented. A flanger = short delay (1-10ms) + LFO-modulated delay time + feedback.
- **Calf Flanger** — production-quality LV2 plugin, stereo, multiple LFO shapes, feedback controls. LGPL-2.1.

### 11. Tremolo

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Audio-Effects** | C++ (JUCE) | GPL-3.0 | [github.com/juandagilc/Audio-Effects](https://github.com/juandagilc/Audio-Effects) |
| **Calf Pulsator** | C++ | LGPL-2.1 | [github.com/calf-studio-gear/calf](https://github.com/calf-studio-gear/calf) |
| **Airwindows** | C++ | MIT | [github.com/airwindows/airwindows](https://github.com/airwindows/airwindows) |

**Best references:**
- Core algorithm: `output = input * (1.0 - depth * lfo(t))`. Complexity comes from LFO shapes (sine, triangle, square, S&H) and tempo sync.

### 12. Filter

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **DSPFilters** | C++ | MIT | [github.com/vinniefalco/DSPFilters](https://github.com/vinniefalco/DSPFilters) |
| **sndfilter** | C | 0BSD | [github.com/velipso/sndfilter](https://github.com/velipso/sndfilter) |
| **synfx-dsp** | Rust | MIT / Apache-2.0 | [docs.rs/synfx-dsp](https://docs.rs/synfx-dsp) |
| **FunDSP** | Rust | MIT / Apache-2.0 | [github.com/SamiPerttu/fundsp](https://github.com/SamiPerttu/fundsp) |
| **augmented-dsp-filters** | Rust | MIT | [crates.io/crates/augmented-dsp-filters](https://crates.io/crates/augmented-dsp-filters) |

**Best references:**
- **DSPFilters** — THE filter reference. RBJ biquads, Butterworth, Chebyshev, Elliptic, all filter types. MIT, no deps, no allocations.
- **sndfilter** (`biquad.c`) — core RBJ formulas from Chromium in clearest possible form. 0BSD.
- **synfx-dsp** / **FunDSP** — SVF and biquad implementations in Rust. SVF is more stable at high frequencies.

### 13. Autopan

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **Calf Pulsator** | C++ | LGPL-2.1 | [github.com/calf-studio-gear/calf](https://github.com/calf-studio-gear/calf) |
| **Panacea** | Csound | GPL-3.0 | [github.com/consint/Panacea](https://github.com/consint/Panacea) |

**Best references:**
- **Calf Pulsator** — autopanner/tremolo hybrid, LFO modulates L/R volumes for stereo movement. LGPL-2.1, production-quality.
- Core algorithm: `left = input * (0.5 + 0.5 * lfo(t))`, `right = input * (0.5 - 0.5 * lfo(t))`. Add depth, LFO shape, tempo sync.

### 14. Phaser

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **ChowPhaser** | C++ (JUCE) | BSD-3-Clause | [github.com/jatinchowdhury18/ChowPhaser](https://github.com/jatinchowdhury18/ChowPhaser) |
| **Calf Phaser** | C++ | LGPL-2.1 | [github.com/calf-studio-gear/calf](https://github.com/calf-studio-gear/calf) |
| **Audio-Effects** | C++ (JUCE) | GPL-3.0 | [github.com/juandagilc/Audio-Effects](https://github.com/juandagilc/Audio-Effects) |

**Best references:**
- **ChowPhaser** — detailed Medium article explaining DSP. BSD-3-Clause. Feedback + all-pass cascade + LFO + nonlinear processing.
- **Audio-Effects Phaser** — cascade of all-pass filters with LFO-swept cutoffs. Simplest textbook implementation.
- A phaser = N all-pass filters in series with LFO-swept cutoff frequencies (typically 4-12 stages).

### 15. Gate

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **JUCE NoiseGate** | C++ | ISC / GPL-3.0 | [github.com/juce-framework/JUCE](https://github.com/juce-framework/JUCE) |
| **Airwindows** | C++ | MIT | [github.com/airwindows/airwindows](https://github.com/airwindows/airwindows) |

**Best references:**
- **JUCE NoiseGate** (`juce_NoiseGate.h`) — clean, documented gate with threshold, ratio, attack, release. Template-based, very readable.
- A gate = compressor with high ratio applied BELOW threshold. Core: detect level -> compare threshold -> apply attack/release envelope -> multiply signal.

### 16. Delay

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **dm-SpaceEcho** | Rust | GPL-3.0 | [github.com/davemollen/dm-SpaceEcho](https://github.com/davemollen/dm-SpaceEcho) |
| **FunDSP** | Rust | MIT / Apache-2.0 | [github.com/SamiPerttu/fundsp](https://github.com/SamiPerttu/fundsp) |
| **synfx-dsp** | Rust | GPL-3.0+ | [docs.rs/synfx-dsp](https://docs.rs/synfx-dsp) |
| **Airwindows (TapeDelay2)** | C++ | MIT | [github.com/airwindows/airwindows](https://github.com/airwindows/airwindows) |
| **QDelay** | C++ (JUCE) | GPL-3.0 | [github.com/tiagolr/qdelay](https://github.com/tiagolr/qdelay) |
| **Cocoa Delay** | C++ (WDL-OL) | MIT | [github.com/tesselode/cocoa-delay](https://github.com/tesselode/cocoa-delay) |
| **Audio-Effects** | C++ (JUCE) | GPL-3.0 | [github.com/juandagilc/Audio-Effects](https://github.com/juandagilc/Audio-Effects) |
| **ffTapeDelay** | C++ (JUCE) | BSD-3-Clause | [github.com/ffAudio/ffTapeDelay](https://github.com/ffAudio/ffTapeDelay) |

**Best references:**
- **dm-SpaceEcho** — best Rust reference. Full delay+reverb modeled after the Roland Space Echo, pure Rust DSP core with tape characteristics. Also see dm-GrainDelay and dm-Reverse from the same author.
- **synfx-dsp** — Rust `DelayBuffer` with linear and cubic (Hermite) interpolation, plus `AllPass`/`Comb` filters built on delay lines.
- **FunDSP** — MIT-licensed Rust DSP with `delay(t)`, `tick()`, `tap(min, max)` with cubic interpolation, `feedback()`, `fdn()` (feedback delay network). No copyleft.
- **Airwindows TapeDelay2** — minimal MIT C++ tape delay. Simulates physical tape loop that speeds up/slows down rather than resizing buffer (authentic pitch-swerve). Bandpass tone shaping in feedback path.
- **QDelay** — most feature-complete open-source delay. Dual stereo, ping-pong, reverse, feedback EQ, diffusion, modulation, pitch shifting, wow/flutter, saturation.
- **Cocoa Delay** — clean MIT impl with static/ping-pong/circular pan modes, wow/flutter, wet signal ducking.
- Core algorithm: circular buffer of `delay_time * sample_rate` samples. Read with interpolation, feedback = `delayed * gain` routed back to input, optionally filtered/saturated. Ping-pong: cross-feed L↔R. Tape: LFO-modulated read position (wow/flutter), LP filter in feedback (high-freq loss per pass), soft-clip saturation. Tempo sync: `delay_ms = (60000 / bpm) * note_fraction`.

### 17. Limiter

| Project | Language | License | URL |
|---------|----------|---------|-----|
| **lamb-rs** | Rust (nih-plug) | GPL-3.0 | [github.com/magnetophon/lamb-rs](https://github.com/magnetophon/lamb-rs) |
| **nih-plug Safety Limiter** | Rust | ISC | [github.com/robbert-vdh/nih-plug](https://github.com/robbert-vdh/nih-plug) |
| **sndfilter** | C | 0BSD | [github.com/velipso/sndfilter](https://github.com/velipso/sndfilter) |

**Best references:**
- **lamb-rs** — Rust lookahead compressor/limiter with nih-plug. Most complete Rust limiter reference.
- **nih-plug Safety Limiter** — simple brickwall limiter, ISC licensed. Good minimal starting point.
- A limiter = compressor with infinite ratio + zero attack (or lookahead).

---

## Cross-Cutting Reference Libraries

| Project | Language | License | Covers |
|---------|----------|---------|--------|
| **sndfilter** | C | 0BSD | Compressor, Reverb, Biquad Filters |
| **FunDSP** | Rust | MIT / Apache-2.0 | Filters, Oscillators, Reverb, Delay, Waveshaping |
| **synfx-dsp** | Rust | MIT / Apache-2.0 | Filters, Distortion, Oscillators, Envelopes |
| **Airwindows** | C++ | MIT | 300+ effects: everything |
| **Audio-Effects** | C++ (JUCE) | GPL-3.0 | Flanger, Phaser, Tremolo, Compressor, Delay, Ping-Pong Delay, Vibrato |
| **Calf Studio Gear** | C++ | LGPL-2.1 | Phaser, Flanger, Autopan, Compressor, Gate, Limiter, Reverb |
| **nih-plug** | Rust | ISC | Safety Limiter, Spectral Compressor, plugin framework |
| **Awesome Audio DSP** | (list) | N/A | [github.com/BillyDM/awesome-audio-dsp](https://github.com/BillyDM/awesome-audio-dsp) |

## Implementation Priority

1. **Start with Rust projects**: OctaSine (FM), Actuate (subtractive/sampler), Yazz (wavetable), VitaliumVerb (reverb), lamb-rs (limiter), FunDSP/synfx-dsp (filters, oscillators, envelopes)
2. **Use sndfilter for effects DSP**: 0BSD license = translate C to Rust freely. Covers compressor, reverb, biquad filters.
3. **Use Airwindows for algorithm variety**: 300+ single-file MIT effects. When you need a specific sonic character.
4. **Use Audio-Effects for textbook implementations**: Clearest bridge between DSP theory and working code for modulation effects.
5. **Use DSPFilters for filter math**: Most complete filter coefficient reference, with Rust port (augmented-dsp-filters) available.
