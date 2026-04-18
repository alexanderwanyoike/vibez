# Vibez AI Vision: Synthesis & Strategic Positioning

## The Opportunity

**No desktop-native, open-source, AI-integrated DAW exists.** Period.

- Suno Studio: browser-based, closed-source
- RipX: proprietary, Windows/Mac only
- Ardour/LMMS/Audacity: zero AI
- Logic/FL/Ableton: AI bolted on as features, not architecture

Vibez is Rust-native, cross-platform, open-source, and early enough to build AI into the architecture rather than graft it on.

---

## Design Philosophy

### "Co-pilot, not autopilot"
- >80% of producers reject autonomous AI songs
- They want AI to handle tedium and spark ideas
- Every AI action must be: **reversible, explainable, optional**

### "AI as time machine"
- Skip the boring parts (gain staging, EQ cleanup, tempo detection)
- Accelerate exploration (sound design, arrangement, mixing)
- Never replace the creative decision

### Two interaction modes
1. **Direct manipulation** — traditional DAW controls (timeline, piano roll, mixer knobs)
2. **Conversational** — natural language sidebar for intent-driven actions

Both operate on the same state. "Move this note up" with the mouse or "make the bass follow the kick" in the chat — same undo stack, same result.

---

## Architecture: Three AI Tiers

```
Tier 1: REAL-TIME (in/near audio callback, <10ms)
  - Onset/pitch/beat detection
  - Lightweight neural effects (timbre transfer via BRAVE-class models)
  - Key/scale detection
  - Runtime: ort crate (ONNX Runtime, Rust-native)

Tier 2: RESPONSIVE (async workers, 100ms-2s)
  - EQ/mix suggestions
  - Chord recognition
  - Audio-to-MIDI transcription
  - Smart auto-naming/coloring
  - Runtime: ort crate on worker threads, results via rtrb

Tier 3: GENERATIVE (background, 2s-30s)
  - Stem generation (ACE-Step, MusicGen)
  - Stem separation (Demucs)
  - Arrangement suggestions
  - Text-to-sound design
  - LLM intent parsing (conversational interface)
  - Runtime: ort or direct model loading, dedicated thread pool
```

All tiers communicate with UI via existing rtrb channels. No cloud dependency. Everything runs locally.

---

## Feature Priorities (What to Build When)

### Phase A: Low-controversy, high-impact (friction reducers)
These make Vibez immediately more useful without touching creative decisions:
- Auto tempo/key detection on audio import
- Smart track naming and color assignment
- Beat/transient detection for grid snapping
- Audio-to-MIDI transcription
- Reference track analysis (key, tempo, structure, spectral profile)

### Phase B: Intelligent assistance (co-pilot features)
- EQ suggestions with reasoning ("high-passing at 80Hz, kick lives there")
- Frequency conflict detection across tracks
- Mix reference comparison ("your low-end is 3dB hotter than reference")
- Arrangement structure detection from audio
- Chord progression display from audio/MIDI

### Phase C: Generative co-creation
- AI stem generation (generate a fitting bass/drums/pad for the current project)
- Stem separation (import any track, decompose to stems)
- Text-to-sound design (describe a sound, get a patch)
- Conversational DAW control (DAWZY MCP pattern)
- Energy curve editor (draw intensity, AI arranges)

### Phase D: Deep intelligence
- Per-artist style learning (LoRA fine-tuning on user's tracks)
- Latent space sound design (2D timbre X/Y pads)
- Semantic audio editing ("make this warmer", "tighten the drums")
- Intelligent arrangement from loops (full song structure with transitions)

---

## Key UI Concepts

### 1. Conversational Sidebar
- Chat panel alongside timeline (collapsible)
- AI actions appear as timeline events (reversible)
- System explains reasoning for every suggestion
- Voice input via Whisper (optional)
- Architecture: MCP-style permissioned tool functions

### 2. Energy Curve Editor
- Drawable overlay on the arrangement timeline
- Y-axis = intensity/energy, X-axis = time
- AI interprets curve into arrangement decisions (density, filter, dynamics)
- "Build toward bar 64" or draw the shape
- **No existing DAW has this.** Genuine differentiator.

### 3. Latent Space Navigator
- 2D X/Y pad widget for exploring:
  - Timbre spaces (warm/cold, bright/dark)
  - Style spaces (genre interpolation)
  - Arrangement density
- Based on pGESAM architecture (VAE disentangling)
- More intuitive than knob-per-parameter

### 4. Contextual Suggestion Cards
- Non-intrusive cards that appear based on current task
- "This track has frequency masking with Track 3" → suggest EQ
- "This section is 8 bars repeated 4x" → suggest variation
- Dismissable, learnable (AI learns what you ignore)

---

## Technical Building Blocks

### Rust-native
- **`ort`** crate: ONNX Runtime bindings, CPU/CUDA/TensorRT/CoreML/DirectML
- **MusicGPT**: Proof that Rust + MusicGen works
- **whisper-rs**: Whisper bindings for voice input
- **tract**: Pure Rust ONNX/TF-lite inference (alternative to ort)

### Models to integrate (all open-source/open-weight)
| Model | Use | Size |
|---|---|---|
| ACE-Step 1.5 | Full song/stem generation | 3.5B, <4GB VRAM |
| Demucs | Stem separation | ~150MB |
| MusicGen | Text/melody-to-music | 300M-3.3B |
| BasicPitch | Audio-to-MIDI | ~10MB |
| Whisper | Voice input | 39M-1.5B |
| BRAVE/RAVE | Neural audio effects | ~50MB |
| CLAP | Audio-text embeddings | ~600MB |

### Reference implementations
- **DAWZY**: MCP architecture for LLM-DAW communication ([arXiv 2512.03289](https://arxiv.org/abs/2512.03289))
- **WigAI**: MCP server for Bitwig ([github.com/fabb/WigAI](https://github.com/fabb/WigAI))
- **Neutone SDK**: Real-time neural audio deployment
- **MixAssist**: Conversational mixing dataset/model ([arXiv 2507.06329](https://arxiv.org/abs/2507.06329))

---

## Competitive Positioning

```
                    Open Source
                        |
                  Vibez  |
                   (here)|
                        |
   Traditional ---------|--------- AI-Native
                        |
             Ardour     |     Suno Studio
             LMMS       |     Soundverse
                        |     ProducerAI
                        |
                    Closed Source
```

Vibez occupies the only empty quadrant: **open-source + AI-native**.

---

## Open Questions
- Local-only vs. optional cloud? (Privacy-first, but some models need GPUs users don't have)
- Ship models bundled or download-on-demand? (Binary size vs. first-run experience)
- LLM for conversational interface: local (Llama 3) or API (Claude)? Or both?
- How to handle model updates without breaking user workflows?
- Plugin format for community AI models? (ONNX as universal format?)

---

## Sources
See `landscape.md` and `ux-paradigms.md` for full references.
