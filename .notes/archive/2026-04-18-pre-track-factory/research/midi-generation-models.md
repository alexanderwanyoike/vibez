# MIDI Generation Models & Approaches for Vibez DAW

Research conducted March 2026. Focused on open-source models that output MIDI (not audio), practical for integration into a desktop Rust DAW.

---

## 1. Open-Source MIDI Generation Models

### Tier 1: Best Candidates for DAW Integration

#### MIDI-GPT (Metacreation Lab) — AAAI 2025
- **What**: Controllable multitrack MIDI generation based on GPT-2 architecture
- **Architecture**: 8 attention heads, 6 layers, 512 embedding size, 2048 token attention window
- **Controls**: Instrument type, musical style, note density (1-10), polyphony level, note duration, velocity, micro-timing
- **Training data**: GigaMIDI (2.1M+ unique MIDI files, 1.8B note events)
- **License**: Open RAIL-M
- **Key strength**: Already integrated into Cubase DAW, Calliope web app, and Ableton plugin. Designed explicitly for computer-assisted composition workflows. Supports track-level and bar-level infilling.
- **Weights**: Available as `models/model.zip` in the GitHub repo (PyTorch format)
- **ONNX**: Not natively exported, but GPT-2-based architecture is straightforward to export
- **Repo**: https://github.com/Metacreation-Lab/MIDI-GPT

#### SkyTNT midi-model — Apache 2.0
- **What**: MIDI event transformer for symbolic music generation
- **Architecture**: Transformer, ~200M parameters (F32)
- **Key strength**: Already has ONNX export (`model_base.onnx`, `model_token.onnx`), multi-track generation, custom tokenizer (MIDITokenizerV2), active development, genre-specific LoRA fine-tunes (JPop, Touhou)
- **License**: Apache 2.0
- **Inference**: Supports both CUDA and CPU via ONNX Runtime
- **Repo**: https://github.com/SkyTNT/midi-model
- **HuggingFace**: https://huggingface.co/skytnt/midi-model

#### MIDI-RWKV — MIT License
- **What**: Small foundation model for symbolic music infilling, based on RWKV-7 linear architecture
- **Architecture**: RWKV-7, ~20M parameters
- **Key strength**: 20M params matches a 780M Anticipatory Transformer baseline (39x more efficient). Uses rwkv.cpp for inference with GGML format conversion. MIT licensed. Designed for edge devices.
- **Controls**: Numerical attribute controls for conditioning generation
- **Training data**: GigaMIDI
- **Context**: Handles sequences beyond 2048 training length via extrapolation
- **Repo**: https://github.com/christianazinn/MIDI-RWKV
- **Best for**: Infilling (generating missing bars given surrounding context)

#### MIDI-LLM — NeurIPS AI4Music 2025
- **What**: Text-to-MIDI generation by extending Llama 3.2 (1B) vocabulary with MIDI tokens
- **Architecture**: Llama 3.2 1B + 55,030 MIDI tokens = 1.47B parameters total
- **Inference speed**: 3-14x real-time speed. RTF of 3.33 at batch-1 (BF16). FP8 quantization gives ~20% further speedup.
- **Key strength**: Natural language conditioning ("compose a jazz piano piece in Bb minor"), leverages LLM world knowledge about musical styles
- **Downside**: 1.47B params is large for a DAW plugin — needs quantization
- **Repo**: https://github.com/slSeanWU/MIDI-LLM
- **HuggingFace**: https://huggingface.co/slseanwu/MIDI-LLM_Llama-3.2-1B

### Tier 2: Strong Research Models

#### Anticipatory Music Transformer
- **What**: Controllable multitrack generation via anticipatory infilling
- **Architecture**: Causal masked transformer (decoder-only), ~780M parameters
- **Key strength**: Can condition on existing tracks to generate accompaniment. Human evaluators rate accompaniments on par with human-composed music.
- **Training data**: Lakh MIDI Dataset
- **Downside**: Large model, Python/PyTorch only
- **Repo**: https://github.com/jthickstun/anticipation

