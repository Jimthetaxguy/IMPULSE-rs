# Impulse — Design System & Implementation Spec

> **"Your AI remembers. Silently."**
> Distilled from the TUI/GUI exploration set. This is the contract for every future screen.

---

## 1. Core Tension (read first)

Impulse lives between two opposing brand promises. Every design decision negotiates these:

| Cockpit Confidence | × | Silent Memory |
|---|---|---|
| Operator feels in command | | Memory works without demanding attention |
| HUD chrome, glyph density, status everywhere | | Quiet, ambient, out-of-the-way unless asked |
| `cargo run -- run` — multi-pane operator workbench | | Background daemon — invisible 99% of the time |

**Rule:** the *home/idle* state leans Silent. The *operator/diagnostic* states earn Cockpit density.

---

## 2. The Six Principles

### P01 · One Focal Point Per Screen
Each screen earns one hero. Everything else is supporting cast.
- **DO** use scale + color contrast to pick the hero; reserve `--cyan` saturation for it.
- **DON'T** stack 4 brand marks (logo + ship + wordmark + tagline); don't render 6 cyan panels of equal weight.

### P02 · Progressive Disclosure
Show the headline. Tease the detail. Reveal the rest only when asked.
- **DO** number-first cards; peeking drawers (`▾ recent activity (8)`); keyboard hints rendered tiny.
- **DON'T** render every JSONL field on the dashboard; treat the home screen as a database admin view.

### P03 · Density by Mode
Three modes, density follows mode (not surface).
- **Calm** = home/idle (V1b cockpit, V4b workbench)
- **Operator** = active multi-agent work (V2 tmux workbench)
- **Diagnostic** = debugging (full V1 dense HUD, V5 memory map)

### P04 · Quiet by Default, Loud on Signal
Everything is dim/mute until a real signal arrives.
- **DO** dim subsystem rows when "all green"; reserve `--amber` border only for pending review; use `--red` only for true blocks.
- **DON'T** color every label cyan because cyan is brand; don't animate things that aren't actively changing.

### P05 · Chrome Serves, Never Shouts
Brackets, scanlines, ASCII frames are texture — they tile under content; they never compete.
- **DO** scanlines at ≤4% opacity; bracket corners only on hero containers; ASCII rocket as identity, not filler.
- **DON'T** wrap every panel in HUD brackets; don't repeat the rocket sprite on multiple screens.

### P06 · Three Faces, Three Jobs
- **JetBrains Mono** — tokens, IDs, paths, numbers, time
- **Inter** — sentences and headlines (GUI only)
- **Press Start 2P** — wordmark moments only

---

## 3. Information Hierarchy — 4 Tiers

Every element must be assigned to exactly one tier. Move ties down.

| Tier | Label | Color | When |
|---|---|---|---|
| **1** | HERO | `--cyan` full | The single most important thing right now. One per view, max. |
| **2** | SUMMARY | `--fg-0` | Three to four supporting numbers/chips that frame the hero. |
| **3** | PEEKING | `--fg-1`/`--fg-2` | Detail teased behind chevron, count, or drawer. |
| **4** | AMBIENT | `--fg-2`/`--fg-3` | Chrome — title bar, status bar, scanlines, brackets. |

---

## 4. Density Modes — When to Use Which

| Mode | Trigger | Layout | Examples |
|---|---|---|---|
| **Calm** | App launched · idle · `< 1` pending action | Hero + 3 cards + 1 peeking row | V1b cockpit, V4b workbench |
| **Operator** | Active session · ≥2 agents working | Sidebar + multipane + status line | V2 tmux workbench, V6 board |
| **Diagnostic** | Debug · context > 80% · errors | Full HUD · all panels · log feed | V1 cockpit, V5 memory map |

The same screen can shift modes — e.g. the home dashboard goes Calm → Diagnostic when context pressure crosses a threshold.

---

## 5. Design Tokens

### Color (oklch where accent)

