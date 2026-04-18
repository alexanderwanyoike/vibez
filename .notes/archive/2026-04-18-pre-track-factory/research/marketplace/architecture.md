# Plugin Marketplace: Technical Architecture

## Backend Stack

**axum + diesel-async + PostgreSQL** — same proven stack as crates.io.

### API: REST with JSON
Every major precedent (crates.io, Godot, npm, Blender, VS Code) uses REST. Simpler to cache at CDN edges, no GraphQL library needed in the Rust client.

### Core Endpoints
```
GET  /catalog?q=&category=&sort=&page=     # Browse/search
GET  /catalog/{id}                          # Product detail
GET  /catalog/{id}/versions                 # Version history
POST /catalog/{id}/download                 # Signed download URL
GET  /library                               # User's purchased items
POST /publish                               # Upload new product
POST /checkout                              # Stripe Checkout session
POST /webhook/stripe                        # Payment webhooks
GET  /reviews/{product_id}                  # Reviews
POST /reviews/{product_id}                  # Submit review
```

### Storage: Cloudflare R2
- **Zero egress fees** — critical for distributing multi-MB plugins and GB sample packs
- S3 would cost $0.09/GB egress — unsustainable at scale
- R2: $0.015/GB/month storage, unlimited free downloads
- Upload → API server (virus scan) → private R2 bucket → signed URLs for downloads

### Search: Meilisearch
- Written in Rust, MIT licensed
- Sub-50ms typo-tolerant search with faceted filtering
- Self-hostable on same VPS early on
- Categories, tags, format, price range as facets

### Database Schema (PostgreSQL)
```
users           — buyer + seller in one table, Stripe customer_id + connect_id
products        — category enum, content_type, price, seller_id
product_versions — semver, per-platform artifact URLs, SHA-256 hashes
purchases       — buyer_id, product_id, payment_id, platform_fee_split
licenses        — RSA-signed key, machine_activation_limit (default 3)
reviews         — verified_purchase flag, one per user per product, bayesian avg
```

### Cost Estimate (Launch)
| Component | Monthly |
|---|---|
| Cloudflare R2 | ~$15 |
| PostgreSQL (managed) | ~$30 |
| VPS (API + Meilisearch) | ~$20 |
| **Total** | **~$65/month** |

Scales well: R2 zero egress means downloads don't increase costs, only catalog size.

---

## Client-Side (In-DAW)

### Native UI for browse/search/install
- Consistent with rest of Vibez (iced)
- Search bar, category filters, grid/list view
- Plugin detail view with screenshots, audio previews, reviews
- One-click install with progress bar

### Payment via Stripe Checkout
- Redirect to Stripe (external browser or `wry` webview)
- Required for PCI compliance — never handle card data
- OAuth for account auth (redirect to browser with callback)

### Download Manager
- `reqwest` with HTTP Range headers for pause/resume
- `trauma` crate: Tokio-based download manager with progress + resume
- SHA-256 verification after download
- Install to standard paths:
  - Linux: `~/.clap/`, `~/.vst3/`
  - macOS: `~/Library/Audio/Plug-Ins/CLAP/`
  - Windows: `C:\Program Files\Common Files\CLAP\`
- Vibez-managed content: `~/.vibez/marketplace/`

### Local State
- SQLite database tracking installed versions
- Periodic update checks against API
- Notification badges for available updates

---

## Payment & Licensing

### Stripe Connect (Express Accounts + Destination Charges)
- Platform receives full payment
- Automatic split: 85% seller / 15% platform
- Stripe handles seller KYC, payouts (118+ countries), tax reporting
- Express accounts give sellers their own Stripe dashboard
- No custom payout infrastructure needed

### Rent-to-Own
- Stripe subscription: monthly payments accumulating toward full price
- Perpetual license issued when fully paid
- Pause/cancel anytime, resume later
- Track accumulated payments in database

### License Validation: RSA-Signed, Offline
```
On purchase:
  Server signs JSON payload with RSA private key:
  { user_id, product_id, version, machines: 3, issued_at }

On plugin load:
  Client verifies signature with embedded public key
  No internet required after initial download

Machine activation:
  Hash of hardware IDs → stored in license
  Deactivate via web dashboard
