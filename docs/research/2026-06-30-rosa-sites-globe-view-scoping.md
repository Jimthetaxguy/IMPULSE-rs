# Globe / Map View — Scoping & Recommendation

## 1. Verdict

Build a **ROSA-frontend "sites map" feature** — a 2D web-mercator map that can smoothly *globe-out* on zoom — that reads the routines KB as its data source over a typed read-model, renders each geolocated "site" as one point carrying **N toggleable, provenance-tagged overlay layers**. The single recommended stack is **MapLibre GL JS v5 (open-source basemap, native globe projection) + a deck.gl v9 overlay layer stack (GPU-virtualized pins / hex-bin aggregation / heatmap)**. This is unambiguously a **ROSA L2 presentation feature** — the routines Rust crate stays a backend knowledge *producer*; the globe *consumes* `kb/<tenant>/<date>.md`. The facets agree on this boundary, and I am adopting it: the globe lives in its **own repo**, never inside the routines crate. The one place the facets conflict — "3D globe first" (library survey leans deck.gl GlobeView / react-globe.gl) vs "2D-that-globes" (UX facet) — I resolve **in favor of the UX facet**: the actual data is one tenant's tight local cluster around Long Island City, and a literal 3D globe spends its entire visual budget on empty ocean. The globe is a *projection toggle*, not the default metaphor.

## 2. PR Decision — routines crate (separate from the globe)

**GO — create a private remote and push `main` now.** This is the highest-value, lowest-risk action in the whole request, and it is fully decoupled from the globe. The repo is Phase-1-complete, verified (57 tests + 1 gated live integration test, clippy/fmt clean on both feature configs), clean tree, 17 commits — and it exists on exactly one disk. The binding reason is **durability**: the user has lost unbacked work before, and `github-first-sync` + `branch-age-hygiene` both demand offsite backup. "PR" is the wrong frame — there is no remote and no feature branch, so there is nothing to open a PR *against*. Back it up, then branch going forward.

**Steps (do the two hygiene fixes first, then push):**
1. Un-gitignore the lockfile — this crate ships a CLI binary (`src/main.rs`), so `Cargo.lock` must be committed for reproducible builds:
   - remove `Cargo.lock` from `.gitignore`, then `git add Cargo.lock`
2. Add a `LICENSE` (or an explicit `All Rights Reserved` note) — hygiene, not a ship gate, per `feedback_private_repos.md`.
3. Belt-and-suspenders secret scan before push: `rg -n 'sk-|ghp_|AKIA|BEGIN.*PRIVATE KEY|secret|token' src/` (adapters should carry `secret_ref` only — already verified clean).
4. Confirm no name collision, then:
   ```
   gh repo create routines --private --source=. --remote=origin --push
   ```
5. Going forward: solo **merge-to-main** is fine; branch to `agent/<feature>` only for risky Phase-2 changes, with self-review via `superpowers:requesting-code-review`.

Do **not** monorepo the globe into this crate — it violates the ION/IMPULSE-vs-ROSA layer boundary and forces a premature polyglot toolchain.

## 3. Recommended Approach

**Chosen render stack: MapLibre GL JS v5 + deck.gl v9 overlay (Option B, "2D-that-globes").** Fit scores that drove this:
- deck.gl as the overlay engine: **fitScore 5** in the library facet — the only candidate that unifies GPU virtualization, aggregation/clustering (Hexagon/Grid/Heatmap), a reactive `layers` array where *toggling = add/remove a layer*, rich picking for per-site metadata, and MIT/no-token licensing.
- "2D-that-globes": **fitScore 5** in the UX facet vs **fitScore 2** for 3D-globe-first. The data is local, sparse, single-tenant; 3D curvature hurts label legibility and click accuracy on a dense cluster.
- MapLibre GL v5's **native globe projection** (GA Feb 2025) resolves the tension for free: default to flat mercator for the personal case, interpolate to an honest globe on zoom-out (past ~z3) when data actually spans the planet (multi-tenant, travel history).

