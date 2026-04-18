# Vibez AI Strategy: Unified Plan

## Deployment Model

Three tiers, user's choice — same interface regardless of backend:

```
┌─────────────────────────────────────────────────┐
│              GenerationBackend trait             │
├─────────────┬─────────────────┬─────────────────┤
│ Local       │ RunPod          │ Vibez Cloud     │
│ (ort/ONNX)  │ (serverless)    │ (hosted API)    │
│ Free        │ Pay-per-use     │ Subscription    │
└─────────────┴─────────────────┴─────────────────┘
```

User configures once in Settings → AI Backend:
- **Local**: Auto-detects GPU (CUDA/CoreML/DirectML) or falls back to CPU
- **RunPod**: User provides API key, we provide the Docker images
- **Vibez Cloud**: Our hosted endpoints, account-based

Models download on first use → `~/.vibez/models/` with SHA-256 verification.

---

## The Three AI Features

### 1. Sample Generation (Stable Audio Open)

| Tier | Model | Hardware | Time | Notes |
|---|---|---|---|---|
| Lite Local | SA Open Small (341M, Int8) | CPU, 4GB RAM | ~7s | Ship as default |
| Full Local | SA Open 1.0 (1.2B, FP16) | 8GB+ VRAM | ~3-5s | Optional download |
| Cloud | SA Open 1.0 | RunPod A10G | ~2s | Cents per sample |

**ONNX decomposition**: T5 encoder + DiT (per-step) + VAE decoder. Denoising loop in Rust.

**License**: Stability AI Community — free under $1M revenue.

### 2. MIDI Generation (Tiered)

| Tier | What | Model | Notes |
|---|---|---|---|
| 0 (ship first) | Rule-based | Pure Rust | Euclidean drums, Markov melodies, arps |
| 1 (bundled) | Small ML | MIDI-RWKV (20M) | CPU sub-second, infilling, MIT |
| 2 (download) | Medium ML | SkyTNT (200M, ONNX) | Pre-exported, Apache 2.0 |
| 3 (cloud) | Large ML | MIDI-LLM (1.47B) | Text prompts, Llama based |

**Key insight**: Attribute controls (density/complexity/style sliders) > text prompts for DAW UX. Text prompts are Tier 3 additive.

**Infilling is the killer feature**: "fill these 4 bars given context" is what producers actually need. MIDI-RWKV supports this natively.

### 3. Stem Separation (Demucs → BS-RoFormer)

| Tier | Model | Hardware | Time (3min song) | Notes |
|---|---|---|---|---|
| Local CPU | htdemucs ONNX (81MB) | Any CPU | ~4 min | Ship as default |
| Local GPU | htdemucs ONNX | 4GB+ VRAM | ~10s | CUDA/CoreML/DirectML |
| Cloud | BS-RoFormer | RunPod GPU | ~10s + transfer | Best quality |

**Rust crate exists**: `stem-splitter-core` — Demucs via ort, no Python.

**Upgrade path**: Start htdemucs → add BS-RoFormer ONNX (better SDR, especially bass at 11.43 dB).

---

## Shared Infrastructure: vibez-ai crate

```
vibez-ai/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── backend.rs          # GenerationBackend trait + local/cloud impls
│   ├── model_manager.rs    # Download, cache, verify models
│   ├── sample_gen.rs       # Stable Audio Open inference
│   ├── midi_gen/
│   │   ├── mod.rs
│   │   ├── rule_based.rs   # Tier 0: Euclidean, Markov, arps
│   │   ├── ml.rs           # Tier 1-2: ONNX inference
│   │   └── tokenizer.rs    # REMI tokenizer in Rust
│   ├── stem_split.rs       # Demucs / BS-RoFormer
│   └── scheduler.rs        # DPM++ / DDPM diffusion scheduler (pure Rust)
```