#### ChatMusician (m-a-p) — LLaMA 2 + ABC Notation
- **What**: LLM with intrinsic musical abilities via ABC notation
- **Architecture**: LLaMA 2 fine-tuned on MusicPile (1.1M samples)
- **Key strength**: Uses pure text tokenizer (ABC notation), no special music tokenizer needed. Can be conditioned on texts, chords, melodies, motifs, musical forms.
- **Downside**: ABC notation is less precise than MIDI for DAW use. Suffers from hallucinations.
- **GGUF**: Available at `MaziyarPanahi/ChatMusician-GGUF` for llama.cpp inference
- **HuggingFace**: https://huggingface.co/m-a-p/ChatMusician

#### Composer's Assistant 2 (2024)
- **What**: Interactive multitrack MIDI infilling for REAPER DAW
- **Architecture**: T5-like encoder-decoder model
- **Controls**: Rhythmic conditioning, horizontal/vertical note onset density, pitch controls, rhythmic interest
- **Key strength**: Already DAW-integrated (REAPER). Trained only on permissively-licensed MIDI. Open source with pretrained weights.
- **Repo**: https://github.com/m-malandro/composers-assistant-REAPER

#### MuseCoco (Microsoft Muzic) — Text-to-Symbolic Music
- **What**: Two-stage text-to-attribute then attribute-to-music generation
- **Architecture**: 1.2B parameters
- **Controls**: Instrument, rhythm danceability, rhythm intensity, genre, bar, time signature, key, tempo, pitch range, emotion
- **Key strength**: Very fine-grained controllability. Self-supervised training (attributes extracted from music).
- **Repo**: https://github.com/microsoft/muzic

#### FIGARO — Controllable with Expert Features
- **What**: Transformer auto-encoder for controllable multi-track generation
- **Architecture**: 4 encoder + 6 decoder layers, REMI+ tokenization
- **Controls**: Instrument, harmony, meta-information (tempo, style)
- **Training data**: Lakh MIDI Dataset
- **Key strength**: Fine-grained control via "domain expert" descriptions

#### Amadeus (August 2025) — Autoregressive + Bidirectional Diffusion
- **What**: Two-level architecture with autoregressive note sequences and bidirectional discrete diffusion for attributes
- **Key strength**: 4x speedup over SOTA, training-free fine-grained note attribute control
- **Dataset**: AMD (Amadeus MIDI Dataset) — largest open-source symbolic music dataset to date

### Tier 3: Older / Narrower Models

#### Google Magenta Suite
- **DrumsRNN**: LSTM for drum track generation. Polyphonic. Maps MIDI drums to classes.
- **MelodyRNN**: LSTM with Lookback (1-2 bar pattern recognition). Simple, fast.
- **GrooVAE**: VAE for adding human-like velocity/timing to quantized drum patterns.
- **MusicVAE**: VAE for interpolation between musical phrases.
- **Key strength**: Well-tested, small models, TensorFlow.js compatible
- **Downside**: Dated (RNN-based), limited long-term structure
- **Groove MIDI Dataset**: 13.6 hours of human-performed drum MIDI with velocity/timing humanization

---

## 2. MIDI Tokenization Approaches

### MidiTok Library (Python)
The standard library for MIDI tokenization. Repo: https://github.com/Natooz/MidiTok

| Tokenization | Description | Sequence Length | Multi-track | Best For |
|---|---|---|---|---|
| **REMI** | Pitch + Velocity + Duration + Bar + Position tokens | Long | No (single track) | Simple melody/drum generation |
| **REMI+** | REMI extended with Program tokens for multi-track | Long | Yes | Multi-track with instruments |
| **TSD** (Time Shift Duration) | Similar to REMI but with TimeShift instead of Bar/Position | Medium | Limited | Compact single-track |
| **Structured** | Fixed pattern: Pitch→Velocity→Duration→TimeShift | Medium | Limited | Consistent structure, easier training |
| **Octuple** | Embedding pooling — each pooled embedding = one note | Short | Yes | Efficient training, less memory |
| **Compound Word** | Like REMI but with pooled note embeddings | Short | Yes | Reduced sequence length |
| **MMM** (Multi-track Music Machine) | Track-by-track concatenation (used by MIDI-GPT) | Medium | Yes | Multitrack infilling |