**Fallback:** `react-globe.gl` (three.js, **fitScore 4** in the library facet) *only if* the priority ever flips to a fast, photoreal 3D-globe demo aesthetic over data-scale/LOD. **Rejected:** Mapbox GL JS (token/commercial), CesiumJS (multi-MB bundle, over-engineered for L2 point overlays — fitScore 3), Kepler.gl (a full Redux app, not embeddable — fitScore 2), custom R3F globe (reimplements solved geodesy — fitScore 2).

**Why not just deck.gl GlobeView?** It is officially flagged **Experimental** (no terrain, limited raster tiles, camera edge cases). MapLibre v5's globe projection is GA and gives a real vector basemap underneath. Same deck.gl overlay layers ride on both, so the globe is a projection toggle, not a rewrite — the experimental path is never a hard dependency.

**Verify-before-code note:** MapLibre v5 globe API and the deck.gl v9 interleaved-layer contract both moved fast through 2025; training data is stale. Pin exact versions and re-check `maplibre.org/maplibre-gl-js/docs` and `deck.gl/docs` before wiring. deck.gl 9.3.5 and MapLibre v5 verified live 2026-06-30.

## 4. Architecture & Data Contract

**Where it lives:** A ROSA L2 frontend (React/TS), separate repo, consuming routines KB over an **MCP/HTTP read-model** (R3: typed core is authority, MCP at the edge). The globe never reads Rust internals — it reads a typed `SiteCollection` JSON produced by the crate.

**Data flow (prose diagram):**
```
routines crate (Rust, backend/producer)
  │  daily-brief routine → sensors (REAL Google Calendar events w/ location string,
  │                                  REAL api.weather.gov @ home lat/lon)
  │  → rule-based brain (flags) → kb/<tenant>/<date>.md  (provenance frontmatter)
  │
  │  NEW: geocode.resolve capability (trait; Nominatim/Google adapter, feature-gated)
  │       turns CalendarEvent.location STRING → {lat,lon}, itself a provenance-bearing
  │       Contribution with gaps[]  (geocode failures = per-site gap, NOT a dropped site)
  │
  │  NEW: sites projection → SiteCollection (typed, tenant-partitioned, serde_json)
  │       persisted as kb/<tenant>/sites/<date>.json (reuses kb::Contribution/Provenance)
  ▼
MCP read-model tool  (e.g. sites.read {tenant, date|range})  ← seam; single tenant per call
  ▼
ROSA frontend (React + MapLibre v5 + deck.gl v9)
  │  projects SiteCollection → ONE GeoJSON FeatureCollection PER overlay layer (edge-side)
  │  layer-toggle UI ⇄ reactive deck.gl layers array
  ▼
Map/globe: pins + aggregation + per-site detail card (renders provenance verbatim)
```

**Geocoding — new routines sensor capability, not a frontend hack.** Turning `CalendarEvent.location` free-text into coordinates is a **hard prerequisite** and belongs in the crate as a real `geocode.resolve` trait boundary (Nominatim self-host or Google behind a feature-gated adapter, secret via Keychain+Infisical `secret_ref`, **real-systems-only, never mocked**). Its output is a `Contribution` with its own provenance and `gaps[]`. Un-geocodable locations surface as an "unplaced sites" tray — never silently dropped (respects `do-not-unify` + `gaps[]`).

**How the invariants cross the seam:**
- **Provenance:** every `OverlayLayer` carries `kb::Contribution` (capability slug + provider + account_id) + `source_version` + `recorded_at` + `gaps[]`, reused verbatim from `kb.rs`. The detail card renders these as a **first-class UI element**, not a debug affordance.
- **do-not-unify:** overlays are a `Vec<OverlayLayer>` (a *federation*). Two weather sources = two entries, never an averaged number. GeoJSON is a *per-layer projection at the edge*, never the source of truth — this is exactly what deck.gl/MapLibre want (one styled layer per source).
- **Multi-tenant:** tenant lives on the `SiteCollection` envelope **and** is embedded in every `SiteId`; the MCP read-model serves exactly one tenant's partition per call (or an explicit, labeled federation view — never a merged point cloud), mirroring `kb.rs` `write_rejects_tenant_mismatch`.

