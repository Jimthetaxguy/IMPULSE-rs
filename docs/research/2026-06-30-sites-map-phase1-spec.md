---
title: ROSA Sites-Map — Phase 1 (rescoped, canonical spec)
description: The thin vertical for the "map-that-globes with per-site metadata overlays" feature — render the one home site already in the routines KB, provenance-first, no geocoder
version: '0.1'
updated: 2026-06-30
type: research
category: architecture
status: approved-for-planning
tags: [rosa, sites-map, maplibre, provenance, phase1, globe]
---

# ROSA Sites-Map — Phase 1 (rescoped, canonical)

> Supersedes §7 of `2026-06-30-rosa-sites-globe-view-scoping.md` (the scoping doc + adversarial
> critique). User decisions (2026-06-30): **map-that-globes** (not a literal 3D globe) for v1; `routines`
> pushed to a private remote; **write this Phase-1 plan**. This spec folds the critique's rescope.

## 1. Goal (thinnest real vertical)

Prove the whole seam — **routines KB → typed site data → frontend → provenance rendered in the UI** —
using the **one real geocoded point the crate already has: home (home site (geohash at export; never raw lat/lon in docs))
with its live weather.gov overlay**. No geocoder, no new R1 capability, no deck.gl, no server. Success:

- `routines sites --tenant james [--date YYYY-MM-DD]` emits a typed `SiteCollection` JSON derived from
  the KB (home site + its weather overlay + provenance), with the home coordinate **coarsened to a
  geohash** at the export boundary (never raw lat/lon on disk/wire for the private home site).
- A new React+TS app (`sites-map`) loads that JSON, renders the home site as a MapLibre v5 marker on a
  flat map that can globe-out on zoom, and on click shows a **detail card rendering the overlay's
  provenance verbatim** (capability slug, provider, `source_version`, `recorded_at`, `gaps`).
- No mock data anywhere in the shipped path (`real-systems-only`): the site + weather come from a real
  daily-brief KB entry.

## 2. Architecture (Phase 1 = local-first, no server)

```
routines crate (Rust)  ── new `sites` subcommand ──▶  SiteCollection JSON (stdout / file)
   reads kb/<tenant>/<date>.md (existing) + config home; reuses kb::{Contribution,Provenance}
   coarsens home coord → geohash at the export boundary (privacy invariant)
                                   │
                                   ▼ (Phase 1: static JSON; Phase 2+: MCP `sites.read` over HTTP → Railway)
sites-map (React + TS + MapLibre GL v5)   loads SiteCollection → 1 GeoJSON source → marker + detail card
```

- **Placement:** the map is a **ROSA-layer frontend in its own repo** (`sites-map`), NOT inside the
  `routines` crate. The crate stays a backend *producer*; the map *consumes* its export.
- **Read-model deferral:** Phase 1 uses a **static JSON export** (a `sites` subcommand) — thinner than a
  server and enough for local dev. The typed **MCP `sites.read` over Streamable HTTP (R3 seam),
  hostable on Railway**, is **Phase 2** (its own gate; the crate has no server surface today).
- **No geocoder in Phase 1.** `geocode.resolve` (calendar-location string → lat/lon) is the risky,
  failure-prone part and is **Phase 3** — where its `gaps[]` / "unplaced sites" tray is the point.

## 3. Data model (v1, trimmed — `#[non_exhaustive]`)

