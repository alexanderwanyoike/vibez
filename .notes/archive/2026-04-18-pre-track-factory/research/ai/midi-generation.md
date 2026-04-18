# MIDI Generation: Technical Research

## TL;DR

- **Ship first (zero deps)**: Rule-based Euclidean drums + Markov melodies in pure Rust
- **Best small model**: MIDI-RWKV (20M params, MIT, CPU sub-second, infilling)
- **Easiest ONNX path**: SkyTNT midi-model (200M, Apache 2.0, pre-exported ONNX)
- **Best DAW-proven**: MIDI-GPT (already in Cubase/Ableton, attribute controls)
- **For text prompts**: MIDI-LLM (1.47B, Llama 3.2 based — cloud tier)

---

## The Tiered Strategy

### Tier 0: Rule-Based (ship first, pure Rust, zero dependencies)

| Feature | How |
|---|---|
| Euclidean drum patterns | Bjorklund algorithm — trivial in Rust |
| Genre templates | House four-on-the-floor, trap hi-hats, breakbeat, etc. |
| Scale-constrained melodies | Markov chains seeded by key/scale selection |
| Arpeggiator presets | Standard up/down/random/pattern modes |
| Chord progressions | Rule-based from music theory (I-V-vi-IV etc.) |

This gives immediate value with zero ML overhead. Users get usable drum patterns and melodic ideas while the ML pipeline matures.

### Tier 1: Small ML Model (bundled, CPU, sub-second)

**MIDI-RWKV** — The standout
- **20M parameters** — matches quality of 780M-param Anticipatory Music Transformer (39x efficiency)
- RWKV-7 linear architecture: O(n) complexity vs O(n²) for transformers
- **MIT licensed**
- Designed for edge devices
- Supports **infilling** — the most useful DAW operation ("fill in these 4 bars given surrounding context")
- Inference via rwkv.cpp (C FFI from Rust) or GGML format
- Sub-second generation on CPU

### Tier 2: Medium ML Model (optional download, 1-5 seconds)

