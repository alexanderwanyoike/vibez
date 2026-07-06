# Plugin Marketplace Research for Vibez DAW

**Date:** 2026-03-02
**Purpose:** Design an in-built plugin marketplace for an open-source Rust DAW where users can discover, buy, sell, and share plugins, presets, and samples.

---

## 1. Existing Plugin Marketplaces and Stores

### Plugin Boutique
- **Model:** Traditional e-commerce storefront for audio plugins (VST, AU, AAX)
- **Revenue split:** 60/40 (non-exclusive) or 70/30 (exclusive) in favor of the developer
- **Features:** Rent-to-own option, Plugin+ dealer program (37% margins), wishlists, sales/bundles, editorial content, "Virtual Cash" loyalty rewards
- **Developer onboarding:** Submit form, Plugin Boutique reviews and may onboard your brand. Developers get a Distributor portal to monitor sales, marketing assets, and reports
- **User experience:** Web-based storefront, separate from any DAW. Strong editorial curation, deal alerts, product reviews. Good discovery via categories and editorial picks
- **Pain point:** Not integrated into any DAW -- users must purchase externally, download installers, manage serial keys separately

### Splice Plugins (Rent-to-Own)
- **Model:** Rent-to-own with interest-free monthly payments. Pay monthly until you own the plugin outright. Can pause/cancel anytime, pay off early
- **Revenue split:** Not publicly disclosed. Splice likely takes 20-30% based on industry norms, but exact terms are under NDA with each developer
- **Catalog:** Curated selection -- Xfer Serum (exclusive), Spitfire Audio libraries (acquired by Splice in 2025), Arturia, Output, FabFilter, iZotope, etc.
- **User experience:** Desktop app manages downloads and authorization. Clean UI. Also integrates with DAWs for sample browsing (Ableton has a Splice label in its browser)
- **Strengths:** Lowers barrier to entry for expensive plugins. Very popular with bedroom producers. No upfront commitment
- **Weaknesses:** Limited catalog compared to Plugin Boutique. Monthly payments can add up. Once you cancel mid-plan, you lose access until you resume

### Plugin Alliance
- **Model:** Subscription (CORE and PRO tiers) + a la carte purchases. 200+ plugins from 40+ brands (Brainworx, SSL, Lindell, Shadow Hills, etc.)
- **Revenue:** Plugin Alliance was reported at ~$50M revenue. Developers join the alliance and share revenue
- **Subscription perks:** CORE subscribers earn 3 plugins/year to keep permanently; PRO earns 10 plugins/year permanently. Plus exclusive discounts and "PA Perks"
- **User experience:** Web store + Plugin Alliance Installation Manager (PAIM) desktop app for download/authorization
- **Strengths:** Enormous catalog. The "earn plugins to keep" model gives subscribers lasting value even if they cancel
- **Weaknesses:** Plugin manager app is another piece of software to maintain. Older UI. Some users complain about voucher system complexity

### Native Instruments (NI 360)
- **Model:** Tiered subscription replacing the old Komplete bundles. Essentials ($15/mo), Plus ($25/mo), Pro ($50/mo)
- **Catalog:** 50-130+ plugins spanning NI, iZotope, and Brainworx. 24,000+ loops and sounds
- **Management:** Native Access desktop app handles downloads, updates, and authorization
- **Cancellation:** Lose access to instruments/effects, but presets are preserved and audio passes through for 15 minutes on open (grace period)
- **Strengths:** Massive, high-quality catalog. Single app manages everything
- **Weaknesses:** Expensive at higher tiers. Subscription fatigue. Lose access entirely on cancel (no "keep" mechanism like Plugin Alliance)

### KVR Audio Marketplace
- **Model:** Developer-direct marketplace. Developers list products, set prices, handle fulfillment. Also has a forum-based Buy & Sell section for secondhand license transfers
- **Fees:** Not publicly detailed, but historically low-friction for developers. PayPal-based transactions
- **Strengths:** Massive database (30,000+ products listed). Community trust. Forum discussions around every product
- **Weaknesses:** Dated UI. No integrated download management. Discovery relies on search/forum activity rather than curation