```rust
struct SiteCollection { tenant: String, date: String, sites: Vec<Site> }   // one tenant per collection
#[non_exhaustive] enum SiteKind { Home, CalendarEventLocation, KbPlace }    // NO finance/message in v1
struct Site {
    id: String,                    // stable id; carries tenant; see site-formation trail below
    kind: SiteKind,
    geohash: String,               // COARSE for private (Home = T3); never raw lat/lon for T3+ on wire
    label: String,
    formed_from: Vec<String>,      // raw observation refs that collapsed into this site (do-not-unify audit)
    overlays: Vec<OverlayLayer>,   // a FEDERATION — never merged/averaged
}
struct OverlayLayer {
    capability: String,            // e.g. "weather.forecast"
    provider: Option<String>, account_id: Option<String>,
    value: OverlayValue,           // typed; Phase 1: Weather { temp_f, short_forecast, precip_prob }
    encoding_hint: String,         // "color-scale" | "icon" | "size" | "heatmap" (frontend hint)
    provenance: Provenance,        // reused verbatim from kb.rs: source_version, recorded_at, gaps
}
```

## 4. Invariants (enforced across the seam)

- **Provenance per overlay** (reuse `kb::Provenance`); the detail card renders it as a first-class UI
  element, not a debug affordance. **Federation, never merge** (`do-not-unify`): two sources = two
  `OverlayLayer`s.
- **Privacy at the persistence/export boundary:** the home site (T3) is exported as a **coarse geohash
  only** — raw lat/lon never leaves the crate for private sites. (Client-side redaction is defense in
  depth, not the primary control.)
- **Site-formation trail:** `Site.formed_from` records which raw observations became this site, so any
  future dedup can't silently unify distinct observations. (Phase 1 home is 1:1 — trivial but the field
  ships now so the contract exists.)
- **Multi-tenant:** tenant on the `SiteCollection` envelope and embedded in each `Site.id`; one tenant
  per export.

## 5. Components / file structure

**routines crate (small additions):**
- `src/sites.rs` — `SiteCollection`/`Site`/`OverlayLayer`/`SiteKind`/`OverlayValue`; `fn project(kb_entry, config) -> SiteCollection` (home site + weather overlay); geohash coarsening helper.
- `src/main.rs` — new `sites` subcommand (`--tenant`, `--date`, tenant-tz default like `query`).
- Dep: a geohash crate (e.g. `geohash`) — verify current version at build time.

**sites-map (new React+TS repo):**
- Vite + React + TS; `maplibre-gl` v5 (verify version live). No deck.gl.
- `src/SiteMap.tsx` (MapLibre map, flat→globe projection on zoom), `src/DetailCard.tsx` (provenance render), `src/types.ts` (mirror SiteCollection), `src/load.ts` (fetch the JSON export).
- A demo open basemap style (MapLibre demotiles or a free style — no token).

## 6. Out of scope for Phase 1 (deferred, tracked)

Geocoder (`geocode.resolve`, Phase 3) · deck.gl aggregation/hex-bin/virtualization (Phase 3, when data
scale needs it) · literal 3D globe as default (Phase 4 projection toggle) · per-site weather beyond the
home anchor (weather.gov is US-only + 2 calls/point — Phase 3+, explicitly gaps-bearing) · MCP/HTTP
`sites.read` server + Railway hosting (Phase 2) · finance/message site kinds & overlays.

## 7. Phasing (dependencies, not time)

1. **Phase 1** (this plan): `sites` export subcommand → static JSON → MapLibre map of the home site + provenance detail card.
2. **Phase 2:** wrap the export as a typed MCP `sites.read` over Streamable HTTP (R3); host on Railway; frontend reads live.
3. **Phase 3:** `geocode.resolve` sensor (calendar locations → sites, gaps/unplaced tray) + deck.gl aggregation once multiple sites exist.
4. **Phase 4:** globe projection toggle; time scrubber; richer overlay encodings.

## 8. Open decisions (defaults chosen; confirm if you disagree)

1. **Repo home for `sites-map`** — standalone new private repo (default) vs a package inside `ROSA_RenewBuild`.
2. **Railway** — Phase-1 is local-first (no deploy); Railway hosting enters at Phase 2. (Confirm this is what "our railway app" meant, or if you want a hosted read-model sooner.)
3. **Basemap style** — MapLibre demotiles/free open style for v1 (no token); revisit if you want a specific cartography.