I adopt the data facet's **Option A (typed SiteCollection, fitScore 5)** over raw GeoJSON-as-canonical (fitScore 2, erodes typing/tenant/provenance) and the flat observation log (fitScore 3, pushes site-formation onto every client).

## 5. Data Model

Canonical wire type = typed `SiteCollection`, tenant-partitioned, reusing existing `kb::Contribution`/`Provenance`. GeoJSON is generated per-layer at the edge (the single place the `[lon,lat]` vs crate `(lat,lon)` axis order is handled).

```rust
struct SiteId(String);                 // deterministic: hash(tenant | round(lat,5) | round(lon,5) | label)
enum SiteKind { Home, CalendarEventLocation, KbPlace, PointOfInterest, FinanceLocation, MessageDerived }
enum RiskTier { T0, T1, T2, T3, T4 }   // Home / finance / message-derived skew higher

struct Geo { lat: f64, lon: f64, elevation_m: Option<f64>, geohash: Option<String> }  // geohash → clustering/LOD

enum OverlayValue {                    // "different info per site" = a federation of typed layers
  Weather  { temp_f: i32, precip_pct: Option<u8>, short_forecast: String },
  Calendar { event_count: u32, next_title: Option<String>, next_start: Option<String>, outdoor_flag: bool },
  KbFacts  { fact_count: u32, snippets: Vec<String>, last_brief_date: Option<String> },
  Source   { capability: String, provider: Option<String>, account_id: Option<String> },
  Freshness{ recorded_at: String, age_seconds: i64 },
  Privacy  { tier: RiskTier, redacted: bool },
  Flags    { flags: Vec<String> },     // brain outputs, e.g. outdoor-event-precip
}

enum VisualEncoding {                  // how a layer maps to a deck.gl channel
  ColorScale { field: String, domain: [f64;2], ramp: String },
  Size { field: String, min: f64, max: f64 },
  Icon { glyph: String },
  Heatmap { weight_field: String },
  Categorical { field: String, palette: Vec<String> },
  Badge { count_field: String },
}

struct OverlayLayer {
  value: OverlayValue,
  provenance: kb::Contribution,        // REUSED — capability + account_id + provider
  recorded_at: String, source_version: String,
  encoding: VisualEncoding, gaps: Vec<String>,
}

struct Site {
  id: SiteId, tenant: TenantId, geo: Geo, label: String, kind: SiteKind,
  source: kb::Contribution,            // ORIGINATING capability
  first_seen: String, last_seen: String, risk_tier: RiskTier,
  overlays: Vec<OverlayLayer>,         // FEDERATION — never collapsed to one number
}

struct SiteCollection {
  tenant: TenantId, generated_at: String, source_version: String,
  bbox: Option<[f64;4]>, layers_index: Vec<String>,  // present overlay kinds → client LOD decisions
  sites: Vec<Site>, gaps: Vec<String>,
}
```

**Overlay taxonomy — what "different info per site" means concretely (v1):** each site carries some subset of **Weather** (home anchor + geocoded event sites), **Calendar density** (event count / next event / outdoor flag), **KB-derived facts** (fact count + snippets from past briefs), and **Provenance/freshness health** (which capability produced it, how stale, gaps). Different sites legitimately carry *different* layers (the home site has weather; a one-off restaurant has calendar + KB facts only) — the `layers_index` tells the client what exists without parsing every site.

## 6. UX

**Interactions:** pan/zoom 2D map by default; time-scrubber steps through discrete brief dates (KB is date-keyed) driving `deck.gl layer.updateTriggers` re-binds; hover → tooltip, click → detail card. Globe projection engages automatically on zoom-out when data spans >1 country or >1 tenant (hysteresis + manual lock to avoid jumpy mid-interaction switches). A synchronized DOM **site-list** gives keyboard/screen-reader parity — 3D rotation is hard to make accessible, another reason 2D is the default.

**Layer-toggle model:** a layer panel of checkboxes, each bound 1:1 to an entry in the deck.gl `layers` array — toggling = include/exclude the layer. This is deck.gl's first-class idiom and maps directly to the `SiteCollection.layers_index`.