### Knobcloud (License Resale)
- **Model:** Secondhand license marketplace. Buy/sell used plugin licenses
- **How it works:** Seller lists license, buyer pays via PayPal, developer transfers license to new owner. Developer must consent to transfer. Some developers charge a transfer fee
- **Limits:** 20 simultaneous offers, 5/day, 20/month per user
- **Significance:** Addresses a real gap -- producers accumulate plugins they don't use and want to recoup value. But it's cumbersome (developer must facilitate each transfer)

### MuseHub
- **Model:** Distribution platform for free and commercial apps, plugins, loops, and sounds. Parent company: Muse Group (also owns Audacity, MuseScore)
- **Developer features:** Handles distribution, payment processing, marketing. Developers can choose one-time payment, subscription, or donation pricing
- **Strengths:** Growing platform with free tier. Clean modern UI. Handles the full distribution pipeline
- **Weaknesses:** Relatively new. Smaller catalog than established players

---

## 2. In-DAW Plugin Stores

### FL Studio -- FL Cloud
- **Integration level:** Deeply integrated. FL Cloud is accessible directly from FL Studio's UI via the Tools menu
- **What it includes:** Sounds library (1M+ sounds), AI mastering, music distribution, and now plugins (69+ at launch from NI, UVI, Minimal Audio, Baby Audio, MeldaProduction, etc.)
- **Plugin management:** FL Cloud Plugins app (auto-installed with FL Studio 2025) handles install, authorization, and updates in one place. Plugins appear alongside stock FL plugins in the Plugin Picker
- **Tiers:** Free (10 instruments/effects), up to Pro (85+ instruments/effects)
- **UX strengths:** "Removes the stress and hassle of staying up to date with multiple plugin managers and passwords." Single sign-on. Seamless browsing and one-click install
- **UX weaknesses:** Locked to FL Studio ecosystem. FL Studio also launched a web version in late 2025
- **Key takeaway:** FL Cloud is the most aggressive in-DAW marketplace play. It bundles content, tools, distribution, and mastering into one subscription inside the DAW

### Ableton Live -- Packs Browser
- **Integration level:** Medium. Built-in browser has a "Packs" section showing installed/available Ableton Packs
- **How it works:** Packs section shows available downloads with one-click install. Installed packs appear in the browser hierarchy. Third-party integration via Splice label in the browser
- **Discovery:** Browse by Pack name, unfold to see presets/instruments/samples. No rating or review system
- **Strengths:** Clean, focused. Doesn't overwhelm with marketplace cruft. Splice integration is elegant
- **Weaknesses:** Limited to Ableton's own Packs ecosystem. No third-party plugin purchasing. No community marketplace

### Logic Pro -- Sound Library
- **Integration level:** Deep. Sound Library Manager built into Logic Pro
- **How it works:** Checkbox-based download manager. Select individual content packages or "Select All Uninstalled." Progress bar in LCD shows download status
- **Storage management:** Shows total/per-pack space usage. Can relocate library to external drive
- **Strengths:** Very Apple -- clean, integrated, "it just works" when it works
- **Weaknesses:** Only Apple's own content. Download issues are well-documented (stuck downloads, phantom installs). No third-party marketplace

### Studio One -- PreSonus Sphere / Studio One+
- **Integration level:** Medium-high. Subscription unlocks all PreSonus plugins and add-ons
- **Model:** Monthly/annual subscription (rebranded from "Sphere" to "Studio One+"). Includes Studio One, Notion, all PreSonus plugins, masterclasses, collaboration tools, community
- **Strengths:** All-inclusive approach. Community features (collaboration, learning)
- **Weaknesses:** Only PreSonus content. No third-party marketplace built in

### Bitwig Studio
- **Integration level:** Low for marketplace (no built-in store), but high for CLAP. Bitwig co-created CLAP with u-he
- **Plugin browser:** Built-in device browser scans for VST/CLAP plugins. No purchasing integration
- **Significance for Vibez:** Bitwig's championing of CLAP without building a marketplace represents a gap in the market

