# Vibez Prime Direction

Date: 2026-04-18
Branch: `feature/sample-first-track-factory`

> **Amendment 2026-07-04**: after real dogfood use, genre starter
> templates and the phrase variation engine were REMOVED. Neither was
> reached for while actually making tracks; both failed the decision
> filter below. This supersedes "Variation Is Essential" and the
> genre-template items in this doc. The variation idea may return
> later, but only if real use demands it. Focus is the core loop:
> browse, chop, warp, arrange, resample.

## Mission

Turn Vibez into a `tune-making machine` for fast electronic music production.

The goal is not to out-Ableton Ableton at general-purpose DAW work. The goal is
to beat a normal DAW workflow for `sample-first construction` of:

- house
- techno
- trance
- dnb
- UKG
- big beat
- electro

We already have a very large sample library (`The Megalodon`) with 90s sample
CD material, loops, textures, and source content. The constraint is no longer
"where do sounds come from?" The constraint is `throughput`.

## Working Definition Of Success

Vibez should let one person:

- find usable source material fast
- audition it in time fast
- build phrases fast
- generate phrase variation fast
- resample and commit fast
- arrange a working track shell fast

The target is not polish-first. The target is `finished tracks per day`.

## Product Position

Vibez is now a:

- sample browser
- drum machine / phrase builder
- loop construction workstation
- resample-and-destroy tool
- genre-template arrangement tool

It is not currently trying to be:

- a full DAW parity effort
- a plugin marketplace product
- a full-song AI generator
- a deep automation system first
- a pristine mastering environment

## Core Workflow

The ideal fast path is:

1. Search / audition samples and loops from the library.
2. Pull sounds into a drum rack or phrase lane.
3. Build a short groove or phrase.
4. Ask the system for useful variations.
5. Apply macro FX or destructive transforms.
6. Resample to audio.
7. Drop into a genre arrangement template.
8. Repeat until a track exists.

## First-Class Principles

### 1. Sample-First

The browser is a core instrument, not a utility panel.

Needs:

- fast search
- favorites / crates
- recent sounds
- tempo-aware preview
- genre tags
- preview-in-context

### 2. Phrase-First

Tracks are built from `phrases`, not just static clips.

A phrase can be:

- 1 bar
- 2 bars
- 4 bars
- 8 bars

Phrases should be easy to duplicate, mutate, reprint, and swap.

### 3. Variation Is Essential

Good electronic music is not a perfectly static loop. Even simple grooves need
small mutations per phrase to stay alive.

This must become a dedicated tool in Vibez.

#### Phrase Variation Tool

The system should generate controlled phrase variations from a base pattern or
loop. This is not "AI makes a new song." It is a fast musical mutation engine.

Targets:

- drum phrases
- bass phrases
- top loops
- FX phrases
- chopped audio phrases

Operations should include:

- end-of-phrase fill
- kick dropout every N bars
- alternate hat hits
- ghost snare injection
- shuffled / UKG swing conversion
- DnB double-time top variation
- reverse cymbal / tail accent
- micro-mutes
- snare pickup before transition
- one-bar turnaround
- density up / density down
- energy ramp into next section

Requirements:

- deterministic seed
- generate 3-8 candidates quickly
- preserve the identity of the source phrase
- genre-aware presets
- phrase-length aware behavior

### 4. Commit Early

Printing and resampling should be normal, not special.

The fastest workflows for big beat, dnb, techno, and electro all benefit from:

- resample
- chop
- distort
- filter
- reverse
- repitch
- reprint

### 5. Macro FX Over Deep Menus

For this phase, a few great, musical, high-impact tools beat broad effect
coverage.

Priority FX categories:

- filter
- distortion / drive
- compressor
- delay
- reverb
- gate
- autopan
- bitcrush
- phaser / flanger

## AI Use

AI should be used as `feedstock generation`, not final song generation.

Good AI use:

- one-shots
- textures
- impacts
- risers
- strange percussion
- vocal fragments
- dirty stabs
- atmospheric layers

Weak AI use:

- fully arranged final tracks
- timing-critical final rhythm sections
- generic autonomous song generation

Suno can still be useful if treated as a raw material source to chop, not as a
finished-track engine.

## Immediate Build Order

1. Project save / open
2. Sample browser with fast audition
3. Drum rack / sampler workflow optimized for loop construction
4. Resample / bounce selected material
5. Genre project templates
6. Phrase variation tooling
7. Macro FX chains for destruction and motion

## Near-Term Non-Goals

Do not disappear into:

- full automation lane architecture
- deep AI chat systems
- marketplace work
- large plugin feature work unless it unblocks this workflow
- general-purpose DAW parity tasks

## Decision Filter

Before building a feature, ask:

`Does this help finish more usable electronic tracks per day?`

If the answer is no, it is probably not part of the current phase.


## Amendment 2026-07-06: next phase is architecture

The stabilization phase worked (user is producing tracks), but
app.rs is an 11k-line monolith holding update(), all views, and all
orchestration. Agreed next phase after the current bug tail:
establish a real architecture with the goals, in the user's words:
make the project maintainable, isolate bugs, allow testing.
Sketch to evaluate when we start: split Message by domain
(transport/tracks/clips/piano-roll/devices/plugins/project) with
per-domain update controllers owning their state slice; extract view
families into widgets/ modules (device cards already moved); pull
the async pipelines (plugin loads, warp, project IO) into services
with channel interfaces so they unit-test without iced.