### Key Insight: BPE on MIDI Tokens
Byte-Pair Encoding applied to MIDI token sequences reduces sequence lengths and improves model performance. TchAIkovsky demonstrated this with REMI + BPE.

### ABC Notation (ChatMusician approach)
- Represents music as text: `X:1\nT:Song\nM:4/4\nK:C\n|CDEF|GABc|...`
- Advantage: Works with any text LLM, no special tokenizer
- Disadvantage: Less precise for timing/velocity, harder to convert back to MIDI cleanly

### For Vibez: Recommendation
Use **REMI+** or **Structured** tokenization for any custom training. For integrating existing models, match whatever the model was trained on. The MIDI-GPT MMM representation is the most practical for multitrack DAW use.

---

## 3. Small/Efficient Models for Desktop Use

| Model | Parameters | Format | CPU Feasible? | Notes |
|---|---|---|---|---|
| **MIDI-RWKV** | 20M | PyTorch/GGML | Yes, excellent | Linear complexity (RWKV), matches 780M transformer |
| **SkyTNT midi-model** | 200M | ONNX available | Yes, reasonable | ONNX already exported |
| **MIDI-GPT** | ~50-100M (est.) | PyTorch | Yes, with ONNX export | GPT-2 small variant |
| **Magenta DrumsRNN** | ~1-5M | TensorFlow | Yes, trivial | Very small LSTM |
| **Magenta MelodyRNN** | ~1-5M | TensorFlow | Yes, trivial | Very small LSTM |
| **MIDI-LLM** | 1.47B | PyTorch/HF | Needs quantization (FP8/INT4) | Large but fast with quant |
| **ChatMusician** | ~7B | GGUF available | Needs quantization, borderline | Really a full LLM |
| **Composer's Assistant** | Unknown (T5-like) | PyTorch | Likely yes | Moderate size |

### Practical Desktop Targets
- **Under 50M params**: Runs on any CPU in <1 second per bar. Best for real-time interaction.
- **50-200M params**: Runs on CPU in 1-5 seconds per bar. Acceptable for "generate and wait" workflow.
- **200M-1B params**: Needs GPU or aggressive quantization. Borderline for desktop.
- **Over 1B params**: GPU required or very slow on CPU. Better as optional/cloud feature.

### Winner for Desktop: MIDI-RWKV (20M params)
At 20M parameters with RWKV's linear complexity, this is the most practical model for CPU inference in a DAW. It matches a 780M transformer on quality. Can use rwkv.cpp for C/C++ inference (callable from Rust via FFI).

---

## 4. Controllability Comparison

| Model | Genre/Style | Tempo | Key/Scale | Density | Conditioning on Tracks | Text Prompt |
|---|---|---|---|---|---|---|
| **MIDI-GPT** | Yes | Implicit | No | Yes (1-10) | Yes (infilling) | No |
| **MIDI-RWKV** | Via fine-tune | Implicit | No | Yes (attrs) | Yes (infilling) | No |
| **SkyTNT midi-model** | Via LoRA | Implicit | No | Limited | Limited | No |
| **MIDI-LLM** | Yes (text) | Yes (text) | Yes (text) | Yes (text) | No | Yes |
| **ChatMusician** | Yes (text) | Yes (text) | Yes (text) | No | Yes (chords) | Yes |
| **MuseCoco** | Yes | Yes | Yes | Yes | No | Yes |
| **Anticipatory** | Implicit | Implicit | No | No | Yes (best) | No |
| **Composer's Asst 2** | No | No | Yes (pitch) | Yes (onset) | Yes (infilling) | No |
| **Magenta DrumsRNN** | Limited | Via input | No | Via priming | No | No |

### For DAW Use: Two Modes of Control
1. **Attribute-based** (MIDI-GPT, MIDI-RWKV): Set parameters like density=7, instrument=drums. Best for quick generation.
2. **Text-based** (MIDI-LLM, ChatMusician): "Generate a funky drum pattern in 120 BPM". More intuitive but needs larger model.

---

## 5. Architecture Options Analysis

