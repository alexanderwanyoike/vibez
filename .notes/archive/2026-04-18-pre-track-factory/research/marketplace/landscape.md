# Plugin Marketplace Landscape

## Current State: Fragmented and Ripe for Disruption

Producers juggle multiple storefronts (Plugin Boutique, Splice, KVR, dev websites), multiple plugin managers (Native Access, PAIM, FL Cloud, iLok), and multiple licensing systems. No single platform owns discovery → install → updates.

---

## Existing Marketplaces

### Major Storefronts
| Platform | Model | Revenue Split | Notes |
|---|---|---|---|
| **Plugin Boutique** | Buy / rent-to-own | ~70/30 | Largest third-party store, sales/bundles |
| **Splice Plugins** | Rent-to-own | ~70/30 | Interest-free monthly, pause/cancel |
| **Plugin Alliance** | Subscription + buy | Varies | Monthly "Mega Bundle" subscription |
| **Native Instruments** | Buy + NI+ sub | — | First-party + curated third-party |
| **KVR Audio** | Marketplace + free | Varies | Community-driven, many free plugins |

### In-DAW Stores
| DAW | Store | Content | Integration |
|---|---|---|---|
| **FL Studio (FL Cloud)** | Deep in-DAW | Plugins, sounds, mastering, distribution | Most aggressive — full subscription inside DAW |
| **Ableton Packs** | In-DAW browser | First-party only | Packs tab in browser |
| **Logic Sound Library** | In-DAW downloads | First-party only | Apple content only |
| **Studio One+ (Sphere)** | Subscription | PreSonus content | Subscription-gated |
| **Bitwig** | None | — | Co-created CLAP but no marketplace |
| **Any open-source DAW** | **None** | — | **Wide open opportunity** |

### Lessons from Other Domains
| Platform | Split | Key Lesson |
|---|---|---|
| **Epic Fab** | 88/12 | Set new benchmark, undercut Unity |
| **Blender Market (Superhive)** | Up to 90/10 | Optional % to Blender Foundation |
| **Godot Marketplace** | 90/10 | 10% to Godot development |
| **VS Code Extensions** | Free | Extensions are free, drives platform adoption |
| **Unity Asset Store** | 70/30 → losing sellers to Fab | Old split is no longer competitive |
| **crates.io** | Free | Open-source registry model |

---

## Revenue Split: The New Standard

The 70/30 era is ending. To attract quality content:
- **Target: 85/15 or 88/12**
- Optional donation to Vibez project (like Blender's 5% / Godot's 10%)
- More generous than Plugin Boutique, competitive with Epic Fab

---

## Payment Models That Work

1. **Buy outright** — Standard, always offer this
2. **Rent-to-own** (Splice model) — Monthly payments, perpetual license when paid off. Enormously popular. Lowers barrier for expensive plugins
3. **Free + donation** — For open-source/community plugins
4. **Subscription bundle** — All-access tier for power users (Phase 3)

---

## Plugin Format: CLAP is the Natural Choice

| Format | License | Notes |
|---|---|---|
| **CLAP** | MIT | Open-source, no fees, single `.clap` file, growing (394 plugins, 15 DAWs) |
| **VST3** | MIT (recently) | Industry standard, more complex packaging |
| **LV2** | ISC | Linux-native, open |
| **AU** | Apple | macOS only |

CLAP's single-file format makes packaging dramatically simpler. Support CLAP first, VST3 second, LV2 for Linux community.

---

## DRM: Less is More

The audio community **hates** aggressive DRM:
- iLok: "highest maintenance copy protection ever made"
- Joey Sturgis Tones publicly dropped iLok in 2025 due to backlash
- PACE kernel drivers cause system instability

**The right approach**: Make buying easier than pirating.
- RSA-signed license files (offline validation)
- No phone-home after activation
- No kernel drivers, no dongles
- 3 machine activations with web deactivation
- Same model as FabFilter, u-he, ValhallaDSP, Xfer — the most respected vendors

---

## Community Features Drive Growth

**Vital's lesson**: Free synth with massive community preset sharing = explosive growth.
**PresetShare**: 354K users, 15M downloads of free presets.

A built-in sharing system differentiates Vibez from every commercial DAW:
- Free community asset library (like Godot Asset Library)
- Commercial marketplace alongside it (like Godot Marketplace/Superhive)
- Covers both open-source ethos and developer sustainability

---

## Indie Developer Pain Points

1. **Discovery**: Hard to get noticed among thousands of plugins
2. **Payment processing**: Setting up Stripe/PayPal, handling taxes, refunds
3. **Multi-platform builds**: Linux/Windows/macOS × multiple formats
4. **Copy protection**: Build vs. buy vs. go without
5. **Updates**: No standard update mechanism, users run old versions
6. **Support burden**: One-person shops overwhelmed by support tickets

A good marketplace solves all of these.
