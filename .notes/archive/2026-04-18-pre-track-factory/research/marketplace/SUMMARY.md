# Plugin Marketplace: Summary of Takeaways

## The Opportunity

No open-source DAW has a marketplace. The current ecosystem is fragmented — producers juggle Plugin Boutique, Splice, KVR, NI, and individual dev websites with separate accounts, managers, and license systems. An integrated in-DAW marketplace is a major differentiator.

## Core Decisions

### Revenue Split: 85/15
- Old 70/30 is dead (Epic set 88/12, Blender/Godot at 90/10)
- 85/15 is competitive and sustainable
- Optional % donation to Vibez project (like Blender's 5%)

### Payment: Buy + Rent-to-Own
- Buy outright (always available)
- Rent-to-own (Splice model) for expensive plugins — enormous demand
- Free + donation tier for community/open-source content
- Stripe Connect handles everything: splits, payouts, KYC, taxes

### Licensing: Trust-Based, Offline
- RSA-signed license files — verify locally, no internet after download
- 3 machine activations, web deactivation
- No DRM, no dongles, no kernel drivers, no phone-home
- Same model as FabFilter, u-he, ValhallaDSP (the most respected vendors)

### Format: CLAP First
- MIT licensed, no fees, single `.clap` file
- Growing ecosystem (394 plugins, 15 DAWs)
- Add VST3 + LV2 support later

### UI: Native In-DAW
- iced UI for browse/search/install (consistent with DAW)
- Stripe Checkout via browser for payment (PCI compliance)
- One-click install with progress

## Content Types (ordered by launch priority)
1. Preset packs (easiest, high demand)
2. Sample packs (WAV/FLAC, large files)
3. MIDI packs (tiny, immediate value with AI MIDI features)
4. Project templates
5. CLAP plugins (needs validation pipeline)
6. AI model weights (future differentiator)

## Tech Stack
- **Backend**: axum + diesel-async + PostgreSQL (same as crates.io)
- **Storage**: Cloudflare R2 (zero egress fees)
- **Search**: Meilisearch (Rust, MIT, sub-50ms)
- **Payment**: Stripe Connect (Express accounts)
- **Client downloads**: reqwest + trauma crate
- **Local state**: SQLite for installed versions
- **Package format**: `.vibezpkg` (ZIP with manifest.toml + per-platform binaries)
- **Launch cost**: ~$65/month infrastructure

## Phased Rollout

**Phase 1 — Free Community Library** (launch with DAW)
- Browse/install free presets, samples, MIDI, templates
- One-click install from in-DAW browser
- Community ratings
- No payment system needed yet

**Phase 2 — Commercial Marketplace**
- Stripe Connect, 85/15 split, rent-to-own
- Developer portal + automated validation pipeline
- CLAP plugin support with malware scanning + crash testing
- Verified purchase reviews

**Phase 3 — Creator Ecosystem**
- Creator profiles, follows, wishlists
- All-access subscription tier
- AI model weights marketplace
- "Made with Vibez" project showcase

## What Makes This Different

Vibez marketplace would be the only:
- In-DAW marketplace for an open-source DAW
- Trust-based licensing (no DRM) in a commercial marketplace
- Platform selling AI model weights alongside traditional plugins
- Combined free community library + paid marketplace (Godot model)
- 85/15 split that's competitive with the best in the industry
