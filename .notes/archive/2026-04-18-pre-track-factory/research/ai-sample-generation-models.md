# AI Sample Generation Models Research

Last updated: 2026-03-02

Research into open-source models for generating short audio samples (one-shots and loops)
for integration into the Vibez DAW.

---

## Executive Summary

The best path forward for Vibez is a **tiered architecture**:

| Tier | Model | Use Case | Hardware | Latency |
|------|-------|----------|----------|---------|
| **Lite (Local CPU)** | Stable Audio Open Small (341M) | One-shots, short SFX | Any CPU (ARM/x86), 4GB RAM | ~7s for 10s audio |
| **Standard (Local GPU)** | Stable Audio Open 1.0 (1.2B) | One-shots + short loops | 8GB+ VRAM GPU | ~3-5s for 10s audio |
| **Full (Cloud/RunPod)** | Stable Audio Open 1.0 + fine-tuned | Longer loops, higher quality | A100/H100 | ~1-2s |
| **Loop specialist** | MusicGen Small (300M) | Musical loops with melody | 4-8GB VRAM | ~10-15s for 8 bars |

**Primary recommendation: Stable Audio Open (both sizes)** for one-shots and SFX,
with **MusicGen Small** as a secondary engine for melodic loops.

---

## 1. Model Inventory

### 1.1 Stable Audio Open 1.0

