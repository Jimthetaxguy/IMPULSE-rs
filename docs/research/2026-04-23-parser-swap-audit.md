# Parser Swap Audit: vt100 → alacritty_terminal

**Status:** Research-only spike (Ralph Plan 7, L178). No code changes.
**Author:** Impulse maintainers
**Date:** 2026-04-23

## Why this audit

`impulse-term-core` currently uses [`vt100`](https://crates.io/crates/vt100) (v0.15)
as its VT/ANSI parser. vt100 is small, dependency-light, and has served the
streaming-grid path well — but it has known limitations that may bite us as the
Dioxus renderer matures and we onboard heavier shell-integration workloads.

This document audits whether swapping to
[`alacritty_terminal`](https://crates.io/crates/alacritty_terminal) — the parser
extracted from the Alacritty terminal emulator — is worth the migration cost.

This is a **decision-prep doc**, not a decision. It lays out tradeoffs so a
future loop can make the call with eyes open.

## Current state: vt100

**What we use it for** (`impulse-term-core/src/backend.rs`,
`impulse-term-core/src/grid.rs`):

- `vt100::Parser` — feeds raw PTY bytes, produces a row/col grid of cells
- `vt100::Screen` — read access to the grid for `GridSnapshot::from_screen`
- `vt100::Color` — converted to our toolkit-neutral `TermColor`
- Bold-bright + inverse-video promotion logic in `from_screen`

**Strengths:**

- Tiny (~3 kLOC), one author, no transitive dep zoo
- Stable API since 2020; we've never been blocked by a vt100 bug
- Semver-compatible upgrades have been painless

**Known limitations:**

- **No native scrollback** — vt100 holds only the visible grid. Scrollback is
  the host's responsibility. We've punted on this; the Dioxus renderer needs
  scrollback eventually for "scroll up to see earlier output."
- **Limited mouse-protocol support** — handles the common cases (X10, SGR) but
  doesn't expose the full state machine. Not a blocker today.
- **No first-class hyperlink (OSC 8) support** — we'd need to parse it
  ourselves out of the byte stream, parallel to how we already do OSC 133.
- **Sixel / kitty-graphics: not supported** — vt100 silently drops them.
  Probably acceptable for an agent sidecar; a problem if we ever want to render
  matplotlib-style inline images.
- **No semantic regions / "shell prompt aware" hooks** — we built OSC 133 on
  top by tapping the byte stream before vt100 sees it.

## Candidate: alacritty_terminal

**What it is:** the parser + grid + scrollback engine extracted from the
Alacritty terminal emulator. Used in production by Alacritty itself, by Zed's
terminal, and (with adapters) by a number of other Rust GUI terminals.

**Strengths:**

- **Built-in scrollback** with a configurable line buffer. We'd get
  scroll-up-to-see-history nearly for free.
- **Mature OSC handling** — OSC 8 (hyperlinks), OSC 52 (clipboard), OSC 10/11
  (default fg/bg) all supported.
- **Rich event API** — `EventListener` trait fires on title change, cursor
  shape change, bell, clipboard request, etc. Today we sniff some of these out
  of the byte stream; with alacritty_terminal we'd subscribe.
- **Sixel support** in recent versions (gated behind a feature).
- **Performance** — Alacritty is a 60+ FPS GPU terminal; the parser is tuned
  for streaming workloads with vte's state machine.
- **Battle-tested** on every major shell + tmux + vim/neovim + emacs combination
  anyone has thrown at Alacritty.

**Costs / risks:**

- **Bigger dep graph.** alacritty_terminal pulls in `vte`, `unicode-width`,
  `bitflags`, `serde`, `regex`, `log`, `parking_lot`, etc. We have most of
  these already, but the transitive footprint roughly triples.
- **API churn.** alacritty_terminal is treated as an internal crate by the
  Alacritty project; minor versions can break consumers without ceremony.
  Several downstream projects pin to a specific commit hash rather than a
  semver range.
- **Less obvious "snapshot" pattern.** vt100 makes it trivial to copy out a
  read-only grid view (`Screen`). alacritty_terminal's grid is owned by the
  `Term` and accessed through iterators / `RenderableContent`. We'd have to
  rewrite `GridSnapshot::from_screen` against a different shape.
- **Sync model is different.** vt100 is synchronous: feed bytes, read grid.
  alacritty_terminal expects an `EventListener` and assumes it owns more of
  the event loop. We'd need to adapt.
- **`.impulse/` blocks model coupling.** Our `BlockStore` (built in L168) is
  driven from the OSC 133 byte tap, which sits *before* the parser. That part
  doesn't change. But scrollback semantics — if alacritty_terminal owns
  scrollback and we own block grouping, we'd need to decide whose row indexing
  wins.

## What would actually break

A migration touches these surfaces:

| Surface | vt100 today | alacritty_terminal | Migration cost |
|---|---|---|---|
| `TerminalBackend::with_parser` | exposes `&Parser` | would expose `&Term<L>` | medium — generic over listener |
| `GridSnapshot::from_screen` | walks `vt100::Screen` rows/cells | walks `Term`'s renderable iter | medium — shape differs |
| `TermColor::From<vt100::Color>` | exhaustive match | would be `From<alacritty_terminal::vte::ansi::Color>` | low |
| OSC 133 byte tap (`Osc133Parser`) | unaffected (sits before parser) | unaffected | none |
| Block model / `.impulse/` log | driven by OSC 133, not parser | unaffected | none |
| Tests (`from_screen`, snapshots) | use vt100 to seed the grid | rewrite seeding, keep assertions | medium |
| `impulse-term` (egui adapter) | reads `GridSnapshot` only | unaffected | none |
| `impulse-term-dioxus` | reads `GridSnapshot` only | unaffected | none |

The good news: because `impulse-term-core` already produces a toolkit-neutral
`GridSnapshot` (Phase 1's payoff), the renderers don't care which parser
produced it. The migration is contained to the core.

## Decision criteria

Recommend swapping **if and only if** we hit one of:

1. **Scrollback becomes a first-class feature requirement.** If users need to
   scroll up through 10k lines of agent output, building scrollback on top of
   vt100 ourselves is ~1k LOC and a maintenance burden. alacritty_terminal
   gives it for free.
2. **Hyperlink (OSC 8) support is requested.** Modern shells (`fish`, `zsh`
   with `oh-my-posh`) emit OSC 8 hyperlinks for file paths and URLs. vt100
   drops them; alacritty_terminal renders them.
3. **A real terminal compatibility bug hits vt100** that won't be fixed
   upstream within a release cycle.

Recommend **staying on vt100** if:

1. Scrollback can live in the *renderer* (Dioxus virtualized list of finished
   blocks from `BlockStore`), which is the current direction.
2. We don't need OSC 8 / sixel / kitty-graphics in the foreseeable future.
3. Dep-graph minimalism continues to be a stated value.

## Recommendation

**Defer the swap.** vt100 + our OSC 133 byte tap covers the agent-sidecar use
case. The Dioxus renderer can build "scrollback" by virtualizing the
`BlockStore` (each finished block is already an addressable unit), which is a
better UX than raw line scrollback anyway — you scroll through *commands*, not
*lines*.

Revisit when:

- Users complain about losing output above the visible grid (real scrollback)
- A specific OSC 8 / OSC 52 / sixel bug report lands
- The renderer needs cursor-shape or title events we currently can't see

## Migration sketch (if/when we do swap)

A future loop should:

1. Add `alacritty_terminal = "<pinned-version>"` to `impulse-term-core` behind
   a `parser-alacritty` feature flag.
2. Implement a `ParserBackend` trait that both `vt100::Parser` and a new
   `AlacrittyParser` adapter satisfy. Keep `vt100` as the default for one
   release.
3. Run the existing `from_screen` test suite against both parsers; diff the
   `GridSnapshot` outputs on a corpus of recorded PTY bytes.
4. Flip the default in a separate release; remove the feature flag a release
   later.

Estimated cost: 4–6 focused loops once the trigger fires.

## References

- vt100 repo: <https://github.com/doy/vt100>
- alacritty_terminal: <https://github.com/alacritty/alacritty/tree/master/alacritty_terminal>
- VTE state machine: <https://vt100.net/emu/dec_ansi_parser>
- OSC 133 (semantic prompts): <https://gitlab.freedesktop.org/Per_Bothner/specifications/-/blob/master/proposals/semantic-prompts.md>
- OSC 8 (hyperlinks): <https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda>