```css
/* Backgrounds */
--bg-0:        #070d12;  /* void */
--bg-1:        #0d1820;  /* panel */
--bg-2:        #0f1820;  /* raised */
--bg-3:        #142028;  /* hover */

/* Foreground */
--fg-0:        #d6f3ff;  /* primary */
--fg-1:        #8fb8c8;  /* secondary */
--fg-2:        #5d8090;  /* label */
--fg-3:        #3a5562;  /* dim */

/* Accents — share lightness/chroma, vary hue */
--cyan:        oklch(0.82 0.14 215);  /* HERO / brand */
--blue:        oklch(0.74 0.15 255);  /* secondary signal */
--amber:       oklch(0.84 0.13 78);   /* pending action */
--green:       oklch(0.82 0.16 145);  /* healthy */
--magenta:     oklch(0.72 0.17 330);  /* notable signal */
--red:         oklch(0.72 0.18 25);   /* blocked / error */

/* Borders */
--border:        rgba(120, 220, 255, 0.22);
--border-strong: rgba(120, 220, 255, 0.45);
```

### Type

| Token | Family | Weights | Use |
|---|---|---|---|
| `--font-mono`  | JetBrains Mono | 400 / 500 / 700 | data, code, time, IDs |
| `--font-ui`    | Inter           | 400 / 500 / 600 / 700 | prose & headlines (GUI) |
| `--font-pixel` | Press Start 2P  | 400 | wordmark only |

| Role | Size | Weight | Family |
|---|---|---|---|
| Hero numeric | 36–52 px | 300–400 | mono |
| Body numeric | 13–14 px | 400 | mono |
| Label / eyebrow | 10–11 px / `letter-spacing: 0.20em` / uppercase | 500 | mono |
| GUI headline | 22–36 px | 300–400 | Inter |
| Body prose | 13–14 px | 400 | Inter |

### Space & Grid

- **Grid unit:** 8 px
- **Card padding:** 20 px (TUI) / 22 px (GUI)
- **Screen margin:** 22–32 px (TUI) / 40–56 px (GUI)
- **Border radius:** 0. Always. (No rounded corners — see brand DNA.)

### Motion

- `blink` — terminal cursor only · 1.05s steps(2)
- `pulse` — 1.6s ease-in-out · only on active streaming indicators
- No hover bounces, no fade-ins on idle UI.

---

## 6. Component Library — What to Implement

### TUI · ratatui (`impulse-rs/src/ui/`)

| # | Component | Status | Notes |
|---|---|---|---|
| T1 | **Pixel-grid wordmark** | NEW | 5×5 pixel font for `IMPULSE`. Splash + identity moments only. |
| T2 | **ASCII rocket sprite** | NEW | 21-col chunky monospace, exhaust shaded magenta→amber. Splash only. |
| T3 | **HUD bracket frame** | NEW | Corner brackets (┌ ┐ └ ┘) + dashed edges. For hero containers. |
| T4 | **Calm card** | NEW | Title-eyebrow + big number + sub + footer divider. Tier 2. |
| T5 | **Bar/track meter** | EXISTS | Reuse for memory budget, agent load. |
| T6 | **Memory stream column viz** | NEW | Vertical sparklines, tile-based. Diagnostic mode only. |
| T7 | **Agent network panel** | EXISTS | Refactor to single-line rows w/ status dots. |
| T8 | **Event log feed** | EXISTS | Add color coding by severity. |
| T9 | **Status line** | EXISTS | Cyan-on-black bottom bar. Tier 4. |
| T10 | **Tab strip** | EXISTS | Number-prefixed; active tab highlights. |
| T11 | **Pending review row** | NEW | Single peeking row · amber accent · `[a/d/s]` hints. |
| T12 | **Pixel cursor block** | EXISTS | Blinking 8×14 cyan block. |

### Desktop · Dioxus (`impulse-rs/impulse-desktop/`)