### A. Fine-tuned LLMs on MIDI (ChatMusician, MIDI-LLM)
- **Pros**: Natural language control, leverage pretrained language understanding, can describe style in words
- **Cons**: Very large (1B+), slow on CPU, overkill for structured MIDI generation
- **Verdict**: Best as optional cloud-backed feature, not for local real-time use

### B. Transformer Models Trained on MIDI (MIDI-GPT, SkyTNT, Anticipatory)
- **Pros**: Purpose-built for music, good quality, moderate size (50M-800M), ONNX exportable
- **Cons**: Still need GPU for large variants, quadratic attention complexity limits context
- **Verdict**: Best balance of quality and practicality. MIDI-GPT is the proven DAW integration choice.

### C. RWKV/Linear Models (MIDI-RWKV)
- **Pros**: Linear complexity, tiny model (20M), matches much larger transformers, great for long sequences
- **Cons**: Newer approach, fewer pretrained models available, less community
- **Verdict**: Best for CPU-only desktop deployment. Most promising for Vibez's use case.

### D. Diffusion Models for MIDI (Mamba-Diffusion, GETMusic)
- **Pros**: Good global coherence, can generate entire pieces at once, controllable via guidance
- **Cons**: Multiple denoising steps = slower inference, less mature for symbolic music
- **Verdict**: Promising research direction but not practical for DAW integration yet.

### E. VAE/GAN (Magenta GrooVAE, MusicVAE)
- **Pros**: Very fast inference, good for interpolation/variation, tiny models
- **Cons**: Limited generation length (typically 2-4 bars), older approach
- **Verdict**: Excellent for specific tasks (humanize drums, interpolate patterns) but not general generation.

### F. Rule-Based + ML Hybrid
- **Pros**: Predictable, fast, no model weight loading, musically correct by construction
- **Cons**: Limited creativity, sounds mechanical without ML refinement
- **Approach**: Use Euclidean rhythms (Bjorklund algorithm) for drums, Markov chains for melodies, then optionally refine with a small ML model
- **Verdict**: Best as the baseline/fallback that works instantly, with ML as an enhancement layer.

---

## 6. Practical Integration Plan for Vibez

### Recommended Architecture: Tiered Approach

```
Tier 0: Rule-Based (built-in, instant, no model required)
  - Euclidean drum patterns (Bjorklund algorithm)
  - Scale-aware random melodies with Markov chains
  - Genre-specific templates (house kick pattern, trap hi-hats, etc.)
  - Arpeggiator patterns

Tier 1: Small ML Model (bundled, CPU, <1s)
  - MIDI-RWKV (20M params) via rwkv.cpp FFI or GGML
  - OR small custom transformer (1-20M params) via ONNX + ort crate
  - Task: infilling, continuation, humanization

Tier 2: Medium ML Model (optional download, CPU/GPU, 1-10s)
  - SkyTNT midi-model (200M) via ONNX + ort crate
  - OR MIDI-GPT via ONNX export + ort crate
  - Task: full multitrack generation, style-conditioned generation

Tier 3: Large Model / Cloud (optional, requires API/GPU)
  - MIDI-LLM (1.47B) for text-to-MIDI
  - Task: "Generate a lo-fi hip hop beat with jazzy chords"
```

### Rust Integration Stack

```
MIDI I/O:        midly crate (parse/write .mid files)
ONNX Inference:  ort crate (wraps ONNX Runtime, GPU + CPU)
                 OR tract crate (pure Rust, CPU only, no C++ deps)
                 OR burn crate (Rust-native, ONNX import via burn-onnx)
RWKV Inference:  rwkv.cpp via FFI (for MIDI-RWKV)
                 OR candle crate (HuggingFace's pure-Rust ML framework)
Tokenization:    Custom Rust implementation of REMI/Structured tokenizer
                 (MidiTok is Python-only, need Rust port of tokenizer logic)
```

### DAW Workflow