### Key dependencies
```toml
[dependencies]
ort = { version = "2", features = ["download-binaries"] }
ndarray = "0.16"
midly = "0.5"
serde = { version = "1", features = ["derive"] }
reqwest = { version = "0.12", features = ["json"], optional = true }  # cloud backend

[features]
default = ["local"]
local = []
cuda = ["ort/cuda"]
coreml = ["ort/coreml"]
directml = ["ort/directml"]
cloud = ["reqwest"]
```

### Thread model
```
UI Thread
  │
  ├── Message::GenerateSample { prompt, config }
  ├── Message::GenerateMidi { role, style, config }
  ├── Message::SeparateStems { track_id, clip_id, config }
  │
  ▼
AI Worker Thread (std::thread::spawn or thread pool)
  │
  ├── Progress updates → UI via rtrb
  ├── Results → UI via rtrb
  │
  ▼
UI Thread receives results
  ├── Sample: creates new audio clip, audition/drag
  ├── MIDI: creates new MIDI clip on track
  └── Stems: creates N new tracks with audio
```

---

## Implementation Order

### Phase A: Foundation
1. Create `vibez-ai` crate with `GenerationBackend` trait
2. Model manager (download, cache, verify)
3. Rule-based MIDI generation (Tier 0) — pure Rust, immediate value
4. Background thread infrastructure + progress reporting

### Phase B: Stem Separation
5. htdemucs ONNX integration via ort (study `stem-splitter-core`)
6. Chunking + overlap-add in Rust
7. UI: right-click clip → "Separate Stems" → progress → new tracks

### Phase C: Sample Generation
8. Stable Audio Open Small ONNX export
9. DPM++ scheduler in Rust
10. Three-model inference pipeline (T5 → DiT loop → VAE)
11. UI: sample browser panel with prompt → generate → audition → drag

### Phase D: ML MIDI
12. MIDI-RWKV integration (rwkv.cpp FFI or ONNX)
13. REMI tokenizer in Rust
14. Infilling mode (fill selected bars given context)
15. UI: attribute sliders + generate variations

### Phase E: Cloud Backend
16. RunPod serverless endpoint (Docker image with all models)
17. Cloud backend implementation (HTTP/WebSocket)
18. Settings UI for backend configuration

---

## Open Questions

1. **Stable Audio Open license**: The <$1M revenue threshold — is this per-org or per-product? Need to read the full license. Fallback: train our own model on CC data using `stable-audio-tools`.

2. **MIDI-RWKV maturity**: It's new (2025). Need to evaluate actual output quality vs. claims. Fallback: SkyTNT midi-model has proven ONNX and is Apache 2.0.

3. **Model bundling vs. download**: Ship Tier 0 (rule-based) bundled. Everything else downloads on first use. Keeps binary small.

4. **Demucs vs. BS-RoFormer timing**: htdemucs has the easiest Rust path today. BS-RoFormer is better quality. Could ship htdemucs first and add BS-RoFormer once its ONNX story matures further.

5. **Cloud pricing**: What do we charge? RunPod costs are tiny (~$0.001/operation). Could offer a free tier (N generations/month) + paid unlimited.

6. **Fine-tuning story**: ACE-Step has LoRA from a few songs. Could we offer "train on your sample pack" as a premium feature? Needs a cloud training endpoint.

---

## Competitive Position

This combination — open models, local-first, three deployment tiers — doesn't exist anywhere:

- **Suno Studio**: Cloud-only, closed models, browser-based
- **Logic Pro**: Apple's models, no user control, Mac-only
- **FL Studio**: Chat assistant only, no generation
- **Ableton**: No AI at all
- **Bitwig**: No AI (WigAI is third-party MCP)
- **Ardour/LMMS**: No AI, no plans

Vibez would be the first open-source DAW where you can:
- Generate a drum loop from a text prompt, locally, on CPU
- Separate any track into stems without leaving the app
- Fill in 4 bars of MIDI that fit the surrounding context
- Do all of this offline, or route to cloud for speed
