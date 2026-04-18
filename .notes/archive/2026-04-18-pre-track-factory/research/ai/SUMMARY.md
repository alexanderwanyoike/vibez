# Vibez AI Research: Summary of Takeaways

## The Opportunity

No desktop-native, open-source, AI-integrated DAW exists. Suno Studio is browser/closed, Logic is Mac/closed, Ableton/Bitwig have no AI, Ardour/LMMS have no AI. Vibez sits in the only empty quadrant: **open-source + AI-native**.

## Deployment Model

Users choose where inference runs — same interface regardless:

- **Local**: Auto-detects GPU or falls back to CPU. Free. Private. Offline.
- **RunPod**: User brings their own API key. Cheap (~$0.001/operation).
- **Vibez Cloud**: Our hosted endpoints. Free tier + paid.

Models download on first use to `~/.vibez/models/`. Binary stays small.

## The Three Features

### 1. Sample Generation → Stable Audio Open

**Model**: Stable Audio Open Small (341M params)
- Runs on any CPU in ~7 seconds, stereo 44.1kHz
- Text-conditioned: "punchy 808 kick", "dark techno hi-hat"
- ONNX export proven (Intel OpenVINO, ARM LiteRT)
- Full model (1.2B) for GPU users, ~3-5s
- License: Free under $1M revenue
- Generate N variations → preview → drag to arrangement

### 2. MIDI Generation → Tiered Approach

**Ship first**: Rule-based in pure Rust (zero deps)
- Euclidean drum patterns (Bjorklund algorithm)
- Genre templates (house, techno, trap, breakbeat)
- Scale-constrained Markov melodies
- Arpeggiator presets

**Then add ML**: MIDI-RWKV (20M params, MIT license)
- Sub-second on CPU, 39x more efficient than comparable transformers
- Supports **infilling** — the killer feature ("fill these 4 bars given context")
- Attribute controls (density, complexity, style, swing) beat text prompts for DAW UX

**Optional upgrade**: SkyTNT midi-model (200M, Apache 2.0, pre-exported ONNX)

**Cloud tier**: MIDI-LLM (1.47B, Llama-based) for natural language prompts

### 3. Stem Separation → Demucs ONNX

**Rust crate already exists**: `stem-splitter-core` — Demucs via ort, no Python
- htdemucs: 81MB model, 4 stems, MIT licensed
- CPU: ~4 min for 3 min song. GPU: ~10 seconds.
- ONNX export fully solved (Mixxx GSOC 2025 — STFT baked into model)

**Quality upgrade path**: BS-RoFormer (+2.5 dB SDR over Demucs, ONNX available)

## Shared Tech Stack

All three features share:
- **`ort` crate** — Rust ONNX Runtime bindings (CPU/CUDA/CoreML/DirectML)
- **`ndarray`** — Tensor manipulation
- **`midly`** — MIDI I/O (for MIDI gen)
- **rtrb channels** — Progress updates + results to UI (already in Vibez)
- **Background thread pool** — Never block UI or audio thread

New crate: `vibez-ai` housing all three features behind a `GenerationBackend` trait.

## Implementation Order

1. **vibez-ai crate** + model manager + backend trait
2. **Rule-based MIDI** (pure Rust, immediate value, no models needed)
3. **Stem separation** (existing Rust crate to study, clearest path)
4. **Sample generation** (ONNX pipeline: T5 → DiT loop → VAE)
5. **ML MIDI** (MIDI-RWKV, REMI tokenizer in Rust)
6. **Cloud backend** (RunPod serverless endpoints)

## Key Numbers

| | Model Size | CPU Time | GPU Time | License |
|---|---|---|---|---|
| SA Open Small | 341M / 2.9GB disk | ~7s | 75ms (H100) | Community (<$1M) |
| MIDI-RWKV | 20M | <1s | — | MIT |
| htdemucs | 81MB | ~4 min (3min song) | ~10s | MIT |
| BS-RoFormer | ~200MB | — | ~10s | MIT |

## What Makes This Different

Vibez would be the first DAW where you can:
- Generate a drum sample from a text prompt, locally, on CPU
- Separate any imported track into stems without leaving the app
- AI-fill 4 bars of MIDI that fit the surrounding musical context
- Do all of it offline with no cloud dependency
- Or route to GPU cloud when you want speed
- With every model being open-source and replaceable