| # | Component | Status | Notes |
|---|---|---|---|
| G1 | **Title bar w/ traffic lights** | EXISTS | Add daemon RTT + protocol version chip. |
| G2 | **Left view rail (icon)** | EXISTS | Dioxus view spine; active = cyan border-left + bg highlight. |
| G3 | **Hero block (rocket + headline)** | NEW | Brand moment for home/supervisor views only. |
| G4 | **Pending action banner** | NEW | Amber border-left · single primary button (`REVIEW & APPLY`). |
| G5 | **Stat trio strip** | NEW | Three columns w/ vertical dividers · 38 px numbers. |
| G6 | **Stat card** | EXISTS | Title-eyebrow + big number + meter. |
| G7 | **Signal history list** | EXISTS | Time · type-tag · message · color dot. |
| G8 | **Subsystem table** | EXISTS | Two-column key/value with status dots. |
| G9 | **Genome graph** | NEW | SVG force-layout · nodes by type · selected ring. |
| G10 | **Injection diff preview** | NEW | Diff-style colored lines (`+ ~ -`). |
| G11 | **Genome growth timeline** | NEW | 30-day bar chart. |
| G12 | **Agent kanban column** | NEW | QUEUE → IN FLIGHT → REVIEW → DONE. |
| G13 | **Task card** | NEW | Tag · title · agent · meta chips · accent border-left. |
| G14 | **Agent chip w/ load bar** | NEW | Name + status + 4-px load meter. |
| G15 | **Handoff prep card** | NEW | From → To · token count · `[SEND] [EDIT]` row. |
| G16 | **Steward proposal** | NEW | Amber border · approve / reject. |
| G17 | **Drawer / disclosure row** | NEW | `▾ label (count)` · expands inline. |
| G18 | **Status bar (bottom)** | EXISTS | Daemon health · session counters. |
| G19 | **Toast / signal notification** | NEW | Briefly surfaces signal-bus events. |

---

## 7. The 4-Step Review Checklist

Run this on every new screen *before* code:

1. **Pick the one hero.** If you can't, the screen has no purpose yet — go back.
2. **Assign every other element to Tier 2, 3, or 4.** Move ties down.
3. **Color audit.** Count cyan elements. If `> 3`, demote some to dim/mute.
4. **Choose density mode** (Calm / Operator / Diagnostic) and stick to it for that screen.

---

## 8. Anti-Patterns (do not ship)

- Six cyan panels of equal weight → flatten hierarchy
- Decorative scanlines + bracket corners + glow + dashed borders all on one card
- Body text in pixel font
- Numbers in proportional fonts (mono only)
- Multiple rocket sprites in one view
- Toasts for every signal — only `Block` and `ContextThreshold > 80%`
- Hover animations on idle UI
- Rounded corners anywhere (border radius is `0`)
- Random saturated hues outside the 6-token accent palette

---

## 9. Implementation Order (suggested)

**Phase 1 — Tokens & primitives** (1–2 days)
- Encode color/type/space tokens in `impulse-rs/src/branding.rs` (TUI) and `impulse-gui/src/theme.rs` (GUI)
- Implement T1, T2, T3, T11 (TUI splash & calm primitives)
- Implement G3, G4, G5, G17 (GUI hero, banner, stats, drawer)

**Phase 2 — Calm mode home** (3–5 days)
- TUI home view (V1b) replacing V1
- GUI home view (V4b) replacing V4
- Wire density-mode auto-switching (Calm by default, escalate on signal)

**Phase 3 — Operator mode**
- T7/T8/T9/T10 polish for V2 tmux workbench
- G12–G16 for V6 orchestration board

**Phase 4 — Diagnostic mode**
- T6 memory-stream viz
- G9–G11 genome graph + diff + timeline
- Toast / signal-bus surface (G19)

---

## 10. Live Artifacts

- **Canvas:** `Impulse Interface Explorations.html` — all six directions side-by-side
- **Tokens (CSS):** `impulse.css`
- **ASCII rocket:** `rocket-sprite.jsx`
- **TUI explorations:** `v1-tui-cockpit.jsx`, `v1b-tui-cockpit-calm.jsx`, `v2-tui-tmux.jsx`, `v3-tui-minimal.jsx`
- **GUI explorations:** `v4-gui-workbench.jsx`, `v4b-gui-workbench-calm.jsx`, `v5-gui-memory.jsx`, `v6-gui-board.jsx`
- **Principles artboard:** `v7-principles.jsx`

---

---

## 11. Retro Broadcast Mode (70s–80s CRT) — Optional Skin

A second visual lane for splash / boot / brand moments, drawn from the Scanimate-era
references (NEW YORK, Avant Garde, oasis, Aperture Science). **Not** for dense data screens —
the bloom destroys legibility. Use it for: app launch, the `cargo run -- run` splash,
marketing, the "online · watching · remembering" boot card. Live data inside a CRT frame
stays in calm cyan phosphor (P04: loud on signal only).

