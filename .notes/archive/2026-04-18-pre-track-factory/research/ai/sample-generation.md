# Sample Generation: Technical Research

## TL;DR

- **Best fit**: Stable Audio Open — designed for samples/sound design, stereo 44.1kHz, text-conditioned
- **Small variant (341M)**: Runs on CPU in ~7s, 2.9GB on disk — the "lite local mode"
- **Full variant (1.2B)**: Needs 8GB VRAM, ~3-5s on RTX 3090
- **License caveat**: Free under $1M revenue, commercial license needed above that
- **ONNX export**: Very doable — decompose into 3 models, loop denoising in Rust

---

## Model Comparison

| Model | Params | Output | Text Cond | License | Best For |
|---|---|---|---|---|---|
| **Stable Audio Open** | 1.2B | 44.1kHz stereo, 47s | Yes | Community (<$1M free) | Drums, riffs, ambience, production elements |
| **SA Open Small** | 341M | 44.1kHz stereo, 11s | Yes | Community (<$1M free) | Same, lighter |
| **AudioLDM 2** | ~350M-750M | 16kHz default (48kHz avail) | Yes | Code MIT, weights need permission | General SFX, speech |
| **MusicGen Small** | 300M | 32kHz mono | Yes | **CC-BY-NC (non-commercial)** | Melodic loops, structure |
| **CRASH** | Small | 44.1kHz | No (class-based) | Unclear | Drum hits, interpolation |

### Winner: Stable Audio Open
- Designed for audio samples, not full songs
- Stereo 44.1kHz (exactly what a DAW needs)
- "Punchy 808 kick", "crispy hi-hat" style prompts work
- Fine-tunable via `stable-audio-tools`
- Trained on 486K CC-licensed recordings (Freesound + Free Music Archive)
- **Weak at**: Vocals, full songs, precise pitch control

---

## Stable Audio Open Details

### Architecture
- 156M autoencoder + 109M T5 text encoder + 1,057M DiT (Diffusion Transformer)
- Small: 341M total, max 11 seconds
- Full: 1.21B total, max 47 seconds

### Hardware Requirements
| Config | VRAM/RAM | Inference |
|---|---|---|
| Full (FP16, GPU) | 8GB+ VRAM (14.5GB peak during VAE) | 8 steps/s (3090), 20 steps/s (H100) |
| Small (Int8, CPU) | 4GB RAM | ~7s for 10s audio |
| Small (GPU) | Minimal | 75ms on H100 |

### License: Stability AI Community License
- Free for individuals and orgs under $1M annual revenue
- Above $1M: commercial license from Stability AI required
- Must register for commercial use

---

## Loop Generation: No Native BPM-Lock

No open model generates BPM-locked loops natively. Practical approach:

1. Text prompt with tempo hint: "128 bpm techno drum loop"
2. Beat-track output to detect actual BPM
3. Time-stretch to exact target BPM (rubato — already have)
4. Trim to exact bar boundaries
5. Crossfade at loop points

For 1-4 bars at typical tempos, SA Open Small's 11s limit is sufficient.

---

## ONNX Export

Diffusion models decompose into 3 independent ONNX models:

| Component | Difficulty | Notes |
|---|---|---|
| T5 Text Encoder | Easy | Standard transformer, static shapes |
| DiT (single denoise step) | Medium | Export one step; loop in Rust |
| VAE Decoder | Easy | Single forward pass |

The iterative denoising loop (20-50 steps) runs in Rust, calling DiT ONNX each step. The scheduler (DPM++, DDPM) is pure math — reimplement in Rust.

### Proven precedents
- SA Open → OpenVINO: Intel published working conversion
- SA Open Small → LiteRT: ARM deployed on mobile
- MusicGen Small → ONNX: Xenova on HuggingFace
- `ort` crate supports CPU/CUDA/CoreML/DirectML

---

## Deployment Tiers

| Tier | Model | Hardware | Time | Cost |
|---|---|---|---|---|
| **Lite Local** | SA Open Small (341M, Int8) | Any CPU, 4GB RAM | ~7s | Free |
| **Standard Local** | SA Open 1.0 (1.2B, FP16) | 8GB+ VRAM GPU | ~3-5s | Free |
| **Cloud (RunPod)** | SA Open 1.0 on A10G | Serverless | ~2-3s | ~$0.0006/sample |
| **Cloud Premium** | SA Open 1.0 on A100 | Serverless | ~1s | ~$0.002/sample |

1000 samples/month on RunPod ≈ $0.60. Batch of 10 variations < $0.01.

---

## UX Flow

```
User clicks "Generate Sample" (or dedicated browser panel)
  → Prompt: "dark techno kick"
  → Mode: [One-shot / Loop]
  → Duration: [auto / 0.5s / 1s / 2s / 4 bars]
  → Count: [8 variations]
  → Backend: [Local / Cloud]
  → [Generate]

Progress: "Generating 3/8..."

→ 8 waveform previews appear
→ Click to audition
→ Drag to arrangement or save to project samples
```

### Batch generation
- Generate N variations from same prompt
- Different random seeds
- Preview all, pick favorites
- Fast iteration cycle

---

## Integration Architecture

```rust
// GenerationBackend trait — local and cloud implement the same interface
trait GenerationBackend {
    fn generate(&self, request: GenerationRequest) -> Result<Vec<GeneratedSample>>;
}

struct GenerationRequest {
    prompt: String,
    mode: SampleMode,        // OneShot | Loop { bpm: f32 }
    duration_secs: f32,
    num_variations: u8,
    seed: Option<u64>,
}

struct GeneratedSample {
    audio: Arc<DecodedAudio>,  // reuse existing type
    prompt: String,
    seed: u64,
}
```

### ONNX local backend
```rust
struct OnnxLocalBackend {
    text_encoder: ort::Session,   // T5
    dit: ort::Session,            // DiT (one denoise step)
    vae_decoder: ort::Session,    // VAE
    scheduler: DpmppScheduler,    // Pure Rust
}
```

### Cloud backend
```rust
struct CloudBackend {
    endpoint: Url,       // RunPod serverless or Vibez API
    api_key: String,
}
```

---

## Other Models Worth Watching

### MusicGen (Meta)
- Better for melodic content and musical structure
- **CC-BY-NC weights — cannot use commercially**
- Code is MIT
- Could use if user brings their own weights or for non-commercial tier

### CRASH (Drum Synthesis)
- Diffusion model specifically for drum sounds
- Interesting interpolation: blend kick + snare = hybrid
- Only 0.48s output, no text conditioning, unmaintained since 2021

### AudioLDM 2
- More complex architecture (4 sub-networks)
- 16kHz default (need 48kHz variant)
- Better for general SFX than music production
- Weights need commercial permission

---

## Key References
- [Stable Audio Open (HuggingFace)](https://huggingface.co/stabilityai/stable-audio-open-1.0)
- [SA Open Small (HuggingFace)](https://huggingface.co/stabilityai/stable-audio-open-small)
- [stable-audio-tools (GitHub)](https://github.com/Stability-AI/stable-audio-tools)
- [AudioLDM 2 (GitHub)](https://github.com/haoheliu/AudioLDM2)
- [MusicGen (GitHub)](https://github.com/facebookresearch/audiocraft)
- [CRASH drum synthesis](https://github.com/crash-diffusion)
- [ort crate](https://github.com/pykeio/ort)
- [Intel OpenVINO SA conversion](https://docs.openvino.ai/latest/notebooks/stable-audio-open.html)