**SkyTNT midi-model**
- 200M params, **Apache 2.0**
- **Pre-exported ONNX** — easiest integration via `ort` crate
- Multi-track generation
- [GitHub](https://github.com/SkyTNT/midi-model)

**MIDI-GPT** (Metacreation Lab)
- Est. 50-100M params, Open RAIL-M license
- **Already integrated into Cubase and Ableton** — most DAW-validated
- Excellent attribute controls: density, polyphony, instrument, style
- The controllability gold standard

### Tier 3: Large / Cloud (optional)

**MIDI-LLM**
- 1.47B params (Llama 3.2 based)
- Natural language: "generate a funky bass line in E minor"
- Needs quantization for CPU or cloud endpoint
- Text prompts are great for exploration but need big models

---

## MIDI Tokenization

Models need to convert MIDI events into tokens. Key approaches:

| Tokenization | Description | Used By |
|---|---|---|
| **REMI** | Tempo-relative: Bar, Position, Pitch, Duration, Velocity | Most common, MidiTok default |
| **Structured** | Track-aware REMI with instrument tokens | Multi-track models |
| **Octuple/Compound Word** | All attributes in one compound token | Efficient, fewer tokens |
| **ABC Notation** | Text-based music notation | ChatMusician (LLaMA) |
| **Piano Roll Image** | 2D grid as image | MuseGAN, niche |

**MidiTok** is the standard Python library. No Rust equivalent exists — need to implement REMI tokenizer in Rust (straightforward: it's a state machine over MIDI events).

---

## Controllability

### Attribute-based (practical for DAW — ship this)
- **Density slider**: How many notes per bar
- **Polyphony**: Monophonic → full chords
- **Instrument/role**: Drums, bass, lead, pad, arp
- **Style/genre**: House, techno, trap, jazz, etc.
- **Key/scale**: Lock to project key
- **Complexity**: Simple → intricate

### Text-based (exploration — Tier 3)
- "Funky bass line in E minor, syncopated"
- "Minimal techno hi-hat pattern with occasional open hats"
- Needs large models (1B+) to understand natural language

### Infilling (most useful DAW operation)
- User has context (surrounding bars, other tracks)
- AI fills in the gap
- MIDI-RWKV specifically supports this
- Much more useful than generation-from-nothing

---

## Rust Integration

### Dependencies
```toml
midly = "0.5"          # MIDI file I/O
ort = { version = "2" } # ONNX inference (Tier 2)
# Or for Tier 1 (MIDI-RWKV):
# rwkv.cpp via FFI, or tract for pure Rust
```

### Processing pipeline
1. User selects: instrument, bars, key/scale, density, style
2. Encode context (surrounding MIDI) into tokens via Rust REMI tokenizer
3. Run model inference (background thread)
4. Decode output tokens back to MIDI events
5. Place as MIDI clip on track
6. User can regenerate (new seed) or refine (adjust attributes)

### Tokenizer in Rust
Need to implement REMI tokenizer — it's a state machine:
- Quantize time to bar positions (e.g., 16th note grid)
- Map pitch/velocity/duration to token IDs
- Vocabulary: ~500-2000 tokens depending on resolution
- Straightforward to implement, no external deps

---

## Training Datasets

| Dataset | Size | License | Best For |
|---|---|---|---|
| **GigaMIDI** | 2.1M+ files | CC BY 4.0 | Everything — definitive dataset |
| **Groove MIDI** (Google) | ~1K hours drums | CC BY 4.0 | Drum humanization, groove |
| **Lakh MIDI** | 176K files | Research | Standard benchmark |
| **ADL Piano MIDI** | 11K piano pieces | Research | Piano/classical |
| **MetaMIDI** | 436K files | CC BY 4.0 | Genre-diverse |

---

## Architecture Decision: Attribute Controls > Text Prompts

For a DAW, **attribute-based controls are more practical than text prompts**:

1. **Faster iteration**: Drag a density slider vs. retype a prompt
2. **Smaller models**: Attribute conditioning needs 20-200M params vs. 1B+ for NLP
3. **Deterministic control**: Slider at 0.7 always means 0.7 density
4. **DAW-native UX**: Sliders/dropdowns fit the mixer/device chain paradigm
5. **Reproducible**: Same attributes + seed = same output

Text prompts are an additive feature for exploration (Tier 3), not the core interaction.

---

## UX Flow

```
Right-click MIDI track → "Generate MIDI"
  or: Select empty bars → "AI Fill"

┌─────────────────────────────────┐
│ AI MIDI Generator               │
│                                 │
│ Role: [Drums ▼]                │
│ Style: [Techno ▼]             │
│ Bars: [4]  Key: [project key]  │
│                                 │
│ Density:    ━━━━━━━●━━━ 0.7    │
│ Complexity: ━━━━●━━━━━━ 0.5    │
│ Swing:      ━━━━━━●━━━━ 0.6    │
│ Humanize:   ━━━●━━━━━━━ 0.4    │
│                                 │
│ [Generate 4 variations]         │
│                                 │
│ ♩♩♩♩ Var 1 [▶] [Use]          │
│ ♩♩♩♩ Var 2 [▶] [Use]          │
│ ♩♩♩♩ Var 3 [▶] [Use]          │
│ ♩♩♩♩ Var 4 [▶] [Use]          │
└─────────────────────────────────┘
```

### Infilling mode
- User selects empty bars on a track with context before/after
- AI generates MIDI that fits the surrounding material
- Musically aware: respects key, rhythm, density of context

---

## Key References
- [MIDI-RWKV (GitHub)](https://github.com/MIDI-RWKV) — 20M params, MIT, infilling
- [SkyTNT midi-model (GitHub)](https://github.com/SkyTNT/midi-model) — ONNX ready, Apache 2.0
- [MIDI-GPT (Metacreation)](https://metacreation.net/midi-gpt/) — DAW-integrated
- [MIDI-LLM](https://github.com/MIDI-LLM) — Llama 3.2 based, text prompts
- [MidiTok (GitHub)](https://github.com/Natooz/MidiTok) — Python tokenizer reference
- [midly (crates.io)](https://crates.io/crates/midly) — Rust MIDI I/O
- [GigaMIDI dataset](https://github.com/GigaMIDI) — 2.1M files, CC BY 4.0
- [Groove MIDI (Magenta)](https://magenta.tensorflow.org/datasets/groove) — Drum dataset
- [Anticipatory Music Transformer](https://arxiv.org/abs/2306.08620) — Infilling reference
- [rwkv.cpp](https://github.com/RWKV/rwkv.cpp) — C inference for RWKV models