### The four non-negotiables (all references share these)
1. **Pure black** background (`#000`) — not near-black.
2. **Hot saturated fills** — amber/orange/blue, no pastels.
3. **Heavy bloom** — bright near-white core, saturated color halo (text-shadow stack).
4. **Aperture-grille striping** — fine *vertical* dark lines (≈2px), faint horizontal scanlines on top.
Plus: bold reduced geometry that survives blur, subtle vignette + barrel via inset shadow.

### Phosphor palette

```css
--p-black:   #000000;   /* the screen */
--p-amber:   #ffb01a;   /* core wordmark */
--p-amber-h: #ffe39a;   /* hot bright center */
--p-orange:  #ff6a00;   /* edge bleed */
--p-red:     #ff3b1f;   /* deep edge */
--p-blue:    #5b63ff;   /* periwinkle (NY skyline / structure) */
--p-cyan:    #2fd0ff;   /* live data inside CRT */
--p-teal:    #2fd6a8;   /* aperture green */
--p-lime:    #b6f03c;   /* OK / healthy */
--p-magenta: #ff3d81;   /* notable */
--p-yellow:  #ffd23f;   /* secondary hot */
```

### Bloom recipe (text)

```css
.phos-amber {
  color: #ffe39a;                 /* core lighter than fill */
  text-shadow:
    0 0 1px #fff,
    0 0 4px  #ffb01a,
    0 0 10px #ff6a00,
    0 0 24px #ff6a00,
    0 0 52px #ff3b1f;             /* halo bleeds to deep red */
}
```

### Aperture-grille overlay

```css
/* vertical phosphor stripes — the dominant artifact */
repeating-linear-gradient(90deg,
  rgba(0,0,0,0) 0 1px, rgba(0,0,0,0.55) 1.6px 2px);   /* mix-blend: multiply */
/* fainter horizontal scanlines layered on top */
repeating-linear-gradient(0deg,
  rgba(0,0,0,0) 0 2px, rgba(0,0,0,0.18) 3px 3.5px);
```

### Brand wordmark
- Face: **Baloo 2 800** (heavy rounded) — matches the oasis/aperture lowercase warmth.
- Set lowercase `impulse` in `--p-amber` phosphor. Tagline in `--p-cyan`, `letter-spacing: 0.4em`, uppercase.
- Emblem: aperture-iris ring (8 angled blades cycling orange→blue→teal→cyan) with the rocket ascending through the center.

### Type addition

| Token | Family | Weights | Use |
|---|---|---|---|
| `--font-broadcast` | Baloo 2 | 500 / 700 / 800 | retro wordmark + CRT headlines only |

### Component additions

| # | Component | File | Notes |
|---|---|---|---|
| R1 | **CRT screen wrapper** | `crt.css` `.crt` | black + vignette + inset barrel |
| R2 | **Aperture-grille overlay** | `crt.css` `.grille` | vertical + horizontal scanlines |
| R3 | **Phosphor text classes** | `crt.css` `.phos-*` | amber/blue/cyan/lime/yellow bloom |
| R4 | **Shape bloom filters** | `crt.css` `.glow-*` | drop-shadow stacks for svg/boxes |
| R5 | **Scan sweep** | `crt.css` `.scan-sweep` | slow moving highlight band (motion-gated) |
| R6 | **Flicker** | `crt.css` `.flicker` | subtle global opacity jitter (motion-gated) |
| R7 | **Aperture-iris + rocket emblem** | `v8-retro-brand.jsx` | the brand mark |
| R8 | **CRT boot card** | `v9-retro-boot.jsx` | `[OK]` checklist + calm stats + one loud pending bar |

**Accessibility:** all motion (`scan-sweep`, `flicker`) is gated behind `@media (prefers-reduced-motion: no-preference)`. The bloom never carries meaning — it is decoration over content that is also legible without it.

### Live artifacts (retro)
- `crt.css` — the effect engine
- `v8-retro-brand.jsx` — broadcast logo lockup + phosphor palette + bloom breakdown
- `v9-retro-boot.jsx` — vibe applied to the product boot/home screen

---

*v0.2 · 2026-06-13 · for impulse-rs · adds Retro Broadcast mode*
