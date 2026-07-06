# Vibez Marketplace Architecture Research

## Table of Contents
1. [Backend Architecture](#1-backend-architecture)
2. [Client-Side Integration](#2-client-side-integration)
3. [Payment and Licensing](#3-payment-and-licensing)
4. [Developer Experience for Sellers](#4-developer-experience-for-sellers)
5. [Community Features](#5-community-features)
6. [Content Types](#6-content-types)
7. [Open-Source Precedents](#7-open-source-precedents)
8. [Moderation and Trust](#8-moderation-and-trust)
9. [Recommended Architecture for Vibez](#9-recommended-architecture-for-vibez)

---

## 1. Backend Architecture

### API Design: REST vs GraphQL

**Recommendation: REST with JSON, with optional GraphQL later.**

Every major marketplace precedent uses REST:
- crates.io: REST JSON API served via axum
- Godot Asset Library: REST API (`GET /asset`, `GET /asset/{id}`, `GET /configure`)
- npm registry: REST (`GET /<package>`, `GET /<package>/<version>`, `PUT /<package>`)
- Blender Extensions: REST API with JSON listing files
- VS Code Marketplace: REST API consumed by editors

REST is simpler to cache at CDN edge, easier for the desktop client to consume (no GraphQL client needed in Rust), and well-understood. GraphQL would add complexity without clear benefit for a marketplace where queries are predictable.

**Proposed API structure:**

```
# Discovery
GET  /api/v1/catalog                    # paginated, filterable listing
GET  /api/v1/catalog/{id}               # single product detail
GET  /api/v1/catalog/search?q=...       # full-text search
GET  /api/v1/catalog/categories         # category tree
GET  /api/v1/catalog/featured           # curated/featured items

# Downloads
GET  /api/v1/downloads/{id}/latest      # latest version download URL (signed)
GET  /api/v1/downloads/{id}/{version}   # specific version download URL (signed)

# User
POST /api/v1/auth/login                 # OAuth flow initiation
GET  /api/v1/user/library               # purchased/installed items
GET  /api/v1/user/licenses              # license keys
POST /api/v1/user/reviews               # submit review

# Publisher
POST /api/v1/publish/upload             # upload new version
GET  /api/v1/publish/dashboard          # sales analytics
PUT  /api/v1/publish/{id}/metadata      # update listing

# Payments
POST /api/v1/checkout/session           # create Stripe checkout
POST /api/v1/webhooks/stripe            # Stripe webhook receiver
```

### Asset Hosting and CDN

**Recommendation: Cloudflare R2 for storage + Cloudflare CDN for delivery.**

| Provider | Storage Cost | Egress Cost | Notes |
|----------|-------------|-------------|-------|
| **Cloudflare R2** | $0.015/GB/mo | **$0 (free)** | Zero egress is the killer feature |
| AWS S3 | $0.023/GB/mo | $0.09/GB | Egress costs explode with large downloads |
| Bunny Storage + CDN | $0.01/GB storage | $0.01/GB delivery | Good but more complex setup |

For a marketplace distributing multi-MB plugin binaries and multi-GB sample packs, egress costs dominate. R2's zero egress model is ideal. Architecture:

```
Upload Flow:
  Publisher → API Server → Virus Scan → R2 Bucket (private)
                                          ↓
                                    CDN Worker generates signed URL
                                          ↓
Download Flow:
  Client → Signed URL → R2 via Cloudflare CDN → Client
```

Use **signed URLs** with expiration (e.g., 1 hour) for paid content. Free content can be served directly via public R2 bucket with CDN caching.

For sample packs (potentially 1-10GB), consider chunked uploads via the TUS protocol or multipart upload to R2 directly from the client with pre-signed upload URLs.

### Search and Discovery

**Recommendation: Meilisearch (self-hosted) or Typesense.**

| Engine | Language | License | Self-Host | Cloud | Latency |
|--------|----------|---------|-----------|-------|---------|
| **Meilisearch** | Rust | MIT | Yes | Yes | <50ms |
| **Typesense** | C++ | GPL-3 | Yes | Yes | <50ms |
| Algolia | Proprietary | SaaS only | No | Yes | <50ms |

Meilisearch is the natural fit for a Rust project:
- Written in Rust, MIT licensed
- Sub-50ms typo-tolerant search out of the box
- Faceted search (filter by category, format, price range, rating)
- Synonym support (e.g., "compressor" = "dynamics processor")
- Simple HTTP API, trivial to integrate
- Can self-host on a single VPS alongside the API server early on

Index structure for the catalog:
```json
{
  "id": "pkg_abc123",
  "name": "Warm Tape Saturator",
  "description": "Analog-modeled tape saturation...",
  "author": "StudioDev",
  "category": "effects",
  "subcategory": "distortion",
  "tags": ["saturation", "tape", "analog", "warming"],
  "formats": ["clap", "vst3"],
  "platforms": ["linux", "windows", "macos"],
  "price_cents": 2900,
  "is_free": false,
  "rating": 4.7,
  "downloads": 1523,
  "created_at": 1709251200,
  "updated_at": 1709337600
}
```

### Database Schema (PostgreSQL)

```sql
-- Users (both buyers and sellers)
CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT UNIQUE NOT NULL,
    display_name TEXT NOT NULL,
    avatar_url TEXT,
    bio TEXT,
    stripe_customer_id TEXT,          -- Stripe customer for buying
    stripe_connect_id TEXT,           -- Stripe Connect account for selling
    is_verified_seller BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Products (plugins, presets, samples, etc.)
CREATE TABLE products (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    seller_id UUID REFERENCES users(id) NOT NULL,
    slug TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    short_description TEXT,
    category product_category NOT NULL,  -- enum
    subcategory TEXT,
    price_cents INTEGER NOT NULL DEFAULT 0,  -- 0 = free
    currency TEXT DEFAULT 'usd',
    is_published BOOLEAN DEFAULT FALSE,
    is_approved BOOLEAN DEFAULT FALSE,      -- moderation gate
    license_type license_type NOT NULL,     -- free, commercial, pay-what-you-want
    content_type content_type NOT NULL,     -- plugin, preset, sample, midi, template, theme, ai_model
    tags TEXT[] DEFAULT '{}',
    icon_url TEXT,
    banner_url TEXT,
    download_count INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

-- Product versions (each upload is a version)
CREATE TABLE product_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID REFERENCES products(id) NOT NULL,
    version TEXT NOT NULL,                   -- semver: "1.2.3"
    changelog TEXT,
    min_app_version TEXT,                    -- minimum Vibez version
    platforms TEXT[] DEFAULT '{}',           -- ["linux", "windows", "macos"]
    formats TEXT[] DEFAULT '{}',            -- ["clap", "vst3", "lv2"]
    file_size_bytes BIGINT NOT NULL,
    sha256_hash TEXT NOT NULL,              -- integrity check
    r2_object_key TEXT NOT NULL,            -- storage path
    is_yanked BOOLEAN DEFAULT FALSE,        -- soft-delete a version
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(product_id, version)
);

-- Screenshots / previews
CREATE TABLE product_media (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID REFERENCES products(id) NOT NULL,
    media_type TEXT NOT NULL,                -- "screenshot", "audio_preview", "video"
    url TEXT NOT NULL,
    sort_order INTEGER DEFAULT 0,
    caption TEXT
);

-- Purchases / licenses
CREATE TABLE purchases (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    buyer_id UUID REFERENCES users(id) NOT NULL,
    product_id UUID REFERENCES products(id) NOT NULL,
    product_version_id UUID REFERENCES product_versions(id),
    stripe_payment_intent_id TEXT,
    amount_cents INTEGER NOT NULL,
    platform_fee_cents INTEGER NOT NULL,     -- Vibez's cut
    seller_payout_cents INTEGER NOT NULL,    -- seller's cut
    status TEXT DEFAULT 'completed',         -- pending, completed, refunded
    created_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(buyer_id, product_id)             -- one purchase per product
);

-- License keys (for offline validation)
CREATE TABLE licenses (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    purchase_id UUID REFERENCES purchases(id) NOT NULL,
    user_id UUID REFERENCES users(id) NOT NULL,
    product_id UUID REFERENCES products(id) NOT NULL,
    license_key TEXT UNIQUE NOT NULL,
    machine_ids TEXT[] DEFAULT '{}',          -- activated machines
    max_activations INTEGER DEFAULT 3,
    is_active BOOLEAN DEFAULT TRUE,
    expires_at TIMESTAMPTZ,                  -- NULL = perpetual
    created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Reviews
CREATE TABLE reviews (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID REFERENCES products(id) NOT NULL,
    user_id UUID REFERENCES users(id) NOT NULL,
    rating INTEGER CHECK (rating >= 1 AND rating <= 5),
    title TEXT,
    body TEXT,
    is_verified_purchase BOOLEAN DEFAULT FALSE,
    helpful_count INTEGER DEFAULT 0,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    UNIQUE(product_id, user_id)              -- one review per user per product
);

-- Wishlists
CREATE TABLE wishlists (
    user_id UUID REFERENCES users(id),
    product_id UUID REFERENCES products(id),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (user_id, product_id)
);

-- Follows (user follows creator)
CREATE TABLE follows (
    follower_id UUID REFERENCES users(id),
    following_id UUID REFERENCES users(id),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (follower_id, following_id)
);

-- Enums
CREATE TYPE product_category AS ENUM (
    'instruments', 'effects', 'presets', 'samples',
    'midi', 'templates', 'themes', 'ai_models'
);
CREATE TYPE content_type AS ENUM (
    'plugin', 'preset_pack', 'sample_pack', 'midi_pack',
    'project_template', 'theme', 'ai_model'
);
CREATE TYPE license_type AS ENUM (
    'free', 'commercial', 'pay_what_you_want', 'subscription'
);
```

### Backend Tech Stack Recommendation

Since Vibez is a Rust project, building the backend in Rust makes sense for ecosystem consistency:

```
Framework:     axum (same as crates.io)
Database:      PostgreSQL + diesel-async (same as crates.io)
Search:        Meilisearch
Storage:       Cloudflare R2
CDN:           Cloudflare
Auth:          JWT + OAuth (GitHub, Google, email/password)
Background:    Custom worker (like crates.io) or Tokio tasks
Payments:      Stripe Connect
Cache:         Redis (session cache, rate limiting)
```

---

## 2. Client-Side Integration

### How Other Apps Handle In-App Marketplaces

**VS Code Marketplace:**
- Fully native UI integrated into the sidebar
- Search, filter by category, sort by installs/rating
- One-click install, auto-update
- Extension API is well-designed: editor consumes a REST API from Azure DevOps backend
- Alternative open-source implementations: [Coder's code-marketplace](https://github.com/coder/code-marketplace), [Open VSX](https://open-vsx.org/)

**Unity Asset Store:**
- Hybrid approach: uses a webview window for browsing, but native UI for the package manager
- Publisher portal is a web app
- Review process takes ~10 business days for new submissions, ~2 days for updates
- Provides [Asset Store Tools](https://github.com/Unity-Technologies/com.unity.asset-store-tools) package for validation

**Blender Extensions:**
- Fully native UI in Blender Preferences panel
- Replaced the old "Add-ons" section with "Extensions"
- Install, enable, update directly from the native UI
- Remote repositories (points to extensions.blender.org by default)
- Users can add custom repository URLs
- Authentication via Blender ID

### Webview vs Native UI Decision

**For Vibez: Hybrid approach — native UI for browsing/installing, webview for checkout/auth.**

| Approach | Pros | Cons |
|----------|------|------|
| **Fully Native (iced)** | Consistent look, no web dependency, fast | Harder to iterate on layout, can't reuse web store |
| **Fully Webview** | Easy to update, reuse website, rich layouts | Inconsistent look, memory overhead, security surface |
| **Hybrid** | Best of both worlds | Slight complexity |

Reasoning:
- The **browse/search/install** flow should be native iced UI for consistency with the rest of the DAW and performance
- The **checkout/payment** flow should use a webview or external browser redirect to Stripe Checkout (required by Stripe for PCI compliance anyway)
- The **auth/login** flow can use OAuth redirect to external browser, then deep-link back to the app

If a webview is needed, Rust has **wry** (from the Tauri team):
- Cross-platform: WebView2 on Windows, WebKit on macOS, WebKitGTK on Linux
- Single dependency, lightweight
- Can communicate between Rust and JS via IPC

### Download Manager

Use **reqwest** with HTTP Range header support for pause/resume:

```rust
// Conceptual download manager design
struct DownloadManager {
    active_downloads: HashMap<ProductId, DownloadTask>,
    db: SqlitePool,  // local DB for tracking download state
}

struct DownloadTask {
    product_id: ProductId,
    version: String,
    url: String,
    total_bytes: u64,
    downloaded_bytes: Arc<AtomicU64>,
    temp_path: PathBuf,
    final_path: PathBuf,
    status: DownloadStatus,  // Queued, Downloading, Paused, Completed, Failed
    cancel_token: CancellationToken,
}

impl DownloadManager {
    async fn start_download(&mut self, product: &Product, url: &str) -> Result<()> {
        let client = reqwest::Client::new();

        // Resume support: check if partial file exists
        let existing_bytes = if self.temp_path.exists() {
            std::fs::metadata(&self.temp_path)?.len()
        } else {
            0
        };

        let mut request = client.get(url);
        if existing_bytes > 0 {
            request = request.header("Range", format!("bytes={}-", existing_bytes));
        }

        let response = request.send().await?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.temp_path)?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk)?;
            self.downloaded_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);

            // Check for pause/cancel
            if self.cancel_token.is_cancelled() {
                return Ok(());
            }
        }

        // Verify SHA-256
        verify_sha256(&self.temp_path, &product.sha256)?;

        // Move to final location
        std::fs::rename(&self.temp_path, &self.final_path)?;
        Ok(())
    }
}
```

Key Rust crates for this:
- **reqwest**: HTTP client with streaming support
- **sha2**: SHA-256 verification
- **tokio**: async runtime
- **tokio-util** (CancellationToken): pause/cancel support
- **trauma**: Tokio-based async download manager library with progress bars and resume

### Plugin Installation and Path Management

Standard plugin paths per platform:

```
# CLAP
Linux:   ~/.clap/
macOS:   ~/Library/Audio/Plug-Ins/CLAP/
Windows: C:\Program Files\Common Files\CLAP\

# VST3
Linux:   ~/.vst3/
macOS:   ~/Library/Audio/Plug-Ins/VST3/
Windows: C:\Program Files\Common Files\VST3\

# LV2
Linux:   ~/.lv2/

# Sample Packs / Presets (Vibez-managed)
All:     ~/.vibez/marketplace/samples/
All:     ~/.vibez/marketplace/presets/
All:     ~/.vibez/marketplace/midi/
All:     ~/.vibez/marketplace/templates/
All:     ~/.vibez/marketplace/themes/
```

The installation process:
1. Download to temp directory
2. Verify SHA-256 hash
3. For plugins: extract to appropriate system plugin path
4. For content packs: extract to Vibez-managed content directory
5. Update local SQLite database with installed version
6. Trigger plugin rescan in the DAW

### Auto-Updates / Version Management

```rust
// On app startup or periodic check
async fn check_for_updates(installed: &[InstalledProduct]) -> Vec<UpdateAvailable> {
    let ids: Vec<&str> = installed.iter().map(|p| p.product_id.as_str()).collect();
    let response = api_client.post("/api/v1/catalog/check-updates")
        .json(&CheckUpdatesRequest { products: ids })
        .send().await?;

    let latest_versions: HashMap<String, VersionInfo> = response.json().await?;

    installed.iter().filter_map(|p| {
        let latest = latest_versions.get(&p.product_id)?;
        if semver::Version::parse(&latest.version) > semver::Version::parse(&p.installed_version) {
            Some(UpdateAvailable {
                product_id: p.product_id.clone(),
                current: p.installed_version.clone(),
                latest: latest.version.clone(),
                changelog: latest.changelog.clone(),
            })
        } else {
            None
        }
    }).collect()
}
```

Batch the update check into a single API call to avoid N+1 requests. The server endpoint accepts a list of product IDs and returns the latest version for each.

---

## 3. Payment and Licensing

### Stripe Connect Marketplace Model

**Recommendation: Stripe Connect with Express accounts and Destination Charges.**

Architecture:
```
Buyer → Stripe Checkout → Payment Intent
                              ↓
                    Platform receives full amount
                              ↓
                    Automatic split:
                      - Platform fee (e.g., 15-20%)
                      - Seller payout (80-85%)
                              ↓
                    Seller's Stripe Express account
                    (daily rolling payouts to their bank)
```

**Why Express accounts:**
- Simplified onboarding for sellers (Stripe-hosted KYC)
- Platform retains control over branding
- Sellers get a Stripe dashboard for their payouts
- No need to build custom onboarding flows

**Destination Charges** (recommended):
```
POST /v1/payment_intents
{
  "amount": 2900,
  "currency": "usd",
  "transfer_data": {
    "destination": "acct_seller123",
    "amount": 2465  // seller gets 85%
  }
}
```

The platform automatically keeps the difference ($4.35 = 15% platform fee). Stripe's processing fee comes out of the platform's portion.

**Pricing:**
- Stripe fee: 2.9% + $0.30 per transaction
- Platform fee: 15-20% (industry standard: Unity takes 30%, Splice varies, Apple/Google take 30%)
- A 15% platform fee would be competitive and attract sellers

### Multi-Vendor Payouts

Stripe Connect handles this automatically with Express accounts:
- Funds accumulate in connected account Stripe balance
- Daily rolling payouts to seller's bank account (configurable: weekly, monthly)
- Instant payouts available (for additional fee)
- Available in 47+ countries, payouts to 118+ countries
- Handles 1099/tax reporting for US sellers

### Alternative Payment Processors

| Provider | Model | Fee | Marketplace Support | Tax Handling |
|----------|-------|-----|---------------------|--------------|
| **Stripe Connect** | Payment processor | 2.9% + $0.30 | Excellent | Via Stripe Tax |
| **Paddle** | Merchant of Record | 5% + $0.50 | Limited | Full (MoR) |
| **LemonSqueezy** | Merchant of Record | 5% + $0.50 | Limited | Full (MoR) |

**Paddle/LemonSqueezy** are Merchant of Record (MoR) platforms — they handle all tax compliance (VAT, GST, sales tax) and act as the legal seller. This simplifies tax but limits marketplace flexibility.

**Note:** Stripe acquired LemonSqueezy in July 2024. The future of LemonSqueezy as a standalone product is uncertain.

**Recommendation:** Start with Stripe Connect. If global tax compliance becomes burdensome, add Stripe Tax or consider a hybrid approach where Paddle handles international sales.

### License Key Generation and Offline Validation

**Critical design principle: The audio community HATES aggressive DRM. Offline use MUST work.**

**Recommended approach: RSA-signed license files.**

```
How it works:
1. Server has RSA private key
2. On purchase, server generates license payload:
   {
     "user_id": "usr_abc",
     "product_id": "pkg_xyz",
     "issued_at": "2026-03-02T00:00:00Z",
     "machine_limit": 3,
     "type": "perpetual"
   }
3. Server signs payload with RSA private key
4. Client receives: { payload, signature }
5. Plugin has embedded RSA public key
6. Plugin verifies signature locally — NO internet needed
7. License file stored at ~/.vibez/licenses/pkg_xyz.license
```

```rust
// Simplified license validation (runs offline)
use rsa::{RsaPublicKey, pkcs1v15::VerifyingKey};
use sha2::Sha256;

fn validate_license(license_file: &[u8], public_key: &RsaPublicKey) -> Result<LicensePayload> {
    let license: SignedLicense = serde_json::from_slice(license_file)?;
    let verifying_key = VerifyingKey::<Sha256>::new(public_key.clone());
    verifying_key.verify(
        license.payload.as_bytes(),
        &Signature::try_from(license.signature.as_slice())?
    )?;
    let payload: LicensePayload = serde_json::from_str(&license.payload)?;
    // Check expiration if applicable
    if let Some(expires) = payload.expires_at {
        if Utc::now() > expires {
            return Err(LicenseError::Expired);
        }
    }
    Ok(payload)
}
```

**Machine binding (optional, soft):**
- Generate a machine fingerprint from hardware IDs (CPU, MAC address, etc.)
- Allow 3 activations by default
- Deactivation available via web dashboard
- This is a trust-based approach — no phone-home required after initial activation

### DRM: What NOT To Do

**iLok / PACE Anti-Piracy:**
- Requires $50 USB dongle OR iLok Cloud (needs internet)
- Kernel-level driver on Windows/macOS
- Has caused system instability, crashes, and driver conflicts
- Universally despised by users
- Even Neural DSP choosing iLok was controversial in the community

**Why people hate aggressive DRM:**
- "I bought it, I should be able to use it" — offline studio setups are common
- USB dongles get lost or break
- Cloud validation fails when internet is down (during a session!)
- Kernel drivers are a security risk
- iLok account lockouts have cost people access to thousands of dollars of software

**What to do instead:**

1. **Simple license file** (FabFilter, u-he, ValhallaDSP, Xfer approach)
   - Serial number or license file, offline forever
   - Most beloved by users
   - u-he, FabFilter, ValhallaDSP are the most respected brands partly because of this

2. **One-time online activation** (like Plugin Alliance)
   - Needs internet once to activate
   - Works offline forever after
   - Acceptable to most users

3. **Trust-based / honor system** (Ardour model)
   - Ardour charges for pre-built binaries on Mac/Windows
   - Source code is free (GPL) — build it yourself
   - Pay-what-you-want for downloads
   - Works because of community goodwill

**Vibez recommended approach:**
- Free items: no license needed
- Paid items: RSA-signed license file, downloaded once after purchase
- 3 machine activations, deactivation via web dashboard
- No phone-home after initial activation
- No kernel drivers, no USB dongles
- Piracy will happen — focus on making the legitimate experience so easy that paying is the path of least resistance

### Splice Rent-to-Own Model (Worth Considering)

Splice popularized rent-to-own for plugins:
- Monthly payments until the full price is paid off
- Plugin is usable during payment period
- Cancel anytime, resume later
- Authorization check every ~3 days (needs internet periodically)
- Once fully paid: perpetual license, no more checks

This could work for expensive plugins ($100+) to lower the barrier to entry. Implementation: Stripe subscriptions with a target amount, converting to perpetual license when fully paid.

---

## 4. Developer Experience for Plugin Sellers

### Submission Process

```
1. Seller creates account → completes Stripe Connect onboarding
2. Uploads product:
   - Binary artifacts (per-platform .clap, .vst3, .lv2 files, or installer)
   - Metadata (name, description, category, tags, price)
   - Screenshots (min 3), audio preview, optional video
   - Icon (512x512 PNG)
   - README / documentation
3. Automated validation pipeline runs
4. Manual review by Vibez team (first submission) or auto-approve (trusted sellers)
5. Published to marketplace
```

### CI/CD Validation Pipeline

Automated checks on every submission:

```
Pipeline stages:
1. MALWARE SCAN
   - ClamAV scan of all uploaded binaries
   - VirusTotal API check (multiple AV engines)
   - Behavioral analysis in sandboxed environment

2. FORMAT VALIDATION
   - CLAP: load with clap-validator, verify descriptor
   - VST3: load with VST3 SDK validator
   - LV2: validate with lilv/lv2lint
   - Check that declared platforms match actual binaries

3. LOAD TEST
   - Instantiate plugin in headless host
   - Process 10 seconds of silence and noise
   - Check for crashes, memory leaks, excessive CPU
   - Validate audio output is finite (no NaN/Inf)

4. METADATA VALIDATION
   - Semver version format
   - Required fields present
   - Image dimensions/format correct
   - Description length within bounds
   - No prohibited content in description

5. CODE SIGNING CHECK
   - macOS: must be signed and notarized
   - Windows: Authenticode signature preferred (not required initially)
   - Linux: SHA-256 hash verification
```

### Multi-Platform Builds

Sellers are responsible for building for each platform they want to support. The marketplace should provide:

- Build templates (GitHub Actions workflows for CLAP/VST3 cross-compilation)
- Documentation on building for each platform
- A CI service that can build from source (future — like F-Droid does for Android apps)
- Platform badges on listings showing which platforms are supported

### Versioning and Updates

- Strict semver enforcement
- Changelog required for each version
- Minimum Vibez version compatibility field
- "Yanking" support (soft-delete a broken version, like crates.io)
- Users on older versions aren't force-updated — they see "update available"

### Revenue Dashboard

Seller portal provides:
- Real-time sales data (units, revenue, refunds)
- Geographic breakdown
- Download analytics (installs, updates, uninstalls)
- Review monitoring
- Payout history (powered by Stripe Connect dashboard)
- Conversion funnel (views → detail page → purchase)

---

## 5. Community Features

### Rating and Review System

**Preventing gaming:**

1. **Verified purchase badge** — only buyers who completed a transaction can leave "verified purchase" reviews. Non-purchasers can still comment but without the badge.

2. **Time-gating** — reviews can only be submitted after the product has been installed for at least 24 hours (prevent drive-by reviews).

3. **One review per user per product** — update, don't stack.

4. **Review score weighting:**
   - Verified purchases weighted 2x
   - Accounts older than 30 days weighted more
   - Reviews with helpful votes weighted more
   - Bayesian average (don't let 1 five-star review put a product at #1)

5. **Anomaly detection:**
   - Spike in reviews from new accounts → flag for manual review
   - Reviews from same IP range → flag
   - Copy-paste text detection

6. **Helpful/Not Helpful voting** — community self-moderates review quality

### User Profiles / Creator Pages

```
/creators/{username}
- Avatar, bio, links
- Published products grid
- Total downloads, average rating
- "Follow" button → notifications on new releases
- "Made with Vibez" showcase (optional)
```

### Additional Community Features

- **Wishlists** — save items for later, get notified on sales
- **Collections/Lists** — curated lists (e.g., "Best free reverbs", "Essential mastering chain")
- **Notifications** — new releases from followed creators, product updates, sale alerts
- **"Made with Vibez" sharing** — share project files or rendered audio with attribution to plugins used
- **Forums/Comments** — per-product discussion threads for support and feedback

---

## 6. Content Types to Support

### Tier 1 (Launch)

| Type | Format | Installation | Notes |
|------|--------|-------------|-------|
| **Audio plugins** | CLAP, VST3, LV2 | System plugin dirs | Platform-specific binaries |
| **Preset packs** | JSON/custom format | `~/.vibez/presets/` | For built-in instruments |
| **Sample packs** | WAV, FLAC, MP3 | `~/.vibez/samples/` | One-shots, loops, stems |

### Tier 2 (Post-Launch)

| Type | Format | Installation | Notes |
|------|--------|-------------|-------|
| **MIDI packs** | .mid files | `~/.vibez/midi/` | Drum patterns, melodies, chord progressions |
| **Project templates** | .vibez project | `~/.vibez/templates/` | Starter projects with routing, effects chains |
| **Themes/skins** | JSON + assets | `~/.vibez/themes/` | Custom color schemes, layouts |

### Tier 3 (Future)

| Type | Format | Installation | Notes |
|------|--------|-------------|-------|
| **AI model weights** | ONNX, safetensors | `~/.vibez/models/` | Fine-tuned models for AI generation features |
| **Third-party presets** | Vendor-specific | Varies | Presets for third-party VST/CLAP plugins |

### Content Packaging

All marketplace items should be distributed as a `.vibezpkg` archive:

```
warm-tape-saturator-1.2.3.vibezpkg
├── manifest.toml          # metadata, dependencies, install instructions
├── LICENSE
├── README.md
├── linux-x86_64/
│   └── WarmTapeSaturator.clap
├── macos-universal/
│   └── WarmTapeSaturator.clap
├── windows-x86_64/
│   └── WarmTapeSaturator.clap
└── presets/
    └── factory/
        ├── Gentle Warmth.json
        └── Heavy Tape.json
```

```toml
# manifest.toml
[package]
name = "warm-tape-saturator"
version = "1.2.3"
description = "Analog-modeled tape saturation effect"
author = "StudioDev"
license = "commercial"
category = "effects"
subcategory = "distortion"
min_vibez_version = "0.1.0"

[content]
type = "plugin"
formats = ["clap"]
platforms = ["linux-x86_64", "macos-universal", "windows-x86_64"]

[install]
plugin_dir = "auto"  # use standard system paths
preset_dir = "presets/warm-tape-saturator/"
```

---

## 7. Open-Source Precedents

### Blender Extensions Platform

**Source:** [projects.blender.org/infrastructure/extensions-website](https://projects.blender.org/infrastructure/extensions-website)

- **Stack:** Django/Python, GPLv3 licensed
- **Inspired by:** Mozilla Addons Server
- **Auth:** Blender ID (SSO across all Blender services)
- **Repository protocol:** JSON listing file + download URLs
- **Key feature:** Users can add custom repository URLs (federated model)
- **In-app UI:** Native Blender Preferences panel, replaced "Add-ons" tab
- **Versioning:** Multiple versions targeting different Blender releases
- **Moderation:** Reviewed before listing on official repository

**Lessons for Vibez:**
- The federated repository model is excellent — allow third-party repos
- Native UI integration is the right call (not webview)
- Blender ID-style SSO is good for ecosystem coherence

### Godot Asset Library

**Source:** [github.com/godotengine/godot-asset-library](https://github.com/godotengine/godot-asset-library)

- **Stack:** PHP backend (now in maintenance mode, being replaced)
- **API:** REST, documented at [API.md](https://github.com/godotengine/godot-asset-library/blob/master/API.md)
- **Key endpoints:**
  - `GET /configure` — categories, login URL
  - `GET /asset?filter=...&category=...&godot_version=...` — search
  - `GET /asset/{id}` — detail with download URL + SHA-256 hash
- **In-app UI:** Fully native, embedded in Godot editor
- **Moderation:** Support levels (official, community, testing)
- **Integrity:** SHA-256 hash on every download for verification

**Lessons for Vibez:**
- The `/configure` endpoint pattern is clever — client fetches config on startup
- Support levels (official/community/testing) is a good trust model
- SHA-256 per-download is essential
- Godot Foundation is replacing this with a proper "Asset Store" — the old PHP system didn't scale

### crates.io

**Source:** [github.com/rust-lang/crates.io](https://github.com/rust-lang/crates.io) — [ARCHITECTURE.md](https://github.com/rust-lang/crates.io/blob/main/docs/ARCHITECTURE.md)

- **Stack:** Rust (axum + diesel-async), PostgreSQL, Ember.js frontend
- **Storage:** S3 for crate tarballs
- **Index:** Git-based index (being replaced by sparse HTTP protocol)
- **Sparse protocol:** Individual HTTP requests for each crate's metadata, CDN-cacheable
- **Background workers:** Custom-built system (not off-the-shelf)
- **Recent:** Migrated to diesel-async, 10-15% perf boost on some endpoints

**Lessons for Vibez:**
- axum + diesel-async + PostgreSQL is a proven stack for Rust registries
- The sparse index protocol is smart for client-side caching
- S3-compatible storage (R2) for artifacts is standard
- The "yanking" concept (soft-delete a version) is important for safety
- Background workers needed for: search indexing, download counting, email notifications

### npm Registry

- **Original:** CouchDB-based (document store for metadata)
- **Current:** Microservices architecture — CouchDB is now just one part
- **Services:** Front door (routing/validation), Auth service, Validate-and-store service
- **API:** REST — `GET /<package>`, `PUT /<package>` for publish
- **Auth:** Bearer tokens, OAuth for write operations
- **Storage:** Separate storage for metadata (DB) and tarballs (CDN)

**Lessons for Vibez:**
- Separate metadata from binary storage early
- npm's evolution from monolith to microservices is a cautionary tale — start simple
- The simple REST API pattern (`GET /package`, `GET /package/version`) scales well

### F-Droid

- **Model:** Decentralized repository system (anyone can host a repo)
- **Build:** F-Droid builds apps from source on their servers (trust model)
- **Signing:** F-Droid signs all APKs with its own key (or reproducible builds with developer key)
- **Client:** Resilient to censorship — works with Tor, local Wi-Fi distribution
- **Modeled after:** Debian package repositories

**Lessons for Vibez:**
- The "build from source on our servers" model is the gold standard for trust, but expensive
- Repository federation is powerful — let power users host private repos
- Code signing at the platform level provides a trust anchor

---

## 8. Moderation and Trust

### Malware Scanning

Multi-layer approach:

```
Layer 1: Static Analysis
  - ClamAV scan (open source, free, local)
  - VirusTotal API (60+ AV engines, $0 for low volume)
  - Custom YARA rules for known malicious patterns

Layer 2: Dynamic Analysis (Sandboxed Execution)
  - Spin up ephemeral VM/container
  - Load plugin in headless CLAP/VST3 host
  - Monitor: file system access, network calls, process spawning
  - Flag anything that: phones home to unexpected domains, reads files outside plugin dir,
    spawns child processes, modifies system files
  - Use Linux namespaces / seccomp for sandboxing

Layer 3: Manual Review
  - First submission from any seller always gets human review
  - Trusted sellers (5+ approved submissions) get fast-tracked
  - Any community reports trigger re-review
```

### Sandboxing Considerations

For installed plugins running in Vibez:
- CLAP plugins already run in-process (hard to sandbox without performance cost)
- Future: out-of-process plugin hosting (each plugin in its own process)
- Linux: seccomp-bpf, namespaces, Landlock LSM
- macOS: App Sandbox, but plugins need audio device access
- Windows: AppContainer, but VST3 plugins often need registry access
- **Practical reality:** Full sandboxing of audio plugins is unsolved industry-wide. Even major DAWs run plugins in-process. Focus on vetting at upload time rather than runtime sandboxing.

### Code Signing

```
Platform signing hierarchy:

1. Vibez Platform Key (root of trust)
   - Signs the marketplace index/manifest
   - Client verifies index hasn't been tampered with

2. Seller Signing (optional, encouraged)
   - macOS: Notarized with Apple Developer ID (required for Gatekeeper)
   - Windows: Authenticode signed (reduces SmartScreen warnings)
   - Linux: GPG signature

3. Package Integrity
   - SHA-256 hash of every artifact stored in the index
   - Client verifies hash after download, before installation
   - Any mismatch = reject + alert
```

### DMCA / Copyright for Sample Packs

This is a significant legal concern. Sample packs can contain:
- Uncleared samples from copyrighted recordings
- Loops that infringe on existing compositions
- Sounds ripped from commercial sample libraries

**Mitigation:**
1. Require sellers to certify originality in submission agreement
2. Accept DMCA takedown notices via `dmca@vibez.dev`
3. Implement a "3 strikes" policy (3 valid DMCA claims = permanent ban)
4. Use audio fingerprinting (Audd.io API, or self-hosted chromaprint/dejavu) to detect known copyrighted material
5. Community reporting with "Report Copyright Issue" button on every listing
6. Hold seller payouts for 7 days after first upload (time for claims to come in)

### Community Reporting

```
Report reasons:
- Copyright infringement
- Malware / security concern
- Misleading description
- Broken / doesn't work
- Spam / duplicate listing
- Inappropriate content

Report flow:
1. User clicks "Report" → selects reason → optional details
2. Report queued for review
3. Auto-actions:
   - 3+ reports from different users → temporarily hide listing
   - Reports from users with high trust scores → prioritized
4. Moderator reviews → dismiss, warn seller, remove listing, or ban seller
```

---

## 9. Recommended Architecture for Vibez

### Phase 1: MVP Marketplace (v0.1)

Focus on free content only to build the ecosystem:

```
Backend:
  - axum REST API
  - PostgreSQL (products, users, versions)
  - Meilisearch (search)
  - Cloudflare R2 (storage)
  - GitHub OAuth for auth

Client (in Vibez):
  - New "Store" tab in sidebar
  - Browse/search with native iced UI
  - One-click install for free content
  - Download progress indicator
  - Installed items management

Content types:
  - Preset packs (for built-in instruments)
  - Sample packs
  - MIDI packs
  - Themes

No payments, no licensing — everything is free.
```

### Phase 2: Paid Marketplace (v0.2)

Add commerce:

```
Backend additions:
  - Stripe Connect integration
  - License generation (RSA-signed)
  - Seller dashboard
  - Review system
  - Automated validation pipeline

Client additions:
  - "My Library" showing purchases
  - License management
  - Update checker
  - Review submission
  - Webview/browser redirect for Stripe Checkout

Content types added:
  - Audio plugins (CLAP, VST3)
  - Project templates
```

### Phase 3: Community Marketplace (v0.3)

Full ecosystem:

```
Backend additions:
  - Wishlists, follows, notifications
  - Creator pages
  - Collections/curated lists
  - Advanced analytics for sellers
  - Rent-to-own subscription option

Client additions:
  - Notification center
  - Creator profiles
  - "Made with Vibez" sharing

Content types added:
  - AI model weights
  - Third-party presets
```

### Infrastructure Costs Estimate (Early Stage)

```
Cloudflare R2:       ~$15/mo (1TB storage, zero egress)
PostgreSQL (RDS):    ~$30/mo (db.t4g.micro)
Meilisearch:         ~$0 (self-hosted on same VPS)
VPS (API + workers): ~$20/mo (Hetzner CX31)
Domain + SSL:        ~$15/year (Cloudflare handles SSL free)
VirusTotal API:      ~$0 (free tier: 500 lookups/day)

Total: ~$65/month to start
```

This scales well because R2 has zero egress — even at 100TB of downloads per month, storage costs only increase with catalog size, not download volume.

---

## Key Sources

- [Stripe Connect Documentation](https://docs.stripe.com/connect)
- [Stripe Connect Marketplace Charges](https://docs.stripe.com/connect/charges)
- [crates.io Source Code](https://github.com/rust-lang/crates.io) and [ARCHITECTURE.md](https://github.com/rust-lang/crates.io/blob/main/docs/ARCHITECTURE.md)
- [Godot Asset Library API](https://github.com/godotengine/godot-asset-library/blob/master/API.md)
- [Blender Extensions Platform Source](https://projects.blender.org/infrastructure/extensions-website)
- [Blender Extensions Platform Blog Post](https://code.blender.org/2022/10/blender-extensions-platform/)
- [Blender Extensions Beta Release](https://code.blender.org/2024/05/extensions-platform-beta-release/)
- [Coder code-marketplace](https://github.com/coder/code-marketplace)
- [Open VSX Registry](https://open-vsx.org/)
- [F-Droid Documentation](https://f-droid.org/en/docs/)
- [Meilisearch vs Typesense vs Algolia](https://www.meilisearch.com/blog/algolia-vs-typesense)
- [Typesense Comparison](https://typesense.org/typesense-vs-algolia-vs-elasticsearch-vs-meilisearch/)
- [Cloudflare R2 vs S3](https://news.ycombinator.com/item?id=42256771)
- [Wry WebView Library](https://github.com/tauri-apps/wry)
- [Trauma Download Manager](https://crates.io/crates/trauma)
- [iLok Love-Hate Relationship](https://www.production-expert.com/production-expert-1/the-love-hate-relationship-with-ilok-and-pace-in-the-music-and-post-production-world)
- [VST Plugin Licensing Guide](https://blog.musehub.com/vst-plugins-licensing-protection/)
- [Ardour Business Model](https://ardour.org/faq.html)
- [Ardour FOSS Model Analysis](https://tylerdavis.xyz/linux/ardour-provides-a-good-model-for-profiting-off-of-foss/)
- [Splice Rent-to-Own](https://support.splice.com/en/articles/8652687-about-rent-to-own)
- [MuseHub Plugin Distribution](https://developer.musehub.com/muse-partners-help/adding-products/product-types/plugins)
- [Unity Asset Store Submission Guidelines](https://assetstore.unity.com/publishing/submission-guidelines)
- [Unity Asset Store Tools](https://github.com/Unity-Technologies/com.unity.asset-store-tools)
- [CLAP Plugin Format](https://github.com/free-audio/clap)
- [npm Registry Architecture](https://blog.npmjs.org/post/75707294465/new-npm-registry-architecture.html)
- [Cargo Registry Index](https://doc.rust-lang.org/cargo/reference/registry-index.html)
- [Sparse Registry RFC](https://rust-lang.github.io/rfcs/2789-sparse-index.html)
- [Paddle vs LemonSqueezy](https://www.paddle.com/compare/lemon-squeezy)
- [Stripe Marketplace Blog](https://stripe.dev/blog/stripe-marketplaces-mapping-commercial-relationships-code)
