# Stem Separation: Technical Research

## TL;DR

- **Best path today**: htdemucs ONNX via `ort` crate — Rust crate `stem-splitter-core` already exists
- **Best quality**: BS-RoFormer (SOTA, +2.5 dB over Demucs) — ONNX export available
- **Future watch**: Moises-Light (13x smaller than BS-RoFormer, better quality, not yet open-sourced)
- **All MIT licensed** (Demucs, BS-RoFormer)

---

## Model Comparison

| Model | Vocals SDR | Drums SDR | Bass SDR | Avg SDR | Size | Notes |
|---|---|---|---|---|---|---|
| **BS-RoFormer** | 10.78 | 9.61 | **11.43** | **9.92** | ~200MB | SOTA open-source |
| **Mel-RoFormer** | **11.21** | **9.91** | 9.64 | 9.64 | ~200MB | Best vocals |
| htdemucs_ft (sparse) | 8.99 | 8.72 | 7.84 | 9.20 | ~320MB (4x81) | Fine-tuned, 4x slower |
| htdemucs | 8.12 | 8.45 | 7.16 | 7.33 | ~81MB | Default, fastest |
| Moises-Light | >BS-RoFormer | >BS-RoFormer | >BS-RoFormer | >9.92 | **13x smaller** | Paper only, not released |

### Best per stem
- **Vocals**: UVR-MDX ensemble or Mel-RoFormer
- **Drums**: BS-RoFormer or Mel-RoFormer
- **Bass**: BS-RoFormer (11.43 dB, dominant)

### Electronic music caveat
All models struggle more with EDM/techno/house: overlapping synth frequencies, heavy sidechain, wide stereo. Still usable but expect more bleed.

---

## Inference Performance (3-min song, htdemucs)

| Platform | Time | Notes |
|---|---|---|
| RTX 3060 (CUDA) | ~7-10s | Mainstream GPU |
| ONNX CPU | ~3.5-5 min | 18% faster than PyTorch |
| PyTorch CPU | ~4.5-6 min | Baseline |
| Apple Silicon (CPU) | ~3-4 min | Good IPC |

htdemucs_ft is 4x slower than htdemucs on all platforms.

---

## ONNX Export: Solved

**Mixxx GSOC 2025** successfully exported htdemucs v4 as fully self-contained ONNX:
- STFT/iSTFT rewritten as PyTorch convolutions (no complex tensors)
- Quality loss: <0.1 dB SI-SDR (effectively identical)
- CPU ONNX is 17.94% faster than PyTorch
- [PR upstream](https://github.com/adefossez/demucs/pull/10)
- [Blog post](https://mixxx.org/news/2025-10-27-gsoc2025-demucs-to-onnx-dhunstack/)

BS-RoFormer ONNX export via [ZFTurbo/MSS_ONNX_TensorRT](https://github.com/ZFTurbo/MSS_ONNX_TensorRT).

---

## Rust Integration

### Existing Rust implementations
| Project | Approach | URL |
|---|---|---|
| **stem-splitter-core** | Rust crate, ort + ONNX, no Python | [GitHub](https://github.com/gentij/stem-splitter-core) |
| **Stemgen** | Rust rewrite, ort + ONNX, NI Stem output | [GitHub](https://github.com/acolombier/stemgen) |
| demucs.cpp | C++17 with ggml, FFI-able | [GitHub](https://github.com/sevagh/demucs.cpp) |
| demucs.onnx | C++ ONNX reference | [GitHub](https://github.com/sevagh/demucs.onnx) |

### Dependencies needed
```toml
ort = { version = "2", features = ["download-binaries"] }
ndarray = "0.16"
# Optional GPU: features = ["cuda"] / "coreml" / "directml"
```

### Processing pipeline
1. Decode to PCM (symphonia — already have)
2. Resample to 44.1kHz stereo (rubato — already have)
3. Chunk into ~40s segments, 25% overlap
4. Run ONNX inference per chunk via ort
5. Overlap-add with linear crossfade
6. Output 4/6 stems

With self-contained ONNX, STFT is inside the model — no Rust FFT needed for inference.

---

## Deployment Tiers

### Local (default)
- CPU: works anywhere, ~5 min for 3 min song
- GPU: ~10-30s with CUDA/CoreML/DirectML
- Model: ~80MB one-time download, cache in `~/.vibez/models/`

### RunPod Serverless
- Upload audio → GPU worker → return stems
- Docker image `beveradb/audio-separator` exists
- Cost: fractions of a cent per separation
- FlashBoot ~1s cold start

### Hosted (Vibez API)
- Same as RunPod on own infra
- Paid cloud tier feature

---

## Architecture for Vibez

```
Right-click clip → "Separate Stems"
  ├── Model: [htdemucs / BS-RoFormer]
  ├── Stems: [4-stem / 6-stem]
  ├── Location: [Local / Cloud]
  └── [Separate]

→ Background thread (NOT audio thread, NOT UI thread)
→ Per-chunk progress via rtrb: "Processing chunk 3/8..."
→ Stems written to disk as they complete
→ Cache: project_dir/cache/stems/{track_id}/{model_name}/
→ New tracks created: "Vocals (separated)", "Drums (separated)", etc.
```

### Key decisions
- **Start with htdemucs ONNX** — best ecosystem, proven Rust path
- **Add BS-RoFormer ONNX later** — better quality, especially bass
- **Cache aggressively** — include model name in cache key
- **Bound memory** — one chunk at a time, write to disk incrementally
- **Model management** — download on first use, SHA-256 verify, `~/.vibez/models/`

### Note on Demucs status
Original author (Defossez) left Meta for Kyutai. Repo is maintenance-only. BS-RoFormer and Moises-Light are the future. But Demucs has the most mature ONNX + Rust story today.

---

## Key References
- [stem-splitter-core (Rust)](https://github.com/gentij/stem-splitter-core)
- [ort crate](https://github.com/pykeio/ort)
- [BS-RoFormer](https://github.com/lucidrains/BS-RoFormer)
- [ZFTurbo MSS Training](https://github.com/ZFTurbo/Music-Source-Separation-Training)
- [ZFTurbo MSS ONNX/TensorRT](https://github.com/ZFTurbo/MSS_ONNX_TensorRT)
- [Mixxx GSOC ONNX export](https://mixxx.org/news/2025-10-27-gsoc2025-demucs-to-onnx-dhunstack/)
- [python-audio-separator](https://github.com/nomadkaraoke/python-audio-separator)
- [Moises-Light paper](https://arxiv.org/abs/2510.06785)