### Summary Table: In-DAW Stores

| DAW | Has Store? | Third-Party? | Plugin Mgmt | Content Types |
|-----|-----------|-------------|-------------|---------------|
| FL Studio | Yes (FL Cloud) | Yes (NI, Baby Audio, etc.) | Full (install/auth/update) | Plugins, sounds, mastering, distribution |
| Ableton | Partial (Packs) | Splice integration only | Download only | Packs (instruments, presets, samples) |
| Logic Pro | Partial (Sound Library) | No | Download/manage | Apple sound packs only |
| Studio One | Yes (Studio One+) | No (PreSonus only) | Full via subscription | Plugins, extensions, education |
| Bitwig | No | No | Scan only | N/A |
| Reaper | No | No | Scan only | N/A |

---

## 3. Indie/Community Plugin Distribution

### Current Distribution Channels for Small Developers
1. **Own website** -- Most common. Full control, but requires handling payments (Stripe/PayPal), licensing, download hosting, support
2. **Gumroad** -- Popular for low-friction selling. Handles payments, provides download links. 10% fee on free plan, less on paid plans
3. **itch.io** -- Used by some for "pay what you want" or free plugins. Developer-friendly, low fees (0% minimum)
4. **GitHub** -- For open-source plugins. Free hosting, CI/CD, community contributions. No payment integration
5. **Plugin Boutique/Splice** -- For established indie devs who can get accepted. Higher visibility but high commission
6. **KVR Marketplace** -- Low barrier to entry, dev-direct sales
7. **MuseHub** -- Newer option, handles full pipeline

### Pain Points for Indie Developers
- **Discovery:** The biggest problem. With 30,000+ products on KVR alone, getting noticed is extremely difficult. Marketing is expensive and time-consuming
- **Payment processing:** Handling international payments, VAT/tax compliance, refunds, chargebacks
- **Licensing/copy protection:** Implementing serial keys, machine fingerprinting, or dongle systems is complex. Many indie devs just use serial keys or trust-based systems
- **Cross-platform builds:** Building for Windows, macOS (Intel + ARM), and Linux across VST3/AU/CLAP/AAX is a matrix of complexity
- **Support burden:** Every customer interaction costs time. Installation issues, OS updates breaking things, DAW compatibility
- **Price pressure:** Race to the bottom. Free plugins set expectations. Hard to charge premium prices as an unknown developer
- **Fragmented tooling:** Need to manage a website, email list, social media, download hosting, license management -- all separately

### KVR Developer Challenge
- Annual contest for free plugins. Good for visibility but doesn't directly help with monetization
- Shows that the community values free/open contribution

### 2025 Indie Developer Survey
- moonbase.sh is conducting a "State of Independent Audio Plugin Companies" survey in 2025, focused on the business side: sales, marketing, pricing, distribution, and challenges
- Builds on a 2018 report. Results will provide updated data on the indie landscape

---

## 4. Plugin Format Landscape

### VST3
- **Status:** Dominant format. Supported by virtually every DAW
- **Licensing:** Recently open-sourced under MIT license by Steinberg (2025), previously required a licensing agreement
- **Strengths:** Universal compatibility. Massive ecosystem. Well-documented
- **Weaknesses:** Complex API. Steinberg controls the spec. No per-note modulation. Threading model limitations

### CLAP (CLever Audio Plug-in)
- **Status:** Growing. 15 DAWs, 93 plugin producers, 394 CLAP plugins as of Oct 2025
- **Licensing:** MIT/Apache-2.0. Truly open-source. No fees, no agreements, no approval
- **Created by:** Bitwig and u-he, with community contributions
- **Strengths:**
  - Full MIDI 2.0 support including per-note modulation
  - Better multicore threading model
  - Simpler API than VST3
  - No licensing fees or legal barriers
  - Two-phase loading (metadata without full init, speeding up scans)
  - File reference extensions (plugins declare needed samples/wavetables)