**Multiple overlays without clutter — orthogonal channels.** Cap **~3 simultaneously-active visual channels**: color = primary metric, radius = secondary, icon = category, with an optional heatmap underlay. Everything beyond that lives behind the layer panel, not stacked on the map. This is the discipline that keeps "many metrics × many sites" readable.

**Virtualization strategy (honors the literal "virtualized" ask without needing 3D):** deck.gl GPU instancing + viewport culling + **H3/hex-bin aggregation**. Low zoom → hex-bin density; mid zoom → clustered pins; high zoom → individual sites + detail. One code path scales 3 → 100k+ sites. Without real aggregation "virtualized" is just a word — a marker-per-site approach janks past a few thousand points.

**Degenerate/empty states designed first, not last:**
- **0 geocodable locations** → static list/card view, no map canvas; unplaced-sites tray shows the geocode gaps.
- **1 tenant, 3 sites** (today's reality) → clean flat neighborhood map, globe suppressed; never an ocean-heavy sphere.
- **T3+ sites (Home, message-derived)** → the `Privacy` overlay **gates rendering**: coordinates fuzzed/withheld (`redacted: true`) until unlocked.

## 7. Phased Plan (dependencies, not time)

Critical path from "smallest real thing" to full virtualized multi-overlay globe. Each phase is a thin, real vertical.

- **Phase 0 — Backup (unblocks nothing technical, but P0 for safety).** Push routines crate to private remote (§2). No dependency on any globe work; do it immediately.

- **Phase 1 — One real site on a map.** *Depends on:* geocode.resolve capability existing in the crate (new trait + one real adapter), because calendar locations are strings.
  - Build `geocode.resolve` (real Nominatim/Google adapter, provenance-bearing, gaps on failure).
  - Emit a minimal `SiteCollection` from **today's** KB: home site (weather) + geocoded calendar-event sites, **one overlay layer (calendar density)**.
  - ROSA frontend: MapLibre v5 flat map + a single deck.gl ScatterplotLayer, click → detail card **rendering provenance verbatim**.
  - This is the thin real vertical: today's calendar+weather sites on a map with one overlay, reusing the KB.

- **Phase 2 — Federation of overlays + toggle UI.** *Depends on:* Phase 1 SiteCollection.
  - Add Weather, KB-facts, Provenance/freshness overlays as separate `OverlayLayer` entries; layer-toggle panel bound to the deck.gl layers array; orthogonal-channel encoding (color/size/icon).
  - Privacy gating for T3+ sites.

- **Phase 3 — Virtualization + time.** *Depends on:* Phase 2 (multiple layers to aggregate).
  - H3/hex-bin aggregation + viewport culling; time-scrubber across date-keyed KB (single-date → later date-range aggregation like "calendar density over 30 days"); persist `kb/<tenant>/sites/<date>.json`.

- **Phase 4 — Globe projection + multi-tenant.** *Depends on:* Phase 3 (virtualization) **and** a real second tenant / travel data existing (Tiffany, or trip history). Do **not** build the auto-projection selector before tenant #2 lands — scope guardrail.
  - Enable MapLibre v5 globe projection on zoom-out; adaptive 2D↔globe selector (UX facet Option C, fitScore 4) with manual override; explicit labeled per-tenant federation view (never a merged point cloud).

**Blocking summary:** Phase 0 is independent and first. Phase 1 blocks on geocoding. Phases 2→3→4 are strictly sequential. Phase 4's globe/multi-tenant work is gated on real second-tenant data — building it earlier is premature.

## 8. Open Decisions for the User (with my recommended defaults)

1. **Geocoder for calendar location strings** — *Recommend:* self-hosted **Nominatim** (no per-call cost, no vendor token, T1). Google Geocoding via the existing calendar account is the fallback if quality is insufficient. Needs a `secret_ref` + provenance either way.
2. **Basemap tiles / offline posture** — *Recommend:* MapLibre **open vector tiles / self-hosted style** (token-free, cache-friendly, honors secret hygiene). Avoid any hosted-token basemap.
3. **3D globe required for the ROSA showcase now, or 2D-first?** — *Recommend:* **2D-first (Phase 1-3), globe as Phase 4 projection toggle.** If a globe demo is needed *this week* for the showcase, that is the only reason to reorder — say so explicitly.
4. **v1 overlay set + each site's PRIMARY (color) metric** — *Recommend:* Phase 1 = calendar density (primary). Phase 2 adds weather + KB-facts + provenance-health.
5. **Fuzzing policy for T3+ sites** — *Recommend:* coarse **geohash-only + withhold-until-unlock** for Home/message-derived; jitter is weaker.
6. **SiteCollection: persisted artifact vs computed on-demand** — *Recommend:* **persist** `kb/<tenant>/sites/<date>.json` alongside the daily brief (matches the KB's date-keyed ledger mindset; enables the time scrubber cheaply).
7. **Temporal sites (past-date overlays) in v1?** — *Recommend:* **no** — single live date for v1; date-range aggregation is Phase 3.
8. **Multiple home anchors per tenant (work/LIC/family)?** — *Recommend:* keep single `HomeLocation` for v1; generalize when a real second anchor is needed.
9. **Globe repo namespace** — *Recommend:* new private repo under the IMPULSE/ION/ROSA stack namespace (ROSA-frontend), separate from routines.

## 9. Risks & Mitigations

- **Metaphor mismatch (globe for local data).** *Mitigation:* 2D-that-globes default; globe engages only when data actually spans the planet. This is the core resolved conflict.
- **Geocoding gap (locations are strings, many un-geocodable).** *Mitigation:* real `geocode.resolve` capability with provenance; failures → "unplaced sites" tray + `gaps[]`, never silent drops.
- **Provenance erosion / do-not-unify.** *Mitigation:* overlays are a typed `Vec<OverlayLayer>` (federation), GeoJSON is an edge projection only, every displayed metric renders its `Contribution` + `source_version` + `recorded_at` + `gaps`. No averaging, no merged blob.
- **Tenant leakage.** *Mitigation:* tenant on the envelope + embedded in `SiteId`; MCP read-model serves one partition per call; check mirrors `kb.rs` mismatch enforcement.
- **Privacy leak (Home / message-derived coordinates).** *Mitigation:* `Privacy` overlay gates rendering; T3+ coords fuzzed to coarse geohash / withheld until unlock.
- **Virtualization theater.** *Mitigation:* real H3/hex-bin aggregation + culling from Phase 3, not marker-per-site.
- **Overlay clutter.** *Mitigation:* cap ~3 active visual channels; rest behind the layer panel.
- **Library version drift (MapLibre v5, deck.gl v9, GlobeView still Experimental).** *Mitigation:* pin exact versions, verify APIs against live docs before code, keep the stable 2D MapView path as the non-experimental floor.
- **`SiteId` collisions from rounded lat/lon.** *Mitigation:* include `label` in the hash and tune precision.
- **Axis-order footgun (`[lon,lat]` GeoJSON vs crate `(lat,lon)`).** *Mitigation:* handled in exactly one place — the edge projection adapter.
- **Single-machine data loss (routines crate).** *Mitigation:* Phase 0 private push, done immediately and independently.
- **Scope creep (globe pulled into routines repo).** *Mitigation:* separate repo, enforced layer boundary; globe consumes KB, never co-lives with it.

---

## Adversarial critique (workflow phase 3)

- **Biggest risk:** Phase 1 is secretly huge — it is not a thin vertical. The design smuggles FOUR net-new subsystems across two languages and a new repo into "Phase 1 — one real site on a map": (a) a brand-new `geocode.resolve` sensor capability in the Rust crate, which requires a change to the closed R1 capability registry (`registry.rs`) + a new trait + a real Nominatim/Google adapter + provenance/gaps wiring; (b) a net-new persisted `SiteCollection` type + `kb/<tenant>/sites/<date>.json` writer; (c) a net-new MCP/HTTP read-model server (`sites.read`) that DOES NOT EXIST TODAY — the crate is currently a CLI binary that writes markdown, there is no server, no HTTP surface at all; and (d) a net-new React/TS repo wiring MapLibre v5 + deck.gl v9. Rendering "one real site" requires standing up all four. The design explicitly calls geocoding a "hard prerequisite" for Phase 1 — that admission is the tell that Phase 1 is the whole architecture minus the toggle UI. The genuinely thin vertical is ignored: the crate ALREADY has one real geocoded coordinate — the home lat/lon (LIC [home-geohash-at-export-boundary]) — with real weather.gov data already attached. Phase 1 should render THAT one point, from the existing KB, with no geocoder, no new trait, no R1 change, and defer the geocoder (the actual risky/failure-prone part) to Phase 2.
- **Recommendation sound?** True
- **Scope-creep:** Two distinct kinds of creep. (1) Speculative data model: `SiteKind` includes `FinanceLocation` and `MessageDerived`, and `OverlayValue`/`RiskTier` reference finance/message sources — NONE of that data exists in the routines crate, which only produces calendar + weather + KB-facts. Those are other-tenant/other-project (IMPULSE/ROSA) future inputs being baked into the v1 wire type as speculative generality; a wire type is expensive to change once a frontend depends on it, so this is the worst place to over-generalize. Trim to what the KB actually emits: Home, CalendarEventLocation, KbPlace. (2) Premature stack weight: deck.gl v9 is pulled into Phase 1 to render ~3 points via a single ScatterplotLayer. deck.gl is the correct choice for the Phase-3 virtualization/aggregation story, but it is heavy dependency weight for a 3-point neighborhood map. Plain MapLibre markers cover Phase 1-2; deck.gl earns its place only when H3/hex-bin aggregation is real (Phase 3). The design's own "virtualization theater" risk applies to itself: at current data scale (one tenant, one tight LIC cluster) NO virtualization is needed, so leading with the full GPU stack is aspirational. To its credit the design flags this honestly and defers the globe to Phase 4 — but it still front-loads the heavy renderer. Otherwise the layering discipline is good and correctly resists the one dangerous merge (globe-into-routines-repo).
- **Gaps:**
  - Privacy leak at the DATA layer, not just the render layer: the design's Privacy gating (redact/fuzz T3+ Home and message-derived coords) is described as a client-side/frontend `Privacy` overlay that 'gates rendering.' But persisting `kb/<tenant>/sites/<date>.json` with resolved lat/lon writes PRECISE coordinates for Home/message-derived sites to disk — coordinates that did not previously exist in that form. Redaction/fuzzing must happen at the persistence + read-model boundary (coarse geohash only, never raw lat/lon on disk for T3+), otherwise the artifact itself is the leak regardless of what the client draws.
  - do-not-unify tension inside SiteId: `SiteId = hash(tenant | round(lat,5) | round(lon,5) | label)` performs a spatial MERGE — two distinct calendar events at the same building collapse to one site server-side via a rounding heuristic. That is exactly the kind of silent collapse do-not-unify warns against. Site-formation (which raw observations became one site) is itself a federation decision and needs an explicit provenance trail (list of contributing observations), not an opaque hash. The design rejected the observation-log facet partly for 'pushing site-formation onto the client' — but it just moved an un-audited site-formation heuristic to the server.
  - Weather-per-site is a real-systems cost/failure trap the design waves past: api.weather.gov is US-ONLY and needs TWO calls per point (points -> gridpoint forecast). N geocoded event sites = N Nominatim calls + 2N weather.gov calls per brief, and weather.gov silently 404s outside the US — which is precisely the Phase-4 travel/multi-tenant scenario the design is aiming for. Weather on non-home sites must be explicitly gaps-bearing and should NOT be in Phase 1-2 for anything but the home anchor.
  - The MCP read-model server is an undelivered, unphased deliverable. It appears in the architecture diagram and §4 but is never given its own phase or effort acknowledgment — it is folded invisibly into Phase 1. Given the crate has zero HTTP/server surface today (CLI only), standing up a typed `sites.read` MCP tool (Streamable HTTP per R3 seam) is a real, separate build step and should be its own gate.
  - Version claims are asserted but not independently reproducible in this review: 'MapLibre v5 globe projection GA Feb 2025', 'deck.gl 9.3.5', 'GlobeView still Experimental' — the design correctly flags these as verify-before-code, but they remain the single biggest technical dependency and should be re-confirmed against live docs (Context7 / maplibre.org / deck.gl) as an explicit pre-Phase-1 gate, not a footnote.
  - Minor accuracy: the PR section says '17 commits' — the actual count is 18 (verified via git log). Does not change the GO decision.
- **Adjustments before user review:**
  - RESCOPE PHASE 1 to remove the geocoder entirely. Phase 1 = render the ONE home coordinate already present in the crate (LIC [home-geohash-at-export-boundary]) with its already-real weather.gov overlay, read from today's KB, on a plain MapLibre v5 map, click -> detail card rendering provenance verbatim. No `geocode.resolve`, no R1 registry change, no new adapter, no deck.gl. This proves the full seam (crate -> read-model -> frontend -> provenance render) with zero new backend risk. Move `geocode.resolve` (the hard, failure-prone part) to Phase 2 where its gaps[]/unplaced-tray machinery is the point.
  - Make the MCP read-model its OWN explicit phase step (Phase 1a): stand up the typed `sites.read {tenant, date}` tool over Streamable HTTP before any frontend work, since the crate has no server surface today. Gate the frontend on it.
  - Move Privacy redaction to the persistence/read-model boundary. T3+ (Home, message-derived) sites must be stored and served as COARSE GEOHASH ONLY — never raw lat/lon on disk or over the wire. Client-side redaction is defense-in-depth, not the primary control. State this as a hard invariant, not an overlay flag.
  - Add an explicit site-formation provenance trail: a `Site` must record which raw observations (calendar event ids / KB refs) collapsed into it, so the rounding-based `SiteId` dedup is auditable and does not silently unify distinct observations. This closes the do-not-unify gap the SiteId hash opens.
  - Trim the v1 wire type to what the KB actually produces: drop `FinanceLocation` and `MessageDerived` from `SiteKind` and the finance/message OverlayValue variants until that data really flows through a tenant. Keep the enum non-exhaustive (Rust `#[non_exhaustive]`) so it can grow without a breaking wire change.
  - Defer deck.gl to Phase 3. Phase 1-2 use native MapLibre markers/sources (adequate for <~1k points). Introduce deck.gl only when H3/hex-bin aggregation is genuinely needed — this also removes a heavy dependency from the earliest, riskiest integration.
  - Constrain weather overlays: home-anchor weather only in Phase 1-2; weather-on-geocoded-sites (Phase 3+) must be explicitly gaps-bearing with a documented US-only / rate-limit failure mode. Do not present per-site weather as free.
  - Surface the metaphor reframe to the user as an explicit decision, not a resolved default. The user literally asked for a 'virtualized globe'; the design answers with a 2D-that-globes map. That is the honest call for current data — but Open Decision #3 should be foregrounded: 'You asked for a globe; at your real data scale (one local cluster) a zoomable map is the honest v1 and the globe is a Phase-4 projection toggle — confirm, or say if a globe demo is needed THIS week for the showcase.'
  - Correct the commit count to 18 in the PR section. Otherwise ship the PR/backup decision as-is — it is sound and correctly decoupled from the globe.
- **Verdict:** SOUND on the big structural calls — ROSA L2 placement, separate repo (correctly refusing the monorepo merge), MapLibre+deck.gl as the eventual virtualization engine, typed SiteCollection over raw GeoJSON, federation-not-merge for overlays, and the immediate 'GO push the routines crate to a private remote now' backup decision (verified: clean tree, no remote, Cargo.lock present-but-gitignored so un-gitignoring is correct). The provenance-reuse claim is real: kb.rs already exposes the Contribution/Provenance types the design reuses. NOT READY as written because of one load-bearing flaw and several leaks: Phase 1 is not thin — it bundles a new Rust R1 capability, a new persisted type, a net-new MCP server, and a new React+GPU repo before drawing a single point, when the thin vertical (render the one home coordinate already in the crate, no geocoder) is right there. Fix that plus the data-layer privacy leak (raw T3 coords persisted to disk), the SiteId silent-merge audit gap, the weather-per-site cost/US-only reality, and trim the speculative finance/message enums. With those adjustments it is ready for the user; ship the PR/backup step immediately and independently regardless.