- **Source**: [Stability AI / HuggingFace](https://huggingface.co/stabilityai/stable-audio-open-1.0)
- **Code**: [github.com/Stability-AI/stable-audio-tools](https://github.com/Stability-AI/stable-audio-tools)
- **Paper**: [arXiv 2407.14358](https://arxiv.org/abs/2407.14358)
- **Parameters**: 1.21B total
  - Autoencoder (VAE): 156M
  - T5 text encoder: 109M
  - Diffusion Transformer (DiT): 1,057M
- **Architecture**: Latent diffusion with DiT backbone, T5-based text conditioning via cross-attention, rotary positional embeddings
- **Output**: Up to 47 seconds, stereo, 44.1kHz
- **Training data**: 486K recordings from Freesound (472K) + Free Music Archive (13K), all CC-licensed
- **License**: **Stability AI Community License** -- free for individuals/orgs under $1M annual revenue. Commercial use above $1M requires separate license from Stability AI.
- **Conditioning**: Text prompts ("punchy 808 kick", "dark ambient pad")
- **Fine-tuning**: Supported via stable-audio-tools library. Can fine-tune on custom drum/sample datasets.
- **VRAM**: ~6GB during diffusion, ~14.5GB peak during decoding. Practical minimum: 8GB GPU (RTX 3070+)
- **Inference speed**:
  - RTX 3090 (24GB): 8 steps/sec
  - RTX A6000 (48GB): 11 steps/sec
  - H100 (80GB): 20 steps/sec
  - Typical generation: ~3-5 seconds for short samples on consumer GPU

**Strengths for Vibez**:
- Specifically designed for audio samples and sound design (not full songs)
- 44.1kHz stereo output matches DAW requirements
- Strong text conditioning for drum hits, synth stabs, ambient textures
- Fine-tunable on custom datasets
- Already integrated into OpenVINO for optimized inference

**Weaknesses**:
- Not great at full musical pieces or vocals
- License requires registration for commercial use above $1M
- 8GB VRAM minimum puts it out of reach for CPU-only users
- No inherent BPM/key awareness for loops


### 1.2 Stable Audio Open Small

- **Source**: [stabilityai/stable-audio-open-small on HuggingFace](https://huggingface.co/stabilityai/stable-audio-open-small)
- **Parameters**: 341M total (same 3-component architecture as 1.0)
- **Output**: Up to 11 seconds, stereo, 44.1kHz
- **License**: Same Stability AI Community License (free under $1M revenue)
- **CPU inference**: Yes! Optimized for ARM CPUs (via KleidiAI). Runs on smartphones.
- **Performance**:
  - Smartphone (ARM CPU): ~7s for 10s audio
  - With quantization (Int8 DiT + FP16 VAE): 6.6s inference, 2.9GB model size, 3.6GB peak RAM
  - H100 GPU: 75ms (!)
- **Deployment options**: LiteRT (TensorFlow Lite), PyTorch, potentially ONNX

**This is the killer model for "lite" local mode.** 341M params, runs on CPU, generates
quality one-shots in seconds. 11 seconds is plenty for one-shots and short loops (2-4 bars at 120bpm).

**Strengths for Vibez**:
- Runs on ANY hardware including low-end laptops and phones
- 11 seconds covers all one-shot use cases
- Same quality architecture as the full model, just smaller
- Already has ARM CPU optimizations
- Model size: 2.9GB quantized

**Weaknesses**:
- 11 second max limits loop length (only 2-4 bars at moderate tempos)
- Slightly lower quality than full model
- Same license constraints


### 1.3 AudioLDM 2

- **Source**: [github.com/haoheliu/AudioLDM2](https://github.com/haoheliu/AudioLDM2)
- **HuggingFace**: [cvssp/audioldm2](https://huggingface.co/cvssp/audioldm2)
- **Paper**: [arXiv 2308.05734](https://arxiv.org/abs/2308.05734)
- **Parameters**:
  - Base: ~350M UNet + shared text encoders (CLAP + Flan-T5-Large) + VAE
  - Large: ~750M UNet + same encoders
- **Architecture**: Latent diffusion with UNet backbone, dual text conditioning (CLAP audio-text embeddings + Flan-T5), GPT-2 "language of audio" bridge
- **Output**: 16kHz (base) or 48kHz (audioldm_48k variant)
- **Training data**: AudioCaps dataset
- **License**: **MIT** (code). Model weights: research-only, contact for commercial.
- **Conditioning**: Text prompts for sound effects, speech, and music
- **Variants**: audioldm2, audioldm2-large, audioldm2-music
- **OpenVINO support**: Yes, documented notebook exists for conversion
- **Inference**: ~30s unoptimized for 10s audio; ~4s with torch compile + optimizations

**Strengths for Vibez**:
- Good at sound effects and environmental audio
- 48kHz variant available
- OpenVINO conversion documented
- MIT licensed code

**Weaknesses**:
- Commercial use of model weights requires negotiation
- Less focused on music production samples than Stable Audio Open
- More complex architecture (CLAP + T5 + GPT-2 + UNet)
- Lower quality for drum/percussion compared to Stable Audio Open
- 16kHz default output is too low for production use


### 1.4 MusicGen (Meta AudioCraft)

- **Source**: [github.com/facebookresearch/audiocraft](https://github.com/facebookresearch/audiocraft)
- **HuggingFace**: [facebook/musicgen-small](https://huggingface.co/facebook/musicgen-small)
- **Parameters**:
  - Small: 300M
  - Medium: 1.5B
  - Large: 3.3B
- **Architecture**: Autoregressive transformer over EnCodec tokens, T5 text conditioning
- **Output**: 32kHz mono (base) or stereo; generates up to ~30s
- **Training data**: 20K hours of licensed music (Meta-owned/licensed)
- **License**: Code is **MIT**. Model weights are **CC-BY-NC 4.0** (non-commercial only!)
- **Conditioning**: Text ("upbeat techno drum loop 128bpm") + optional melody conditioning
- **Variants**: small, medium, large, melody, stereo
- **ONNX**: Xenova/musicgen-small exists on HuggingFace as ONNX for transformers.js. TinyMusician (distilled, 1.04GB quantized) runs on mobile via ONNX Runtime.
- **Inference**: 50 auto-regressive steps per second of audio. Small model: ~10-15s for 5s audio on consumer GPU.

**Strengths for Vibez**:
- Best model for musical loops with melodic/harmonic content
- Melody conditioning lets users hum/play a reference
- ONNX export proven (Xenova, TinyMusician)
- Good genre/style control via text
- Small model (300M) fits in modest GPU memory

**Weaknesses**:
- **CC-BY-NC license on weights kills commercial use** (dealbreaker without relicensing)
- 32kHz output needs upsampling for 44.1/48kHz DAW use
- Autoregressive = slower than diffusion for short clips
- Not designed for isolated one-shots (generates music, not samples)
- No inherent BPM precision (generates "approximately" requested tempo)

**Note on commercial alternatives**: Rightsify released "Hydra II", a MusicGen model
trained entirely on their own licensed music, which may have different license terms.


### 1.5 CRASH (Drum Synthesis)

- **Source**: [github.com/simonrouard/CRASH](https://github.com/simonrouard/CRASH)
- **Paper**: [arXiv 2106.07431](https://arxiv.org/abs/2106.07431) (ISMIR 2021)
- **Architecture**: Score-based diffusion with conditional U-Net, operates on raw audio waveforms
- **Output**: 44.1kHz, fixed length (21,000 samples = ~0.48 seconds)
- **Conditioning**: Class-conditional (kick, snare, cymbal classes), NOT text-conditioned
- **Capabilities**: Unconditional generation, variations, interpolation, inpainting, class-conditional, class-mixing
- **License**: Not explicitly stated on repo (needs verification)

**Strengths for Vibez**:
- Purpose-built for drum one-shots at 44.1kHz
- Very fast inference (small U-Net)
- Supports interpolation between sounds (blend kick + snare)
- Inpainting (modify part of a drum hit)
- Class-mixing for "hybrid" sounds

**Weaknesses**:
- Fixed 0.48s length (fine for kicks/snares, too short for some cymbals)
- No text conditioning -- only class labels
- Research code, not production-ready
- No active maintenance since 2021
- License unclear
- Small training set compared to modern models


### 1.6 DrumGAN (Sony CSL Paris)

- **Source**: [github.com/SonyCSLParis/DrumGAN](https://github.com/SonyCSLParis/DrumGAN)
- **Architecture**: GAN with perceptual timbral conditioning
- **Output**: Kick, snare, cymbal sounds
- **Conditioning**: Perceptual features (brightness, depth, etc.) -- NOT text
- **License**: Research prototype (license needs verification)
- **Commercial use**: Integrated into Steinberg Backbone (commercial product)

**Strengths**: Perceptual parameter control (brightness, warmth, etc.) maps well to DAW UI.
**Weaknesses**: Not text-conditioned. Research-quality code. No open license confirmed.


### 1.7 Riffusion

- **Source**: [github.com/riffusion/riffusion-hobby](https://github.com/riffusion/riffusion-hobby)
- **Architecture**: Fine-tuned Stable Diffusion generating spectrograms, inverse FFT to audio
- **License**: Code MIT, model weights CreativeML OpenRAIL M
- **Output**: Short clips via spectrogram generation

**Not recommended** for Vibez. The spectrogram-to-audio approach produces artifacts and
lower quality than native audio diffusion models. Historical interest only.


### 1.8 VampNet

- **Source**: [github.com/hugofloresgarcia/vampnet](https://github.com/hugofloresgarcia/vampnet)
- **Architecture**: Bidirectional masked transformer over codec tokens (1280 dim, 20 heads)
- **Output**: 44.1kHz via codec decoder
- **License**: Weights are **CC BY-NC-SA 4.0** (non-commercial, share-alike)
- **Capabilities**: Inpainting, outpainting, looping with variation, continuation

**Interesting for loop variation** (take a loop, generate variations) but non-commercial license is a blocker.


### 1.9 audio-diffusion (teticio)

- **Source**: [github.com/teticio/audio-diffusion](https://github.com/teticio/audio-diffusion)
- **Architecture**: DDPM on mel spectrograms (256x256 = 5s audio)
- **Output**: 5 seconds, various styles
- **Conditioning**: Unconditional (or can be conditioned)

Useful as a learning resource but not production-quality. Models are small and limited.


### 1.10 TinyMusician (Distilled MusicGen)

- **Source**: [arXiv 2509.00914](https://arxiv.org/abs/2509.00914)
- **Architecture**: Knowledge-distilled MusicGen-Small
- **Size**: 1.04GB quantized (55% smaller than MusicGen-Small)
- **ONNX**: Successfully exported and deployed on iOS via ONNX Runtime
- **Performance**: 93% of teacher quality, runs on-device

Proves that MusicGen-class models CAN be ONNX-exported and run on-device.
But inherits MusicGen's CC-BY-NC license problem.

---

## 2. License Comparison

| Model | Code License | Weight License | Commercial OK? |
|-------|-------------|---------------|----------------|
| Stable Audio Open 1.0 | MIT | Stability Community | Yes, if <$1M revenue. Otherwise need license. |
| Stable Audio Open Small | MIT | Stability Community | Same as above |
| AudioLDM 2 | MIT | Research only | Need to negotiate |
| MusicGen | MIT | CC-BY-NC 4.0 | **No** (non-commercial) |
| CRASH | Unclear | Unclear | Unknown |
| DrumGAN | Unclear | Unclear | Probably not (Sony) |
| Riffusion | MIT | OpenRAIL-M | Yes with restrictions |
| VampNet | MIT | CC-BY-NC-SA 4.0 | **No** |
| TinyMusician | Unclear | Inherits CC-BY-NC | **No** |

**Winner: Stable Audio Open** -- the only production-quality model with a clear path
to commercial use. The $1M revenue threshold is generous for an indie DAW.

---

## 3. ONNX Export Feasibility

### 3.1 Architecture Challenges

Diffusion models have a multi-component pipeline that complicates ONNX export:

1. **Text encoder** (T5/CLAP) -- Straightforward ONNX export. Static input shapes.
2. **Diffusion backbone** (DiT/UNet) -- The denoising loop runs N steps, each calling the model. The model itself can be exported to ONNX; the loop is orchestrated in host code.
3. **VAE decoder** -- Straightforward ONNX export. Single forward pass.

The iterative denoising loop is NOT a blocker -- you export the single-step model and
run the loop in Rust. This is exactly how Stable Diffusion ONNX works for images.

### 3.2 Model-Specific ONNX Status

| Model | ONNX Status | Notes |
|-------|-------------|-------|
| Stable Audio Open 1.0 | **OpenVINO conversion documented** | Intel has a working notebook. Components (T5, DiT, VAE decoder) exported individually. Path to ONNX via PyTorch -> ONNX -> OpenVINO exists. |
| Stable Audio Open Small | **LiteRT (TFLite) conversion done** | ARM deployed it on mobile. ONNX conversion should follow similar pattern. |
| AudioLDM 2 | **OpenVINO conversion documented** | Similar component-wise export as Stable Audio. |
| MusicGen Small | **ONNX export exists** (Xenova/musicgen-small) | Proven working in transformers.js. TinyMusician also ONNX-exported for iOS. |
| CRASH | Not attempted | Small U-Net, should be trivial to export |

### 3.3 ONNX Export Strategy for Rust (`ort` crate)

```
Export pipeline (Python, one-time):
  1. Load model in PyTorch
  2. Export T5 encoder -> text_encoder.onnx
  3. Export DiT/UNet (single step) -> diffusion_model.onnx
  4. Export VAE decoder -> vae_decoder.onnx
  5. (Optional) Quantize with ONNX Runtime quantization tools

Runtime pipeline (Rust, via ort crate):
  1. Load all 3 ONNX models via ort::Session
  2. Tokenize text prompt (can use rust-tokenizers or pre-tokenize)
  3. Run text_encoder to get embeddings
  4. Initialize noise tensor
  5. Run diffusion_model in a loop (20-50 steps) with scheduler
  6. Run vae_decoder on final latents
  7. Output: raw audio samples at 44.1kHz
```

**Key considerations for ort integration:**
- ort v2.0 supports CUDA, CoreML, DirectML execution providers
- Dynamic shapes need `DynamicDimension` in session options
- The scheduler (DDPM, DPM++, etc.) must be reimplemented in Rust -- it's just math
- Total ONNX model sizes: ~2-5GB depending on quantization
- CPU inference with Int8 quantization is viable for the Small model

### 3.4 Inference Speed: ONNX vs PyTorch

ONNX Runtime is generally faster than PyTorch for inference:
- Static graph optimization eliminates Python overhead
- Operator fusion reduces memory bandwidth
- Int8/FP16 quantization further speeds up
- Expect 1.5-3x speedup over PyTorch for the diffusion step
- The VAE decoder benefits most from ONNX optimization

---

## 4. Deployment Architecture

### 4.1 Tier 1: Lite Local Mode (CPU)

**Model**: Stable Audio Open Small (341M), quantized Int8
**Hardware**: Any modern CPU (Intel/AMD/ARM), 4GB+ RAM
**Model size on disk**: ~2.9GB
**Peak RAM**: ~3.6GB
**Generation time**: ~7s for 10s audio on ARM; faster on x86 with AVX
**Max duration**: 11 seconds
**Use cases**: One-shot drums, synth stabs, short FX, 1-2 bar loops

Implementation:
```
ort Session with CPU ExecutionProvider
Int8 quantized ONNX models
Scheduler: DPM++ 2M Karras (fewer steps = faster)
20-30 diffusion steps (quality/speed tradeoff)
```

### 4.2 Tier 2: Standard Local Mode (GPU)

**Model**: Stable Audio Open 1.0 (1.2B)
**Hardware**: NVIDIA GPU with 8GB+ VRAM (RTX 3060+)
**Generation time**: ~3-5s for short samples, ~8-10s for 30s+ audio
**Max duration**: 47 seconds
**Use cases**: All one-shots, loops up to 8 bars, ambient textures

Implementation:
```
ort Session with CUDA ExecutionProvider
FP16 models
30-50 diffusion steps
Can run MusicGen Small alongside for melodic loops
```

### 4.3 Tier 3: Cloud Mode (RunPod/Modal)

**Model**: Stable Audio Open 1.0 + fine-tuned variants
**Hardware**: A10G ($0.40-0.76/hr) or A100 ($2.17/hr) on RunPod
**Generation time**: ~1-2s on A100
**Use cases**: Batch generation, highest quality, longer loops

Implementation:
```
RunPod Serverless with pay-per-second billing
Flask/FastAPI endpoint wrapping stable-audio-tools
Batch generation: generate 10 variations in parallel
Cost per generation: ~$0.001-0.005 per sample
WebSocket for streaming preview back to DAW
```

### 4.4 Cost Analysis (Cloud)

Using RunPod Serverless with A10G at $0.76/hr:
- 5s sample generation: ~3 seconds compute = ~$0.0006 per sample
- Batch of 10 variations: ~$0.006
- Heavy session (100 generations): ~$0.06
- Monthly active user (1000 generations): ~$0.60

Very affordable. Could be offered as a free/paid tier feature.

---

## 5. Loop Generation (BPM/Key Sync)

### 5.1 Current State of Art

No open-source model natively generates BPM-locked loops. The workflow is:

1. **Generate** raw audio with text prompt including tempo hint ("128 bpm techno loop")
2. **Detect** BPM of generated audio (beat tracking algorithm)
3. **Time-stretch** to exact target BPM
4. **Trim** to exact bar boundaries
5. **Crossfade** loop points

### 5.2 Practical BPM Sync Pipeline

```
User Input:
  prompt: "dark techno kick loop"
  target_bpm: 128
  bars: 4
  key: Am (optional)

Pipeline:
  1. Compute target duration: 4 bars * (60/128 * 4) = 7.5 seconds
  2. Generate with Stable Audio Open: "dark techno drum loop 128bpm" (duration=8s)
  3. Beat-track the output (use aubio/essentia algorithm in Rust)
  4. Time-stretch to exact 128 BPM (rubato crate already in Vibez)
  5. Trim to exactly 7.5 seconds (4 bars at 128bpm)
  6. Apply short crossfade at loop boundary
  7. Return loopable audio clip
```

### 5.3 Key/Scale Awareness

- Stable Audio Open has limited pitch awareness
- MusicGen is better at key-aware generation ("C minor techno bassline")
- Post-processing: pitch detection + pitch shift to target key is more reliable
- For one-shots: pitch is less critical (users tune in the sampler)

### 5.4 Open Source Tools for Loop Post-Processing (Rust)

- **rubato**: Already in Vibez, handles resampling and time-stretching
- **aubio-rs**: Rust bindings for beat tracking, onset detection, pitch detection
- **essentia** (via FFI): Comprehensive audio analysis
- Custom: Zero-crossing detection for clean loop points

---

## 6. One-Shot Generation Quality Assessment

### 6.1 What Works Well (Stable Audio Open)

Based on community testing and demos:

- **Kick drums**: Excellent. "808 kick", "punchy techno kick", "deep house kick" all produce usable results
- **Snares**: Good. "crispy snare", "acoustic snare", "trap snare" work well
- **Hi-hats**: Good. "closed hi-hat", "open hi-hat", "shaker"
- **Claps**: Good. "electronic clap", "layered clap"
- **Synth stabs**: Excellent. "saw stab", "brass stab", "chord stab"
- **Bass hits**: Good. "808 bass hit", "reese bass"
- **Percussion**: Very good. "conga", "bongo", "rim shot", "tambourine"
- **Ambient textures**: Excellent. "dark ambient pad", "vinyl crackle"

### 6.2 What Needs Work

- **Precise tonal control**: Can't specify exact pitch (C3, A2, etc.)
- **Transient quality**: Sometimes softens attacks compared to real samples
- **Consistency**: Same prompt produces varying quality; batch + selection is necessary
- **Very short samples**: Sometimes adds unwanted tail/reverb to what should be dry hits

### 6.3 Fine-Tuning Opportunity

Fine-tuning Stable Audio Open on curated drum sample datasets (e.g., CC-licensed
one-shots from Freesound) would significantly improve:
- Transient sharpness
- Category accuracy (kick vs. snare vs. hat)
- Consistency between generations
- Dry/clean output quality

---

## 7. Proposed API Design for Vibez

### 7.1 User-Facing Interface

```
+---------------------------------------------------+
|  AI Sample Generator                          [x]  |
+---------------------------------------------------+
|  Prompt: [dark techno kick with long tail    ] [>] |
|                                                     |
|  Type:  [One-shot v]  Duration: [Auto     v]       |
|  BPM:   [128      ]  Bars:     [--       ]        |
|  Key:   [--       ]  Variation: [=====|==]        |
|                                                     |
|  Mode:  (o) Local CPU  ( ) Local GPU  ( ) Cloud    |
|                                                     |
|  Results:  [1] [2] [3] [4] [5] [6] [7] [8]        |
|            [>]play [>]  [>]  [>]  [>]  [>]         |
|                                                     |
|  [ Generate 8 Variations ]  [ Drag to Arrangement ] |
+---------------------------------------------------+
```

### 7.2 Internal API

```rust
/// Configuration for sample generation
struct GenerateRequest {
    prompt: String,
    sample_type: SampleType,      // OneShot, Loop
    duration_secs: Option<f32>,   // None = auto (0.5s for kicks, 8s for loops)
    target_bpm: Option<f32>,      // For loops
    target_bars: Option<u32>,     // For loops
    target_key: Option<Key>,      // Optional pitch target
    num_variations: u32,          // 1-16
    seed: Option<u64>,            // For reproducibility
    quality: Quality,             // Draft (fewer steps), Normal, High
}

enum SampleType {
    OneShot { category: DrumCategory },
    SynthStab,
    Loop { style: LoopStyle },
    Texture,
}

enum DrumCategory {
    Kick, Snare, HiHat, Clap, Percussion, Tom, Cymbal,
}

struct GenerateResponse {
    variations: Vec<GeneratedSample>,
}

struct GeneratedSample {
    audio: Arc<DecodedAudio>,     // Reuse existing Vibez type
    sample_rate: u32,             // 44100
    channels: u32,                // 2 (stereo)
    detected_bpm: Option<f32>,    // Post-analysis
    detected_key: Option<Key>,    // Post-analysis
    prompt_used: String,
    seed: u64,
    generation_time_ms: u64,
}
```

### 7.3 Generation Pipeline

```rust
/// The core generation pipeline
async fn generate(req: GenerateRequest) -> GenerateResponse {
    // 1. Select backend based on available hardware
    let backend = select_backend(); // CPU, CUDA, Cloud

    // 2. Build enhanced prompt
    let prompt = enhance_prompt(&req);
    // "dark techno kick" -> "dark techno kick drum, one shot, dry, punchy, electronic"

    // 3. Compute duration
    let duration = match req.sample_type {
        SampleType::OneShot { .. } => req.duration_secs.unwrap_or(0.5),
        SampleType::Loop { .. } => compute_loop_duration(req.target_bpm, req.target_bars),
        SampleType::Texture => req.duration_secs.unwrap_or(5.0),
    };

    // 4. Generate N variations (parallel on GPU/cloud, sequential on CPU)
    let raw_samples = backend.generate_batch(
        &prompt,
        duration,
        req.num_variations,
        req.quality,
    ).await;

    // 5. Post-process
    let variations = raw_samples.into_iter().map(|raw| {
        let mut sample = raw;

        // Normalize
        sample = normalize_audio(sample);

        // For loops: beat-track, time-stretch, trim to bars
        if let SampleType::Loop { .. } = req.sample_type {
            if let Some(bpm) = req.target_bpm {
                sample = sync_to_bpm(sample, bpm, req.target_bars);
            }
        }

        // For one-shots: trim silence, apply fade-out
        if matches!(req.sample_type, SampleType::OneShot { .. }) {
            sample = trim_silence(sample);
            sample = apply_fade_out(sample, 0.01); // 10ms fade
        }

        sample
    }).collect();

    GenerateResponse { variations }
}
```

### 7.4 Backend Abstraction

```rust
trait GenerationBackend: Send + Sync {
    async fn generate_batch(
        &self,
        prompt: &str,
        duration_secs: f32,
        count: u32,
        quality: Quality,
    ) -> Vec<AudioBuffer>;

    fn name(&self) -> &str;
    fn estimated_time_secs(&self, duration_secs: f32, count: u32) -> f32;
}

struct OnnxLocalBackend {
    text_encoder: ort::Session,
    diffusion_model: ort::Session,
    vae_decoder: ort::Session,
    scheduler: DpmPlusPlusScheduler,
}

struct CloudBackend {
    endpoint: Url,
    api_key: String,
    client: reqwest::Client,
}
```

---

## 8. Implementation Roadmap

### Phase 1: Proof of Concept (1-2 weeks)
1. Export Stable Audio Open Small to ONNX (Python script)
2. Load in Rust with `ort` crate, verify inference works
3. Implement DPM++ scheduler in Rust
4. Generate first one-shot from Rust code
5. Wire up to Vibez UI as a simple dialog

### Phase 2: Local CPU Mode (2-3 weeks)
1. Optimize ONNX model with Int8 quantization
2. Implement prompt enhancement (append "one shot, dry, clean" etc.)
3. Add batch generation (sequential on CPU)
4. Post-processing: normalize, trim silence, fade
5. Drag-to-arrangement integration
6. Preview playback in generator UI

### Phase 3: GPU + Loop Support (2-3 weeks)
1. Add CUDA execution provider support
2. Load Stable Audio Open 1.0 (full) for GPU users
3. Implement BPM detection and time-stretch pipeline
4. Loop trimming to exact bar boundaries
5. Parallel batch generation on GPU

### Phase 4: Cloud Backend (1-2 weeks)
1. Create RunPod serverless endpoint
2. Implement WebSocket streaming for preview
3. Add cloud backend option to UI
4. Batch generation on cloud (parallel)

### Phase 5: Polish (ongoing)
1. Prompt library / presets ("Techno Kicks", "Lo-fi Drums", etc.)
2. Fine-tune model on curated drum samples
3. Add MusicGen Small as secondary engine for melodic loops
4. Parameter controls (pitch, brightness, decay) via prompt engineering
5. Save/favorite generated samples
6. Generation history

---

## 9. Key Dependencies for Rust

```toml
[dependencies]
ort = { version = "2.0", features = ["cuda"] }  # ONNX Runtime
tokenizers = "0.20"  # HuggingFace tokenizers for T5
ndarray = "0.16"     # Tensor operations
rand = "0.8"         # Noise generation for diffusion
rubato = "0.16"      # Already in Vibez -- resampling/time-stretch
```

---

## 10. Sources

### Models
- [Stable Audio Open 1.0 - HuggingFace](https://huggingface.co/stabilityai/stable-audio-open-1.0)
- [Stable Audio Open Small - HuggingFace](https://huggingface.co/stabilityai/stable-audio-open-small)
- [Stable Audio Open Paper](https://arxiv.org/abs/2407.14358)
- [Stable Audio Tools - GitHub](https://github.com/Stability-AI/stable-audio-tools)
- [Stability AI Community License](https://huggingface.co/stabilityai/stable-audio-open-1.0/blob/main/LICENSE.md)
- [Stable Audio Open + ARM KleidiAI](https://developer.arm.com/community/arm-community-blogs/b/ai-blog/posts/audio-generation-arm-cpus-stable-audio-open-small-kleidiai)
- [AudioLDM 2 - GitHub](https://github.com/haoheliu/AudioLDM2)
- [AudioLDM 2 Paper](https://arxiv.org/abs/2308.05734)
- [AudioLDM 2 - HuggingFace Diffusers](https://huggingface.co/docs/diffusers/en/api/pipelines/audioldm2)
- [MusicGen / AudioCraft - GitHub](https://github.com/facebookresearch/audiocraft)
- [MusicGen Small - HuggingFace](https://huggingface.co/facebook/musicgen-small)
- [CRASH - GitHub](https://github.com/simonrouard/CRASH)
- [CRASH Paper](https://arxiv.org/abs/2106.07431)
- [DrumGAN - GitHub](https://github.com/SonyCSLParis/DrumGAN)
- [VampNet - GitHub](https://github.com/hugofloresgarcia/vampnet)
- [TinyMusician Paper](https://arxiv.org/abs/2509.00914)
- [Xenova/musicgen-small ONNX](https://huggingface.co/Xenova/musicgen-small)

### ONNX/Inference
- [ort Rust crate](https://github.com/pykeio/ort)
- [ort Documentation](https://ort.pyke.io/)
- [Stable Audio Open + OpenVINO](https://docs.openvino.ai/2024/notebooks/stable-audio-with-output.html)
- [AudioLDM 2 + OpenVINO](https://docs.openvino.ai/2024/notebooks/sound-generation-audioldm2-with-output.html)
- [MusicGen ONNX export issue](https://github.com/huggingface/optimum/issues/1297)

### Deployment
- [RunPod Serverless Pricing](https://www.runpod.io/pricing)
- [RunPod Serverless for Generative AI](https://www.runpod.io/articles/guides/serverless-for-generative-ai)

### Loop/BPM Tools
- [DJ-IA VST (Stable Audio + BPM sync)](https://github.com/innermost47/ai-dj)
- [audio-loop-gen (MusicGen wrapper)](https://github.com/phdapps/audio-loop-gen)
- [Audacity tempo detection (C++ algorithm)](https://conference.audio.dev/a-fast-open-source-c-loop-classifier-and-tempo-estimator-new-tempo-detection-feature-in-audacity/)