- **Weaknesses:**
  - Not yet supported by Ableton, Logic, Cubase, Pro Tools
  - JUCE 8 still lacks native CLAP support (most devs use JUCE)
  - Chicken-and-egg adoption problem
- **Relevance for Vibez:** As an open-source DAW, CLAP is the natural primary format. VST3 support (now MIT) should also be implemented. CLAP's metadata extensions are ideal for marketplace integration

### AU (Audio Unit)
- **Status:** macOS/iOS only. Required for Logic Pro compatibility
- **Licensing:** Apple's framework. Free to use on Apple platforms
- **Relevance:** Must-have for macOS builds of Vibez, but not primary marketplace format

### LV2
- **Status:** Open-source Linux standard. 1,200+ plugins available
- **Licensing:** Royalty-free open standard
- **Relevance:** Important for Linux audio community. Less relevant commercially but aligns with open-source ethos
- **Notable projects:** Linux Studio Plugins (LSP), x42-plugins

### AAX
- **Status:** Pro Tools only. Requires signing agreement with Avid
- **Relevance:** Low priority for Vibez. Pro Tools users won't be your early audience

### Marketplace Format Handling
- Most marketplaces (Plugin Boutique, Splice) distribute platform-specific installers. User selects OS, gets an installer that places plugin files in standard locations
- CLAP's file reference extension could enable a marketplace to understand a plugin's dependencies (samples, wavetables) and manage them
- A CLAP-first marketplace could offer simpler packaging since CLAP plugins are single .clap files (shared libraries) with no complex installation

---

## 5. Monetization Models

### Buy Outright (Traditional)
- **How:** One-time purchase. User owns license permanently
- **Examples:** Most plugins on Plugin Boutique, developer websites
- **Pros:** Simple. Users prefer ownership. No ongoing commitment
- **Cons:** Developers need continuous new releases to generate revenue. Upgrade pricing creates friction

### Rent-to-Own (Splice Model)
- **How:** Monthly payments until full price is paid. Interest-free. Pause/cancel anytime
- **Pros:** Low barrier to entry. Popular with younger/budget-conscious producers. Predictable revenue for developers
- **Cons:** Extended payment period. Users may abandon before ownership. Complex accounting