```
1. User selects empty bars on a track (or right-clicks → "Generate MIDI")
2. UI shows generation panel with controls:
   - Pattern type: Drums / Melody / Bass / Chords / Arpeggio
   - Genre preset: House / Techno / Hip-Hop / Lo-Fi / Jazz / etc.
   - Density slider (sparse → dense)
   - Complexity slider (simple → complex)
   - Key/Scale selector (from project or manual)
   - Tempo (from project)
   - [Optional] "Condition on track:" dropdown (feed existing track as context)
3. "Generate" button → runs inference in background thread
4. Generated MIDI appears as a clip on the track
5. User can: accept, regenerate, or edit manually
```

### Latency Expectations
- **Tier 0 (rule-based)**: <10ms, instant
- **Tier 1 (20M RWKV)**: 100-500ms for 4 bars on CPU
- **Tier 2 (200M transformer ONNX)**: 1-5 seconds for 4 bars on CPU, <1s on GPU
- **Tier 3 (1.47B LLM)**: 10-30 seconds on CPU, 2-5 seconds on GPU

### ONNX Export Feasibility
- **SkyTNT midi-model**: Already has ONNX export. Ready to use.
- **MIDI-GPT**: GPT-2 architecture, straightforward to export via `torch.onnx.export()` or HuggingFace Optimum
- **MIDI-RWKV**: RWKV has ONNX export support, also has GGML/rwkv.cpp path
- **Magenta models**: TensorFlow format, can convert via tf2onnx
- **MIDI-LLM**: Llama architecture, can export via llama.cpp GGUF or ONNX with optimum

---

## 7. Training Data / Datasets

| Dataset | Size | Content | License | Best For |
|---|---|---|---|---|
| **GigaMIDI** (Metacreation) | 2.1M+ files, 1.8B notes | All genres, expressive performance annotations | CC BY 4.0 | General training, largest available |
| **Lakh MIDI** | 176,581 files | Multi-genre, matched to Million Song Dataset | Research use | Multi-track, genre diversity |
| **MidiCaps** (2024) | 168,407 files | Lakh subset with text captions (tempo, key, genre, mood) | Research | Text-conditioned training |
| **Groove MIDI** (Google) | 1,150 files, 13.6 hours | Human-performed drums, velocity + microtiming | CC BY 4.0 | Drum humanization |
| **E-GMD** (Expanded Groove) | Larger than GMD | Extended drum performances | CC BY 4.0 | Drum transcription + generation |
| **POP909** | 909 songs | Pop songs with melody, bridge, piano arrangement, chord annotations | Research | Arrangement, chord-conditioned |
| **Aria-MIDI** (2025) | 1.18M files, ~100K hours | Transcribed solo piano | Research | Piano-specific |
| **AMD** (Amadeus, 2025) | "Largest open-source" | Symbolic music, pre-training + fine-tuning splits | TBD | General-purpose |
| **Slakh2100** | 2,100 tracks | Multi-track audio + aligned MIDI, synthesized from Lakh | Research | Multi-instrument |
| **ADL Piano MIDI** | ~11,000 files | Piano performances | CC BY 4.0 | Piano generation |

---

## 8. Recommended Implementation Roadmap for Vibez

### Phase 1: Rule-Based Generator (Pure Rust, no ML)
Ship first with algorithmic generation:
- Euclidean rhythm generator for drums (Bjorklund algorithm — trivial to implement in Rust)
- Genre template library (JSON/TOML files defining common patterns for house, techno, trap, etc.)
- Scale-constrained random melody with Markov chains
- Arpeggiator with pattern presets
- Humanization: random velocity variation + slight timing offset

**Effort**: 1-2 weeks. **Value**: Immediately useful, zero dependencies.

### Phase 2: Small ONNX Model Integration
Add ML-powered generation:
- Integrate `ort` crate for ONNX Runtime
- Bundle SkyTNT midi-model ONNX files (~200M params, already exported)
- Implement REMI/Structured tokenizer in Rust (port from MidiTok Python logic)
- Run inference in background thread, present results as MIDI clips

**Effort**: 2-4 weeks. **Value**: Genuinely creative generation.

### Phase 3: MIDI-RWKV for Infilling
Add context-aware generation:
- Integrate MIDI-RWKV (20M params) via rwkv.cpp FFI or GGML
- Enable "condition on existing tracks" — feed surrounding bars as context
- Enable bar-level infilling (select empty bars, generate based on context)