```

Same model as FabFilter, u-he, ValhallaDSP, Xfer.

---

## Developer Experience (Sellers)

### Submission Flow
1. Stripe Connect onboarding (one-time)
2. Upload per-platform binaries (Linux/macOS/Windows)
3. Fill metadata: name, description, category, tags, price
4. Upload screenshots + audio preview
5. Automated validation pipeline runs
6. Manual review for first-time sellers
7. Published to marketplace

### Validation Pipeline
1. **ClamAV** malware scan
2. **VirusTotal** check
3. **Format validation**: load plugin in headless host
4. **Crash test**: process silence and noise, check stability
5. **Metadata validation**: required fields, image sizes

### Package Format: `.vibezpkg`
```
myplugin-1.0.0.vibezpkg (ZIP archive)
├── manifest.toml
│   ├── name, version, description, category
│   ├── author, license, homepage
│   ├── min_vibez_version
│   ├── content_type: "clap_plugin" | "preset_pack" | "sample_pack" | ...
│   └── platforms: [linux-x86_64, macos-aarch64, windows-x86_64]
├── linux-x86_64/
│   └── myplugin.clap
├── macos-aarch64/
│   └── myplugin.clap
├── windows-x86_64/
│   └── myplugin.clap
├── presets/          (optional)
├── screenshots/
└── preview.mp3       (audio demo)
```

### GitHub Actions Templates
Provide CI templates for sellers to build multi-platform CLAP plugins and auto-package into `.vibezpkg`.

---

## Content Types

### Launch (Phase 1-2)
| Type | Complexity | Notes |
|---|---|---|
| Preset packs | Low | JSON/binary files, easiest to distribute |
| Sample packs | Low | WAV/FLAC files, largest by size |
| MIDI packs | Low | .mid files, tiny |
| Project templates | Low | Vibez project files |

### Later (Phase 3)
| Type | Complexity | Notes |
|---|---|---|
| CLAP plugins | High | Needs validation pipeline |
| VST3 plugins | High | More complex packaging |
| AI model weights | Medium | LoRA fine-tunes, ONNX models |
| Themes/skins | Low | UI customization |

---

## Community Features

### Reviews
- Verified purchase badge
- 24-hour install time-gate before reviewing
- One review per user per product
- Bayesian average scoring
- Anomaly detection for review bombing

### Social
- Creator pages / profiles
- Follow creators → notifications on new releases
- Wishlists
- Curated collections ("Best free drum plugins", "Essential mixing tools")
- "Made with Vibez" project showcase

### Free Community Library
- Separate from paid marketplace
- No payment needed to browse/download
- Community-contributed presets, samples, MIDI
- GitHub-backed or self-hosted
- One-click install from within DAW

---

## Moderation & Trust

### Three-Layer Scanning
1. **Static**: ClamAV + VirusTotal on upload
2. **Dynamic**: Sandboxed plugin loading (headless host)
3. **Manual**: Review for first-time sellers

### DMCA / Copyright
- Seller certifies originality on upload
- Takedown process for claims
- Audio fingerprinting for sample packs (future)
- 3-strikes policy for repeat offenders

### Code Signing
- Platform signs approved packages
- Client verifies signature before install
- Similar to F-Droid's approach

---

## Phased Rollout

### Phase 1: Free Community Library
- In-DAW browser for free content
- Presets, samples, MIDI packs, templates
- One-click install
- No payment system needed
- Community ratings

### Phase 2: Commercial Marketplace
- Stripe Connect integration
- 85/15 revenue split
- Buy outright + rent-to-own
- Developer portal + submission pipeline
- CLAP plugin support with validation
- Reviews with verified purchase

### Phase 3: Creator Ecosystem
- Creator profiles and pages
- All-access subscription tier
- AI model weights marketplace
- "Made with Vibez" showcase
- Affiliate/referral program

---

## Key References
- [crates.io source (GitHub)](https://github.com/rust-lang/crates.io) — axum/diesel/PostgreSQL
- [Blender Extensions](https://extensions.blender.org/) — native in-app, federated repos
- [Godot Asset Library API](https://godotengine.org/asset-library/asset) — REST, support levels
- [Stripe Connect docs](https://stripe.com/docs/connect) — Express accounts, destination charges
- [Cloudflare R2 pricing](https://www.cloudflare.com/r2/) — zero egress
- [Meilisearch](https://www.meilisearch.com/) — Rust, MIT, sub-50ms search
- [CLAP format](https://cleveraudio.org/) — MIT, single-file, growing ecosystem
- [trauma crate](https://crates.io/crates/trauma) — Tokio download manager