### Subscription Bundles
- **How:** Monthly/annual fee for access to a library. Examples: NI 360, Plugin Alliance, FL Cloud, Studio One+
- **Pros:** Predictable recurring revenue. Users get massive libraries. Reduces piracy incentive
- **Cons:** Subscription fatigue. Users lose access on cancel (except Plugin Alliance's "keep" model). Perceived as not owning anything

### Free + Paid Tiers (Vital Model)
- **How:** Core product is free (or open-source). Premium features/presets/support cost money
- **Pros:** Massive adoption. Community building. Reduces piracy to zero at base level
- **Cons:** Conversion rates are typically 2-5%. Need huge user base to sustain
- **Example:** Vital synth -- free basic version, $5 for more presets, $25 for full preset library and text-to-wavetable

### Donationware / Pay-What-You-Want
- **How:** Users choose their own price (including $0)
- **Examples:** Many Linux/open-source plugins, some itch.io distributions
- **Pros:** Accessible. Community goodwill
- **Cons:** Very low conversion/revenue. Not sustainable as primary model

### Revenue Splits Across Platforms

| Platform | Creator Share | Platform Share | Notes |
|----------|-------------|----------------|-------|
| Plugin Boutique (non-exclusive) | 60% | 40% | |
| Plugin Boutique (exclusive) | 70% | 30% | |
| Splice | ~70-80% (est.) | ~20-30% (est.) | Not publicly disclosed |
| Epic Fab | 88% | 12% | Best split in any marketplace |
| Unity Asset Store | 70% | 30% | Industry "standard" |
| Superhive (Blender Market) | 70-90% | 10-30% | 70% default, up to 90% with subscription |
| BlenderKit | 70% | 25% + 5% to Blender | Fair share model based on usage |
| Godot Marketplace | 70% | 30% (10% donated to Godot) | |
| Apple App Store | 70-85% | 15-30% | 85% for small business program |
| VS Code Marketplace | 100% | 0% | Free to publish. No monetization built in |

### Open-Source Project Marketplace Models

**Blender Ecosystem:**
- Superhive (formerly Blender Market): Traditional marketplace. 70-90% to creators. Subscription tiers for creators to get better rates
- BlenderKit: Subscription-based "fair share" model. Revenue pooled from subscriber payments, distributed to creators based on download count x (complexity x quality score). 70% to creators, 5% to Blender Foundation

**Godot Ecosystem:**
- Official Godot Asset Library: Completely free, open-source. GitHub-based. No commercial features
- Godot Marketplace (third-party): 70% to creators. Donates 10% of platform revenue to Godot Foundation. Fills the commercial gap

**VS Code:**
- Extension Marketplace: Free to publish and download. No built-in monetization
- Developers monetize via freemium (free extension + paid features), SaaS backends, or sponsorship
- Open-source alternative: coder/code-marketplace for self-hosted/air-gapped deployments

**Key lesson for Vibez:** Open-source projects benefit from having BOTH a free asset library (community contributions, open-source plugins) AND a commercial marketplace (indie devs making a living). The Godot model -- free official library + third-party commercial marketplace -- is a pragmatic middle ground. BlenderKit's usage-based fair share model is innovative but complex.

---

## 6. Technical Implementation

### Architecture for an In-DAW Marketplace

```
+------------------+     +-------------------+     +------------------+
|  Vibez DAW UI    |     |  Marketplace API  |     |  Plugin CDN      |
|  (Marketplace    |<--->|  (REST/GraphQL)   |<--->|  (S3/R2/B2)     |
|   Browser Panel) |     |                   |     |                  |
+------------------+     +-------------------+     +------------------+
                              |         |
                    +---------+    +----+--------+
                    |              |             |
              +-----+-----+ +----+----+ +------+------+
              | Auth/Users | | Payment | | Review/     |
              | (OAuth2)   | | (Stripe)| | Rating DB   |
              +------------+ +---------+ +-------------+
```

### Core Components

**1. Marketplace API Server**
- REST or GraphQL API for browsing, search, purchase, download
- Plugin metadata: name, description, version, author, format (CLAP/VST3/LV2), OS support, size, price, screenshots, audio demos
- Search: full-text + faceted (category, format, OS, price range, rating)
- Could be built in Rust (axum/actix-web) for consistency with DAW codebase

**2. Plugin Hosting & CDN**
- Store plugin binaries on object storage (S3, Cloudflare R2, Backblaze B2)
- CDN in front for fast global downloads
- Signed URLs for purchased/authorized downloads
- Separate hosting for free vs. paid content
- Consider: CLAP plugins are single .clap files (shared libraries), much simpler to host than multi-file VST installers
- Estimated storage: Start small. 1000 plugins x 50MB avg = 50GB. Very manageable

**3. License/Authorization System**
- **Option A: Trust-based (recommended for open-source DAW)**
  - Purchase generates a license key tied to user account
  - DAW checks license on download, not on every launch
  - No phone-home DRM. Once downloaded, it works
  - This aligns with open-source values and avoids iLok-style frustration

- **Option B: Lightweight phone-home**
  - Periodic license validation (e.g., once per week)
  - Graceful degradation (plugins work offline for N days)
  - Still no dongles or intrusive DRM

- **Option C: Cryptographic signing (best compromise)**
  - Marketplace signs plugin binaries with a key. DAW verifies signature
  - Purchased plugins get a user-specific signed manifest
  - Prevents tampering, provides authenticity, no ongoing phone-home
  - Similar to how package managers (apt, cargo) work

**4. Auto-Updates**
- Plugin manifest includes version info and checksums
- DAW periodically checks for updates (configurable)
- One-click update from within the DAW
- Rollback capability (keep previous version)
- CLAP's version extension facilitates this

**5. Developer Portal**
- Web-based dashboard for developers to:
  - Upload plugin binaries (per OS/format)
  - Write descriptions, upload screenshots and audio demos
  - Set pricing (free, paid, rent-to-own, donation)
  - View sales analytics
  - Respond to reviews
  - Manage versions and changelogs
- Submission review process: automated (virus scan, format validation, basic testing) + optional community review

**6. Review & Rating System**
- Star ratings (1-5) + written reviews
- Only verified purchasers can review (prevents spam)
- Developer can respond to reviews
- Report mechanism for abuse
- Aggregate scores with recency weighting

### DRM Philosophy for an Open-Source DAW
The community strongly dislikes heavy DRM:
- iLok is widely hated ("the highest maintenance copy protection ever made")
- Joey Sturgis Tones moved away from iLok in October 2025 due to user backlash
- Forum sentiment is polarized but trending anti-DRM
- REAPER's model (honor system + serial key) is well-regarded
- **Recommendation:** Trust-based or cryptographic signing. Make the marketplace experience so good that buying is easier than pirating. This is the Steam model for games, and it works

---

## 7. Community & Sharing Features

### Preset Sharing
- **Existing platforms:** PresetShare.com (18,000+ free presets, 354,000 users, 15M downloads), Producer Presets, Vital's forum
- **In-DAW implementation:** Built-in preset browser that connects to community preset repository. Users can upload/download presets per-plugin. Tagging, categorization, ratings
- **Revenue opportunity:** Free community presets + paid premium preset packs from sound designers

### Project Template Sharing
- Share starter templates (genre-specific setups with routing, effects chains, instrument configurations)
- Could include "stems" or arrangement structures
- Educational value: learn from other producers' setups

### Sample Pack Sharing
- Compete with Splice Sounds and Loopcloud at a smaller scale
- Community-contributed sample packs with tagging and preview
- In-DAW browser for browsing and auditioning samples
- Royalty-free licensing for shared content

### User Profiles & Following
- Creator profiles showing their plugins, presets, sample packs
- Follow system for notifications on new releases
- Download/sales counters for social proof
- Integration with forum/community discussions

### Reference: How Other Platforms Handle Community

| Platform | Community Features |
|----------|-------------------|
| Splice | Follows, playlists, creator profiles, "Create" mode for AI-matched sounds |
| Vital | Forum-based sharing, community preset packs, skins |
| PresetShare | User profiles, preset packs, audio previews, download counts |
| Loopcloud | In-DAW browsing, effects processing on samples, BPM/key matching |
| Arcade (Output) | Playable instrument, daily new content, AI-powered "Co-Producer" |

---

## 8. Lessons from Other Domains

### Superhive (formerly Blender Market)
- **Model:** Traditional marketplace with subscription tiers for creators (70-90% revenue share)
- **Lesson:** Creator-friendly terms attract quality content. No exclusivity requirement. Let creators set their own affiliate rates
- **Lesson:** Rebranding (Blender Market -> Superhive) can be risky for brand recognition

### BlenderKit
- **Model:** "Fair share" subscription marketplace. Users pay subscription, revenue distributed to creators based on usage
- **Lesson:** Usage-based revenue is fairer than per-sale but harder to explain to creators. Need transparent scoring algorithm
- **Lesson:** Donating 5% to open-source project builds goodwill and sustainability

### Godot Asset Library
- **Model:** Free, open-source, community-driven. All assets are free
- **Lesson:** A free community library is table stakes for an open-source project. But it can't sustain commercial developers alone
- **Lesson:** Need a separate commercial marketplace for people who want to sell

### VS Code Extension Marketplace
- **Model:** Free to publish and consume. No built-in monetization
- **Lesson:** Massive adoption (50,000+ extensions). But quality varies wildly without curation
- **Lesson:** Private/enterprise marketplace features came later (2025) -- shows demand for curation
- **Lesson:** The open-source coder/code-marketplace project shows that self-hosting marketplaces has demand
- **Lesson:** Without built-in monetization, developers use freemium/SaaS backends, fragmenting the experience

### Epic Fab
- **Model:** 88/12 split. Supports multiple engines (Unity, Unreal, Godot, Blender)
- **Lesson:** Engine-agnostic marketplace can capture broader audience. 88/12 is the new benchmark for creator-friendly splits
- **Lesson:** Cross-engine compatibility is valuable but complex to enforce

### Unity Asset Store
- **Model:** 70/30 split. Massive catalog
- **Lesson:** 70/30 is increasingly seen as too much for the platform. Competitive pressure from Fab's 88/12
- **Lesson:** Automated submission review + community flagging scales better than manual curation

### Steam (Games)
- **Lesson:** Making buying easier than pirating is the best DRM. Convenient marketplace + social features + library management eliminated most casual piracy
- **Lesson:** User reviews are critical for discovery and trust
- **Lesson:** Wishlists drive significant sales (notification on price drop)
- **Lesson:** Workshop (user-generated content) creates massive community engagement

---

## 9. Recommendations for Vibez Marketplace

### Phased Approach

**Phase 1: Community Asset Library (Free)**
- GitHub-based or self-hosted repository of free CLAP/LV2 plugins and presets
- In-DAW browser for discovery and one-click install
- Community contributions via pull requests or web upload
- Similar to Godot Asset Library
- No payment infrastructure needed yet
- Focus on getting the UX right: search, categories, preview, install, update

**Phase 2: Commercial Marketplace (Paid)**
- Add payment processing (Stripe)
- Developer portal for commercial submissions
- 85/15 or 88/12 split (be competitive with Fab, not with the old 70/30 model)
- Donate 5% of platform revenue to Vibez development (BlenderKit model)
- Rent-to-own option for plugins above a price threshold
- Review and rating system

**Phase 3: Creator Ecosystem**
- Preset marketplace (free + paid)
- Sample pack marketplace
- Project template sharing
- User profiles, following, creator verification
- Subscription tier for "all access" to participating plugins

### Key Design Principles

1. **CLAP-first:** Prioritize CLAP plugins. Simpler packaging (single .clap file), better metadata, no licensing barriers. Support VST3 and LV2 as well
2. **No hostile DRM:** Trust-based or cryptographic signing. Never require dongles or persistent internet. Make buying easy, not punishing
3. **Creator-friendly splits:** 85/15 or better. Donate a portion to open-source. Undercut the 70/30 standard
4. **In-DAW experience:** The marketplace should be a first-class panel in the DAW, not a separate app. Browse, preview, buy, install, update -- all without leaving Vibez
5. **Open protocol:** Consider making the marketplace protocol open so others can host compatible marketplaces (like VS Code's open marketplace spec). Aligns with open-source values
6. **Audio previews:** Let users hear plugins before buying. Inline audio demos are critical for music software
7. **Community-first:** Free content library alongside commercial marketplace. Preset sharing. Template sharing. Build the community before monetizing

### Technical Stack Suggestion (Rust-Native)

- **API server:** axum or actix-web (Rust)
- **Database:** PostgreSQL (metadata, users, reviews) + object storage (plugin binaries)
- **Search:** Meilisearch or Typesense (fast, relevant, faceted search)
- **CDN:** Cloudflare R2 (S3-compatible, no egress fees) or Backblaze B2
- **Payments:** Stripe (handles international payments, tax compliance, payouts to creators)
- **Auth:** OAuth2 (GitHub, Google, email/password)
- **In-DAW client:** HTTP client (reqwest) embedded in Vibez, with local cache and manifest
- **Package format:** `.vibez-pkg` -- a signed archive containing the .clap/.vst3 binary, metadata.toml, screenshots, and audio demo

---

## Sources

### Plugin Marketplaces
- [Splice Rent-to-Own](https://splice.com/plugins/rent-to-own)
- [Plugin Boutique Rent-to-Own](https://www.pluginboutique.com/rent-to-own-plugins)
- [Selling with Plugin Boutique](https://help.pluginboutique.com/hc/en-us/articles/6232293017108-Selling-with-Plugin-Boutique)
- [Plugin Alliance Subscriptions](https://www.plugin-alliance.com/pages/subscriptions)
- [NI 360 Subscription](https://www.native-instruments.com/en/products/subscription/360-subscription/)
- [KVR Audio Marketplace](https://www.kvraudio.com/marketplace/orders/)
- [Knobcloud - Buy/Sell Plugin Licenses](https://knobcloud.com/faq)
- [MuseHub Plugin Distribution](https://blog.musehub.com/musehub-plugin-distribution/)
- [How to Sell and Distribute Your VST Plugins](https://blog.musehub.com/sell-distribute-vst-plugins/)

### In-DAW Stores
- [FL Cloud](https://www.image-line.com/fl-cloud)
- [FL Studio 2024 Introduces Plugins to FL Cloud](https://www.recordingmag.com/news/fl-studio-2024-introduces-plugins-to-fl-cloud/)
- [Ableton Live 12 Browser](https://help.ableton.com/hc/en-us/articles/12927340213660-The-Live-12-Browser)
- [Logic Pro Sound Library Management](https://support.apple.com/en-us/guide/logicpro/lgcpf77e757d/mac)
- [PreSonus Studio One+](https://musictech.com/news/gear/presonus-studio-one-plus/)

### Plugin Formats
- [CLAP - Bitwig](https://www.bitwig.com/stories/clap-the-new-audio-plug-in-standard-201/)
- [CLAP Audio Plugin Format - Martinic](https://www.martinic.com/en/blog/clap-audio-plugin-format)
- [VST3 Open-Sourced Under MIT](https://www.kvraudio.com/forum/viewtopic.php?t=624544)
- [CLAP Wikipedia](https://en.wikipedia.org/wiki/CLever_Audio_Plug-in)
- [LV2 Plugin Standard](https://lv2plug.in/)
- [Linux Studio Plugins](https://lsp-plug.in/)

### Revenue Models & Marketplaces
- [Fab.com Distribution Agreement](https://www.fab.com/distribution-agreement)
- [Fab 88/12 Revenue Share](https://www.epicgames.com/site/en-US/news/fab-epics-new-unified-content-marketplace-launches-today)
- [Unity Asset Store Publisher Guide](https://makaka.org/unity-assets/unity-asset-store-publisher)
- [Superhive (Blender Market) Creator Info](https://superhivemarket.com/become-a-creator)
- [Superhive Commission Calculation](https://support.superhivemarket.com/article/32-how-commission-earnings-are-calculated)
- [BlenderKit Fair Share](https://www.blenderkit.com/docs/fair-share/)
- [Godot Marketplace](https://godotmarketplace.com/support-open-source/)
- [Godot Asset Library](https://godotengine.org/asset-library/asset)

### Community & Sharing
- [Splice - Spitfire Audio on Rent-to-Own](https://musictech.com/news/gear/splice-spitfire-audio-rent-to-own/)
- [PresetShare.com](https://presetshare.com/)
- [Vital Synth](https://vital.audio/)
- [Loopcloud vs Splice Comparison](https://www.edmprod.com/loopcloud-vs-splice/)

### DRM & Licensing Sentiment
- [Goodbye iLok - Joey Sturgis Tones](https://joeysturgistones.com/blogs/learn/everything-engineers-get-wrong-about-ilok)
- [KVR Forum: Thoughts on iLok](https://www.kvraudio.com/forum/viewtopic.php?t=583988)
- [KVR Forum: Plugin Marketplaces - Worth It?](https://www.kvraudio.com/forum/viewtopic.php?t=542545)

### Developer Ecosystem
- [State of Independent Audio Plugin Companies 2025 Survey](https://www.kvraudio.com/forum/viewtopic.php?t=623548)
- [VS Code Extension Publishing](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)
- [Open Source VS Code Marketplace (coder)](https://github.com/coder/code-marketplace)
- [Fab.com vs Unity Asset Store 2026 Guide](https://www.syncbrief.com/p/fab-com-vs-unity-asset-store-the-2026-game-composer-s-marketplace-guide)