**Effort**: 2-3 weeks. **Value**: Musical coherence with existing arrangement.

### Phase 4: Text-to-MIDI (Optional/Cloud)
For users with capable hardware or API access:
- Integrate MIDI-LLM via GGUF/llama.cpp or cloud API
- Natural language prompt: "funky bass line in E minor, 16th note feel"
- Could also be a cloud endpoint that Vibez calls

**Effort**: 2-4 weeks. **Value**: Most intuitive interface, but requires resources.

---

## 9. Key Takeaways

1. **MIDI-RWKV is the efficiency champion**: 20M params matching 780M transformers, MIT license, designed for edge. Best default choice for Vibez.

2. **MIDI-GPT has the most DAW validation**: Already in Cubase and Ableton plugins. Proven workflow. Attribute-based control is ideal for DAW UX.

3. **SkyTNT midi-model is the easiest to integrate**: Apache 2.0, ONNX already exported, 200M params is manageable.

4. **Start with rule-based**: Euclidean rhythms + genre templates give immediate value with zero ML dependencies. Layer ML on top.

5. **The `ort` crate is the integration path**: Rust ONNX Runtime bindings, battle-tested, supports CPU and GPU. Alternatively, `tract` for pure Rust with no C++ dependencies.

6. **Tokenization must be reimplemented in Rust**: MidiTok is Python-only. You need a Rust port of whichever tokenization the chosen model uses.

7. **GigaMIDI is the dataset**: 2.1M files, CC BY 4.0, all genres. If you ever train a custom model, this is the data.

8. **Infilling > generation**: For a DAW, "fill in these 4 bars given context" is more useful than "generate something from scratch." MIDI-RWKV and MIDI-GPT both support this.

---

## Sources

- [MIDI-GPT (Metacreation Lab)](https://github.com/Metacreation-Lab/MIDI-GPT)
- [MIDI-GPT Paper (AAAI 2025)](https://arxiv.org/abs/2501.17011)
- [SkyTNT midi-model](https://github.com/SkyTNT/midi-model)
- [MIDI-RWKV](https://github.com/christianazinn/MIDI-RWKV)
- [MIDI-LLM (NeurIPS AI4Music 2025)](https://github.com/slSeanWU/MIDI-LLM)
- [ChatMusician](https://huggingface.co/m-a-p/ChatMusician)
- [Anticipatory Music Transformer](https://github.com/jthickstun/anticipation)
- [Composer's Assistant 2](https://github.com/m-malandro/composers-assistant-REAPER)
- [MuseCoco (Microsoft Muzic)](https://github.com/microsoft/muzic)
- [FIGARO](https://arxiv.org/abs/2201.10936)
- [Amadeus](https://arxiv.org/abs/2508.20665)
- [MidiTok](https://github.com/Natooz/MidiTok)
- [GigaMIDI Dataset](https://huggingface.co/datasets/Metacreation/GigaMIDI)
- [Groove MIDI Dataset](https://magenta.withgoogle.com/datasets/groove)
- [Aria-MIDI Dataset](https://arxiv.org/abs/2504.15071)
- [POP909 Dataset](https://github.com/music-x-lab/POP909-Dataset)
- [Lakh MIDI Dataset](https://colinraffel.com/projects/lmd/)
- [ort crate (Rust ONNX Runtime)](https://github.com/pykeio/ort)
- [tract crate (Pure Rust ONNX)](https://github.com/sonos/tract)
- [Candle (HuggingFace Rust ML)](https://github.com/huggingface/candle)
- [Burn (Rust DL framework)](https://github.com/tracel-ai/burn)
- [midly crate (Rust MIDI)](https://github.com/kovaxis/midly)
- [TchAIkovsky](https://huggingface.co/blog/afmck/tchaikovsky)
- [Magenta Studio](https://magenta.withgoogle.com/)
- [Euclidean Rhythms (Toussaint)](https://cgm.cs.mcgill.ca/~godfried/publications/banff.pdf)
- [Mamba-Diffusion for Music](https://arxiv.org/abs/2505.03314)
- [RTen (Rust ONNX Runtime)](https://robertknight.me.uk/posts/rten-2025/)
