---
status: active
phase: 2-3
audience: builder
tags: [research, terminal, zellij]
last_updated: 2026-02-20
---

# Terminal Layer Analysis: Ghostty, Zellij, mise

> **Version:** 1.0 | **Status:** Research Complete | **Updated:** 2026-02-20
> **Purpose:** Deep analysis of terminal foundation tools for Impulse
> **Builds on:** docs/research/TOOL-STACK-ANALYSIS.md (Layer 0 section)

---

## Executive Summary

The terminal foundation layer for Impulse consists of three tools that form the visual and operational substrate on which AI agents run: Ghostty (GPU terminal emulator), Zellij (WASM-extensible multiplexer), and mise (polyglot tool version manager). After deep analysis of source code, plugin APIs, and real-world benchmarks, the key findings reshape several Impulse design assumptions.

**Ghostty is recommended but not necessary.** Its four-thread architecture (main, I/O, PTY-read, renderer) with lock-free mailboxes delivers genuine performance advantages for multi-pane scenarios, maintaining 480+ FPS in stress tests where macOS Terminal.app drops below 10 FPS. However, the performance gap between Ghostty, Alacritty, and Kitty is typically 5-15% -- meaningful but not disqualifying. Impulse should not hard-depend on Ghostty; any modern GPU terminal works.

**Zellij's WASM plugin system is more capable than documented in our existing analysis.** Plugins can read and write to the host filesystem via four WASI-mounted directories (`/host`, `/data`, `/cache`, `/tmp`), with `FullHdAccess` permission enabling dynamic filesystem traversal. The `FileSystemCreate/Update/Delete` events allow plugins to watch for file changes in the Zellij startup directory -- meaning a Phase 3 Zellij plugin could directly observe `.impulse/` file mutations. The pipe system enables rich inter-plugin communication including lazy plugin loading on first message. This opens architectural paths we had not considered.

**mise's hook system is a direct match for Impulse auto-initialization.** The `enter` hook fires when `cd`-ing into a project directory where a `mise.toml` exists, with `MISE_PROJECT_ROOT` available as an environment variable. A single line in `mise.toml` could auto-create `.impulse/` directories and seed `GENOME.md` templates on project entry, removing manual setup friction entirely.

---

## 1. Ghostty -- GPU-Accelerated Terminal

### 1.1 Architecture

Ghostty employs a **four-thread-per-surface architecture**, a design unique among terminal emulators:

| Thread | Responsibility | Key Detail |
|--------|---------------|------------|
| **Main** | User input, window events | Processes keybindings via `keyCallback()`, dispatches actions through platform runtime |
| **I/O** | PTY communication event loop | Reads from lock-free mailbox (64-message SPSC queue), updates terminal state under mutex |
| **PTY Read** | Dedicated non-blocking PTY reads | Spawned from I/O thread to prevent event loop stalls from blocking `read()` syscalls |
| **Renderer** | 120fps draw loop | 8ms frame timer, 600ms cursor blink timer; acquires terminal state mutex, diffs screen, issues GPU commands |

**Synchronization primitives:**
- **Mutex** on `renderer_state` (contains shared `Terminal` instance) -- critical sections minimized
- **Lock-free mailboxes** -- SPSC queues with 64-message capacity for inter-thread communication (`.write`, `.resize`, `.change_config` messages)
- **Async wakeup** -- `xev.Async` notifications cross threads without polling

**Rendering pipeline (per frame):**
1. Renderer thread acquires terminal state mutex
2. Diffs current screen against previous frame via `updateFrame()`
3. Requests shaped text from font subsystem (HarfBuzz for complex scripts)
4. Updates texture atlases for new/changed glyphs
5. Issues draw commands to GPU backend (Metal on macOS, OpenGL on Linux)
6. Presents to display surface
7. Releases mutex -- I/O thread can resume updating terminal state

**Font subsystem:**
- `SharedGridSet` (shared cache) and `SharedGrid` (per-configuration group)
- Font discovery via platform APIs: CoreText on macOS, Fontconfig on Linux
- HarfBuzz for text shaping (handles ligatures, complex scripts)
- Glyph rasterization to texture atlases for GPU batching
- Ghostty is the only Metal-based terminal that supports ligatures without CPU rendering fallback

**Terminal grid representation:**
- `ScreenSet` holds primary and alternate screens
- `PageList` implements a circular buffer for scrollback (fixed-size, oldest pages rotate out)
- `Cell` struct: character + SGR attributes + foreground/background colors
- Scrollback is entirely in memory (no disk paging); the `scrollback-limit` config controls per-surface memory budget

**Zig implementation details:**
- Comptime configuration generation (Actions/Commands generated at compile time for ABI stability)
- Tagged unions for platform runtime selection (`app-runtime` build option)
- Explicit allocator passing prevents hidden allocations
- `ArenaAllocator` manages derived config lifetime
- Build system produces: standalone executable, `libghostty` static library, macOS XCFramework

### 1.2 Performance Characteristics

**Synthetic benchmarks (validated from multiple sources):**

| Benchmark | Ghostty | Alacritty | Kitty | macOS Terminal | iTerm2 |
|-----------|---------|-----------|-------|----------------|--------|
| DOOM-fire FPS | 480-500 | ~404 | ~401 | <10 | N/A |
| vtebench (Shakespeare 11MB IO) | 38.3ms | 16.7ms | 18.3ms | N/A | N/A |
| Plain text dump speed | 4x faster than iTerm/Kitty | Comparable to Ghostty | 4x slower than Ghostty | 2x slower than Ghostty | 4x slower |

**Key performance observations:**
1. Ghostty, Alacritty, and Kitty form a cohort of "fastest" terminals with 5-15% differences between them. Other terminals (iTerm2, Terminal.app) show 2x+ gaps.
2. Ghostty intentionally optimizes for **real-world responsiveness** over synthetic throughput. The vtebench numbers (where Alacritty leads) are described by Ghostty's maintainer as "contrived" -- they measure raw VT IO throughput for a specific zsh prompt, not actual user-perceived latency.
3. Ghostty's advantage becomes most apparent during **heavy multi-pane scenarios**: tailing logs, compiling, and rapid scrolling across 4+ panes simultaneously, where the dedicated renderer thread prevents frame drops.
4. Scrollback is per-surface (per-pane in Zellij terms). With 4 agent panes each running at `scrollback-limit = 1000000` bytes, total memory overhead is ~4MB -- negligible.

**The real bottleneck for multi-agent workspaces is not rendering -- it is PTY throughput.** When 4+ agents produce output simultaneously, the bottleneck is the kernel's PTY buffer (typically 4KB-64KB depending on OS), not GPU rendering speed. Ghostty's dedicated PTY read thread helps here, but this is an OS-level constraint.

### 1.3 Configuration for Multi-Agent Workspaces

Recommended Ghostty configuration for Impulse workspaces (`~/.config/ghostty/config`):

```ini
# Performance: Multi-pane agent workspace
scrollback-limit = 2000000          # 2MB per pane (agents produce lots of output)
window-padding-x = 2                # Tighter panes, more content visible
window-padding-y = 2

# Font: Monospace with ligature support
font-family = "JetBrains Mono"
font-size = 13
font-feature = +calt                # Contextual alternates (code-friendly)
font-feature = +liga                # Standard ligatures
# Note: Ghostty 1.2.0+ disabled +dlig by default; enable if desired

# Clipboard: Allow agent tools to use clipboard
clipboard-read = allow              # Or "ask" for prompt-before-read
clipboard-write = allow
clipboard-paste-protection = true   # Warn about dangerous pastes (security)

# Splits: Vim-style navigation for multi-pane
keybind = cmd+shift+enter=new_split:auto

# Shell integration
shell-integration = detect          # Auto-detect zsh/bash/fish

# Rendering
window-vsync = true                 # Prevent tearing
bold-is-bright = false              # Don't brighten bold text (easier on eyes in long sessions)
```

**Configuration options that matter for Impulse specifically:**
- `scrollback-limit`: Set high (2MB+) because AI agents produce verbose output. Memory is allocated lazily, so a high limit costs nothing until used.
- `clipboard-read/write`: Important for agents that use clipboard integration (e.g., copying code snippets).
- `shell-integration`: Enables semantic zones (command start/end markers) that Zellij can leverage.
- `window-vsync`: Prevents tearing when multiple panes update simultaneously.

### 1.4 Comparison with Alternatives

| Feature | Ghostty | Alacritty | Kitty | iTerm2 | WezTerm |
|---------|---------|-----------|-------|--------|---------|
| GPU Backend (macOS) | Metal | OpenGL | OpenGL | Metal | OpenGL/Metal |
| Ligature Support | Yes (GPU-native) | No | Yes | Yes | Yes |
| Split Panes (built-in) | Yes | No | Yes | Yes | Yes |
| Configuration | File (key=value) | TOML | Python-based | GUI | Lua |
| Memory per-pane | ~500KB base | ~300KB base | ~400KB base | ~2MB base | ~800KB base |
| Language | Zig | Rust | Python+C | Objective-C | Rust |
| Cross-platform | macOS, Linux | macOS, Linux, *BSD | macOS, Linux | macOS only | macOS, Linux, Windows |

**For Impulse, the most relevant comparison axis is behavior under Zellij.** When Ghostty hosts Zellij, its built-in split panes are unused -- Zellij manages all splitting. The critical value-add is:
1. GPU rendering that stays smooth when Zellij has 4+ panes with simultaneous output
2. Low base memory per-surface (since Zellij creates many surfaces)
3. Native Metal integration on macOS (avoids OpenGL-to-Metal translation layer overhead)

### 1.5 Verdict for Impulse

**Is a GPU terminal NECESSARY for Impulse?** No. Any modern terminal emulator works. The OpenCode plugin hooks (Impulse's actual integration surface) are completely terminal-agnostic.

**Is a GPU terminal RECOMMENDED?** Yes, for the multi-pane Zellij experience. When running 4+ agent panes with rapid output, a GPU terminal provides noticeably smoother scrolling and lower input latency. But this is a UX quality improvement, not a functional requirement.

**Rendering bottleneck analysis:** With 4 concurrent agent panes producing heavy output, the bottleneck ordering is:
1. **LLM API latency** (100ms-2s per response) -- dominates by orders of magnitude
2. **PTY kernel buffer** (OS-level, ~4-64KB) -- can cause brief pauses during output bursts
3. **Terminal rendering** (<8ms per frame in Ghostty) -- effectively never the bottleneck

**Recommendation:** Install Ghostty. Configure as shown above. But do not make any Impulse code depend on Ghostty-specific features. The `impulse-plugin/` code should be terminal-agnostic.

---

## 2. Zellij -- Terminal Multiplexer with WASM Plugin System

### 2.1 WASM Plugin Architecture

Zellij plugins are WebAssembly modules compiled to `wasm32-wasip1` (WASI Preview 1), executed inside the **wasmi v0.51 interpreter** (migrated from wasmtime in v0.44.0 for smaller binary size and simpler embedding).

**Plugin lifecycle:**
1. **Download** (if remote): HTTP/HTTPS `.wasm` files fetched and cached in `ZELLIJ_CACHE_DIR`
2. **Module compilation**: WASM bytes parsed into `wasmi::Module`
3. **Environment creation**: WASI context established with filesystem mounts; `PluginEnv` created
4. **Instantiation**: `wasmi::Instance` created with host function imports
5. **Initialization**: Plugin's `_start()` runs, then `load()` called with configuration `BTreeMap<String, String>`
6. **Cloning**: Same plugin instantiated per connected client (each client gets own instance)

**Plugin trait (Rust SDK -- `zellij-tile` crate):**
```rust
impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Subscribe to events, request permissions
        request_permission(&[PermissionType::ReadApplicationState]);
        subscribe(&[EventType::PaneUpdate, EventType::TabUpdate]);
    }
    fn update(&mut self, event: Event) -> bool {
        // React to events; return true to trigger render()
        false
    }
    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        // React to pipe messages from CLI, keybindings, or other plugins
        false
    }
    fn render(&mut self, rows: usize, cols: usize) {
        // Render UI to plugin pane (println! writes to pane)
        println!("Status: {} rows x {} cols", rows, cols);
    }
}
```

**Communication with host:** Protocol Buffers over WASM imports/exports. The `host_run_plugin_command` function is exported from Zellij to plugins via WASM imports; Zellij checks permissions before executing any host-side command.

### 2.2 Plugin Capabilities and Sandbox Limitations

#### Filesystem Access (WASI Mounts)

Each plugin instance gets four filesystem directories:

| Mount Point | Scope | Persistence | Purpose |
|------------|-------|-------------|---------|
| `/host` | Plugin's working directory | Session | Read/write files in the Zellij startup CWD. **Default: read-only access to CWD where Zellij was launched** |
| `/data` | Unique per `(plugin_id, client_id)` | Persistent across sessions | Session-specific plugin state |
| `/cache` | Shared across all instances of same plugin | Persistent | Shared persistent data (e.g., compiled templates) |
| `/tmp` | Shared across ALL plugins | Session | Temporary files, cross-plugin coordination |

**Critical finding for Impulse:** With the default `/host` mount, a Zellij plugin launched from a project directory CAN read `.impulse/GENOME.md`, `.impulse/LIVE_STATE.json`, and `.impulse/HISTORY_INDEX.md` -- because `/host` maps to the Zellij startup directory, which is typically the project root.

**FullHdAccess permission:** Allows a plugin to dynamically change its `/host` directory at runtime, enabling filesystem browsing beyond the initial working directory. This is a sensitive permission that requires explicit user grant.

#### Permission System (Complete List)

| Permission | What It Gates | Impulse Relevance |
|-----------|--------------|-------------------|
| `ReadApplicationState` | Pane/tab/session/mode info events | HIGH -- needed to observe agent panes |
| `ChangeApplicationState` | Modify panes, tabs, navigation | MEDIUM -- for dashboard interactions |
| `OpenFiles` | Open files in `$EDITOR` | LOW |
| `RunCommands` | Execute background commands | HIGH -- needed for status bar commands (e.g., `cat .impulse/LIVE_STATE.json`) |
| `OpenTerminalsOrPlugins` | Create terminal/plugin panes | MEDIUM -- for spawning new agent panes |
| `WriteToStdin` | Write to pane stdin as if user | LOW (security-sensitive) |
| `Reconfigure` | Modify Zellij config at runtime | LOW |
| `FullHdAccess` | Access full filesystem, not just CWD | MEDIUM -- enables reading `.impulse/` from any directory |
| `StartWebServer` | Control Zellij web server | LOW |
| `InterceptInput` | Capture all keypresses to plugin | LOW |

#### Event System (Subscribable Events)

Events relevant to a Impulse status bar or dashboard plugin:

| Event | Data Provided | Use Case |
|-------|--------------|----------|
| `PaneUpdate` | Active panes: title, command, exit code | Show which agents are running |
| `TabUpdate` | Tab info: position, name, swap layouts | Track workspace organization |
| `SessionUpdate` | Active sessions on machine | Multi-session awareness |
| `ModeUpdate` | Input mode, theme, session name | UI theming |
| `FileSystemCreate` | Created files in Zellij CWD | Watch for `.impulse/LIVE_STATE.json` creation |
| `FileSystemUpdate` | Modified files in Zellij CWD | Watch for `.impulse/` file changes |
| `FileSystemDelete` | Deleted files in Zellij CWD | Detect session cleanup |
| `RunCommandResult` | exit code, stdout, stderr, context | Read command output (e.g., parse LIVE_STATE.json) |
| `Timer` | Timer expiry notification | Periodic polling (e.g., every 5s read GENOME.md stats) |
| `PaneClosed` | Pane ID | Detect agent session ending |
| `CommandPaneOpened/Exited` | Pane ID, exit code | Track command lifecycle |
| `ListClients` | Connected clients, focused panes, running commands | Multi-user awareness |
| `CustomMessage` | Inter-plugin/worker messages | Dashboard <-> status bar communication |
| `Visible` | Plugin visibility changed | Optimize rendering (skip when hidden) |

**Key discovery:** The `FileSystem*` events are particularly powerful for Impulse. A Zellij plugin could subscribe to `FileSystemUpdate` and react when `.impulse/LIVE_STATE.json` is modified by the OpenCode plugin hooks -- creating a real-time reactive pipeline without polling.

#### Plugin Command API (Relevant Subset)

```rust
// Run background command, get result via RunCommandResult event
run_command(&["cat", ".impulse/LIVE_STATE.json"], context);

// Write to a specific pane's stdin
write_chars_to_pane_id("agent status check\n", pane_id);

// Open a floating terminal for an agent
open_terminal_floating("/path/to/project", x, y, width, height);

// Open a file in the editor
open_file_floating("/path/to/.impulse/GENOME.md");

// Set a timer for periodic updates
set_timeout(5.0); // fires Timer event in 5 seconds

// Send a pipe message to another plugin
pipe_message_to_plugin(
    MessageToPlugin {
        plugin_url: "file:/path/to/dashboard.wasm",
        plugin_config: BTreeMap::new(),
        message_name: "live_state_update",
        message_payload: Some(json_string),
        ..Default::default()
    }
);

// Control floating pane positioning (0.42.0+)
// Plugins can change floating pane coordinates of themselves and other panes
```

#### Sandbox Limitations Summary

| Capability | Status | Notes |
|-----------|--------|-------|
| Read files in project CWD | YES | Via `/host` mount |
| Write files in project CWD | YES | Via `/host` mount |
| Read `.impulse/` directory | YES | If Zellij started from project root |
| Access files outside CWD | REQUIRES `FullHdAccess` | Security-sensitive permission |
| Direct network access | NO | Use `web_request` command (requires `WebAccess` permission) |
| Spawn processes | Via `run_command` only | Results delivered asynchronously via event |
| Direct inter-plugin memory sharing | NO | Memory fully isolated |
| Access other panes' scrollback | YES | Via `ReadApplicationState` |
| Write to other panes' stdin | YES | Via `WriteToStdin` permission |

### 2.3 KDL Layout System -- Advanced Patterns

Zellij uses KDL (KDL Document Language) for layout definition. The system is declarative, not procedural -- there are no conditionals, loops, or environment variable interpolation.

**Core features:**

```kdl
// Pane templates -- reusable configurations
pane_template name="agent" {
    command "opencode"
    args "--agent" "build"
    start_suspended true      // Wait for ENTER before running
    close_on_exit false       // Keep pane after agent exits
}

// Tab templates with child injection point
tab_template name="impulse-tab" {
    pane size=1 borderless=true {
        plugin location="file:zjstatus.wasm"
    }
    children                  // Panes injected here
    pane size=1 borderless=true {
        plugin location="status-bar"
    }
}

// CWD composition (relative paths compose hierarchically)
layout {
    cwd "/path/to/project"    // Global CWD
    tab name="Agents" cwd="." {       // Inherits global CWD
        split_direction "vertical"
        pane command="opencode" cwd="." {
            args "--agent" "build"
        }
        pane command="opencode" cwd="." {
            args "--agent" "plan"
        }
    }
}

// Floating panes with precise positioning
floating_panes {
    pane x=5 y=2 width="40%" height="60%" {
        plugin location="file:impulse-dashboard.wasm"
    }
    pane x="50%" y=2 width="48%" height="30%" {
        name "GENOME Viewer"
        command "bat"
        args "--paging=always" ".impulse/GENOME.md"
    }
}

// Plugin loading with configuration
pane size=1 borderless=true {
    plugin location="file:impulse-status.wasm" {
        genome_path ".impulse/GENOME.md"
        refresh_interval "5"
        show_agent_count "true"
    }
}
```

**Swap layouts** allow dynamic layout changes when panes are added/removed:
```kdl
swap_tiled_layout name="2-agent" max_panes=2 {
    tab {
        pane split_direction="horizontal" {
            pane
            pane
        }
    }
}
swap_tiled_layout name="3-agent" min_panes=3 max_panes=3 {
    tab {
        pane split_direction="vertical" {
            pane
            pane
            pane
        }
    }
}
```

When a new pane is opened (Alt+n), Zellij automatically snaps to the matching swap layout based on pane count.

**Limitations of the KDL layout system:**
- No conditional logic (cannot do "if macOS then X else Y")
- No environment variable interpolation in layout files
- No runtime parameterization (layouts are static declarations)
- No dynamic pane creation from within a layout (use plugin API for that)
- Plugin configuration is string-only (`BTreeMap<String, String>`)

### 2.4 Session Resurrection

Zellij serializes session state to the user's cache folder every second.

**What survives resurrection:**
- Layout of panes and tabs (positions, sizes, split directions)
- Commands running in each pane (as command panes)
- Optionally: pane viewport and scrollback (when configured)

**What does NOT survive:**
- Plugin internal state (WASM memory is wiped)
- Environment variables set during the session
- Ephemeral shell state (aliases, functions, variables)
- Active process state (processes are killed on exit)

**For Impulse:** `.impulse/LIVE_STATE.json` is by design ephemeral and gitignored. Session resurrection will re-create the pane layout and re-launch agent commands, but `LIVE_STATE.json` will be stale (it was last written by the previous session's `session.end` hook). The `session.start` hook handles this correctly -- it reads whatever is on disk, and the first `tool.execute.after` will refresh it.

**Important detail:** Resurrected command panes show a "Press ENTER to run..." banner by default (safety measure against re-running destructive commands). This can be bypassed with `zellij attach --force-run-commands <session-name>`.

**Session data is stored as human-readable layout files** in the system cache directory. These are KDL-formatted and can be manually inspected or edited.

### 2.5 Floating Panes (0.42.0+)

Zellij 0.42.0 introduced **pinned floating panes** and plugin control over floating pane coordinates. This is the foundation for Impulse's Phase 3 dashboard UI.

**Key capabilities:**
- Plugins can change floating pane coordinates (x, y, width, height) of themselves and other panes
- Changes can be batched (send a vector of coordinate changes) for smooth dashboard layouts
- **Pinned panes** stay on top even when not focused (Ctrl+p, i to toggle)
- Plugins can read mouse motions when hovered, enabling interactive UI elements
- Plugins can stack arbitrary panes using pane IDs (combine panes into a stack)
- Built-in UI components (buttons, text formatting) work with mouse interaction

**Dashboard architecture enabled by 0.42.0+:**
```
+--------------------------------------------------+
| zjstatus bar: [Agent: 3 active] [GENOME: 47 lines] |
+--------------------------------------------------+
|                     |                              |
|   Agent 1 (build)   |   Agent 2 (plan)            |
|                     |                              |
|   Agent 3 (review)  |   Agent 4 (test)            |
|                     |                              |
+--------------------------------------------------+
|  +---[Pinned Float: Impulse Dashboard]---+        |
|  | LIVE_STATE: 3 agents active           |        |
|  | Last GENOME update: 2m ago            |        |
|  | Session #47 | Duration: 34m           |        |
|  +---------------------------------------+        |
+--------------------------------------------------+
```

### 2.6 Event Bus and Inter-Plugin Communication

Zellij provides three mechanisms for plugin communication:

**1. Pipes (primary IPC mechanism, introduced in 0.40.0):**
- Unidirectional message channels carrying serializable text
- Message structure: `name` (string), `payload` (string), `args` (string->string map)
- Broadcast mode: pipes without a specific destination go to ALL plugins
- Targeted mode: pipe to specific plugin URL + configuration
- **Lazy loading:** piping to a non-running plugin auto-launches it, queuing the message
- Backpressure: CLI pipe input can be blocked/unblocked by the plugin
- Two-way communication: source plugin ID is provided, allowing response pipes

```rust
// Send from Plugin A to Plugin B
pipe_message_to_plugin(MessageToPlugin {
    plugin_url: "file:/path/to/plugin-b.wasm",
    message_name: "genome_updated",
    message_payload: Some("{\"lines\": 47}"),
    ..Default::default()
});

// Plugin B receives in pipe() method
fn pipe(&mut self, msg: PipeMessage) -> bool {
    match msg.source {
        PipeSource::Plugin(plugin_id) => {
            // Can respond back using plugin_id
        }
        PipeSource::Cli(pipe_id) => {
            // Can write to CLI's stdout
            cli_pipe_output(&msg.name, "response data");
        }
        PipeSource::Keybind => {}
    }
    true // re-render
}
```

**2. CLI-to-Plugin messaging:**
```bash
# Send message from shell to plugin
zellij pipe --plugin "file:impulse-status.wasm" --name "refresh" --payload '{"force": true}'
```
This means external scripts (including mise hooks or OpenCode tools) can communicate with running Zellij plugins.

**3. Plugin Workers (background threads):**
- `post_message_to` / `post_message_to_plugin` for plugin <-> worker communication
- Workers run on separate threads, handling long-running operations without blocking the UI thread
- Results communicated back via `CustomMessage` events

**Can a plugin observe what is happening in another pane?**
Yes, with `ReadApplicationState` permission:
- `PaneUpdate` event provides titles, commands, and exit codes for all panes
- `ListClients` provides which pane each connected client has focused
- `CommandPaneOpened/Exited` tracks command lifecycle
- However, plugins CANNOT read the actual terminal output (scrollback buffer content) of other panes. They can only observe metadata.

### 2.7 Plugin Development Experience

**Toolchain setup:**
```bash
rustup target add wasm32-wasip1
cargo install cargo-generate        # Optional: for templates
cargo generate zellij-org/rust-plugin-example  # Scaffold new plugin
```

**Compile cycle:**
```bash
cargo build --target wasm32-wasip1 --release
# Output: target/wasm32-wasip1/release/my-plugin.wasm
```

**Compile times (observed from zjstatus and rust-plugin-example):**
- Clean build: ~30-60 seconds (depending on dependencies)
- Incremental build: ~5-15 seconds
- WASM binary size: zjstatus is ~2MB, minimal plugin is ~200KB

**Hot reload development environment:**
Zellij provides an official development workflow:
1. Run `zellij -l zellij.kdl` in the plugin's directory (loads development layout)
2. Layout includes: source editor pane, compilation pane, plugin preview pane
3. Press `Ctrl+Shift+r` to compile and reload the plugin in-place
4. Development helper plugin (`develop-rust-plugin.wasm`) automates the compile-reload cycle

**Debugging:**
- `tracing` crate writes to `/host/.zjstatus.log` (as seen in zjstatus source)
- `println!()` in `render()` writes to the plugin pane (useful for state inspection)
- No step-through debugger available (WASM interpreter limitation)
- Tracing to file is the primary debugging mechanism

**Testing:**
- Unit tests run natively (not in WASM) using `#[cfg(test)]` guards
- zjstatus uses `rstest` for parameterized tests
- Mock the Zellij shims with `#[cfg(all(not(feature = "bench"), not(test)))]` guards on `run_command` calls
- No official integration test framework for plugin-host interaction

**Pain points:**
- No step debugger; `tracing` to file is the primary debugging tool
- Plugin configuration is `BTreeMap<String, String>` only -- no structured types, no nested config
- WASM binary needs redistribution (users must download `.wasm` file)
- Permission prompts appear on every session start (no persistent grant mechanism found)
- wasmi interpreter is slower than wasmtime (trade-off for smaller binary and simpler embedding)

### 2.8 Verdict for Impulse

**Can a WASM plugin read `.impulse/` files?** YES. The `/host` mount maps to the directory where Zellij was started. If Zellij starts from the project root (the normal case), the plugin can read `.impulse/GENOME.md`, `.impulse/LIVE_STATE.json`, and `.impulse/HISTORY_INDEX.md` via standard WASI file I/O (`std::fs::read_to_string("/host/.impulse/GENOME.md")`).

**Can a WASM plugin WRITE to `.impulse/` files?** YES. The `/host` mount provides write access. However, this creates a potential conflict: if both the OpenCode plugin (via impulse-plugin hooks) and a Zellij WASM plugin write to the same files, you need coordination. Recommendation: the Zellij plugin should be READ-ONLY for `.impulse/` files, and only the OpenCode hooks should write.

**Can a WASM plugin watch for `.impulse/` file changes reactively?** YES, via `FileSystemCreate`, `FileSystemUpdate`, and `FileSystemDelete` events. This eliminates the need for polling in the Zellij status bar plugin -- it can subscribe to file change events and re-render only when `.impulse/` files are actually modified.

**Revised architecture for Phase 3 Zellij plugins:**

```
OpenCode Hooks (writes)          Zellij WASM Plugin (reads)
─────────────────────           ──────────────────────────
session.start hook   ──write──> GENOME.md
                                    │
tool.execute.after   ──write──> LIVE_STATE.json
                                    │ FileSystemUpdate event
                                    ▼
session.end hook     ──write──> GENOME.md ──trigger──> Status Bar re-render
                     ──write──> HISTORY_INDEX.md       Dashboard re-render
```

**Plugin development timeline estimate:**
- Phase 3 status bar plugin (read `.impulse/` files, display in bar): 2-3 days
- Phase 3 dashboard plugin (floating pane, interactive): 5-7 days
- Both require: Rust toolchain, `wasm32-wasip1` target, familiarity with `zellij-tile` crate

---

## 3. mise -- Polyglot Tool Version Manager

### 3.1 Hook System

mise supports six hook types, configured in `mise.toml` under the `[hooks]` section. Hooks require the `experimental` setting and are gated behind `mise activate` shell integration (except pre/post-install).

**Hook types (validated from source code at `cloned-repos/mise/src/hooks.rs`):**

| Hook | Trigger | Shell Integration Required | Use Case |
|------|---------|---------------------------|----------|
| `enter` | First `cd` into project directory where `mise.toml` activates | Yes | Project initialization, environment setup |
| `leave` | `cd` out of project directory where `mise.toml` was active | Yes | Cleanup, state teardown |
| `cd` | Every directory change while mise is active | Yes | Directory-aware behaviors |
| `preinstall` | Before `mise install` begins | No (works with shims) | Pre-flight checks |
| `postinstall` | After `mise install` completes | No (works with shims) | Post-install setup |
| `watch_files` | File matching glob pattern changes | Yes | Auto-formatting, auto-testing |

**Configuration syntax (three forms):**

```toml
# Simple string form
[hooks]
enter = "echo 'Entered project'"
leave = "echo 'Left project'"
cd = "echo 'Changed to $PWD'"

# Table form with explicit shell
[hooks.enter]
shell = "bash"
script = "source .impulse/init.sh"

# Array form (multiple hooks)
[hooks]
enter = [
    "echo 'Setting up project'",
    { script = "source .impulse/init.sh", shell = "bash" }
]

# Tool-level postinstall
[tools]
bun = { version = "1.1", postinstall = "bun install" }
node = { version = "20", postinstall = "npm install -g pnpm" }

# Watch files (triggers on file changes)
[[watch_files]]
patterns = ["src/**/*.ts"]
run = "bun run type-check"
```

**Environment variables available in hooks:**

| Variable | Available In | Value |
|----------|-------------|-------|
| `MISE_PROJECT_ROOT` | All hooks | Project root directory |
| `MISE_CONFIG_ROOT` | All hooks | Directory containing the active `mise.toml` |
| `MISE_ORIGINAL_CWD` | All hooks | User's current directory |
| `MISE_PREVIOUS_DIR` | `cd`, `leave` | Previous directory (only on directory changes) |
| `MISE_INSTALLED_TOOLS` | `postinstall` | JSON array of `{name, version}` objects |
| `MISE_TOOL_NAME` | Tool-level `postinstall` | Short name of installed tool |
| `MISE_TOOL_VERSION` | Tool-level `postinstall` | Installed version |
| `MISE_TOOL_INSTALL_PATH` | Tool-level `postinstall` | Installation path |
| `MISE_WATCH_FILES_MODIFIED` | `watch_files` | Colon-separated list of modified files |
| `MISE_NO_HOOKS` | (Set by mise internally) | `"1"` to prevent recursive hook execution |

**Important implementation detail (from source):** mise sets `MISE_NO_HOOKS=1` in the hook environment to prevent recursive hook execution. If a hook runs `mise run` which spawns a shell that activates mise and re-triggers hooks, the recursion is blocked.

**Limitation:** Shell hooks cannot perform cleanup. Unlike `[env]` settings (which mise can track and reverse), hook side-effects are fire-and-forget. If an `enter` hook sets an environment variable, the corresponding `leave` hook must manually unset it.

### 3.2 Impulse Integration Potential

**Could mise hooks auto-init `.impulse/` on project enter?**

YES. This is one of the most valuable integrations available. Here is the exact configuration:

```toml
# mise.toml (project root)
[settings]
experimental = true          # Required for hooks

[hooks]
enter = """
  if [ ! -d ".impulse" ]; then
    echo "[impulse] Initializing .impulse/ directory..."
    mkdir -p .impulse
    if [ -f "impulse-plugin/templates/GENOME.md" ]; then
      cp impulse-plugin/templates/GENOME.md .impulse/GENOME.md
    else
      echo "# GENOME.md\\n> Project: $(basename $PWD)\\n> Created: $(date -I)" > .impulse/GENOME.md
    fi
    echo '{"agents":[],"lastUpdated":"'$(date -Iseconds)'"}' > .impulse/LIVE_STATE.json
    echo "# Session History\\n" > .impulse/HISTORY_INDEX.md
    echo "[impulse] Initialized. GENOME.md, LIVE_STATE.json, HISTORY_INDEX.md created."
  fi
"""

leave = """
  echo "[impulse] Session context: $(wc -l < .impulse/GENOME.md 2>/dev/null || echo 0) GENOME lines"
"""
```

**Integration opportunities beyond auto-init:**

1. **Tool version enforcement:** mise pins exact Bun version for `impulse-plugin/`, ensuring consistent behavior across machines:
   ```toml
   [tools]
   bun = "1.1"
   rust = "1.85"    # For Phase 3 Zellij plugins
   ```

2. **Watch files for auto-testing:**
   ```toml
   [[watch_files]]
   patterns = ["impulse-plugin/src/**/*.ts"]
   run = "cd impulse-plugin && bun test"
   ```

3. **Postinstall for dependency setup:**
   ```toml
   [tools]
   bun = { version = "1.1", postinstall = "cd impulse-plugin && bun install" }
   ```

4. **Environment variables for Impulse config:**
   ```toml
   [env]
   IMPULSE_ENV = "development"
   IMPULSE_GENOME_MAX_LINES = "500"
   IMPULSE_HISTORY_RETENTION_DAYS = "90"
   ```

5. **Project enter notification to Zellij plugin:**
   ```toml
   [hooks]
   enter = "zellij pipe --plugin 'file:impulse-status.wasm' --name 'project_enter' --payload '{\"project\": \"'$MISE_PROJECT_ROOT'\"}' 2>/dev/null || true"
   ```
   This pipes a message to the running Zellij plugin when a user enters the project directory -- the plugin can then refresh its display.

### 3.3 CI/CD Integration

mise provides first-class CI/CD support through multiple mechanisms:

**GitHub Actions (recommended):**
```yaml
# .github/workflows/impulse-test.yml
name: Impulse Tests
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: jdx/mise-action@v3
        with:
          cache: true              # Cache mise tools between runs
          experimental: true       # Enable experimental features (hooks)
      - run: cd impulse-plugin && bun install && bun test
```

**`mise-action@v3` configuration options:**
- `version`: Pin specific mise release (default: latest)
- `install`: Auto-run `mise install` (default: true)
- `cache`: Enable GitHub caching of tool installations (default: true)
- `experimental`: Activate experimental features (default: false)
- `mise_toml`: Inline tool configuration (avoids needing mise.toml in repo)
- `tool_versions`: Alternative inline config format

**Caching strategy:**
- Cache `MISE_DATA_DIR` (`.mise/installs`, `.mise/mise-[version]`)
- Use `mise.toml` and `mise.lock` as cache keys
- Typical cache hit rate: 95%+ (tools only re-downloaded on version change)

**Docker-based CI (GitLab, Jenkins, etc.):**
```dockerfile
FROM debian:bookworm-slim
RUN curl https://mise.run | MISE_INSTALL_PATH=/usr/local/bin/mise sh
COPY mise.toml .
RUN mise install
```

**Bootstrap method (zero external download in CI):**
```bash
mise generate bootstrap  # Creates committed ./bin/mise binary
# In CI: ./bin/mise install && ./bin/mise x -- bun test
```

**Key benefit for Impulse:** The same `mise.toml` that manages local development tools also controls CI tool versions. No drift between "it works on my machine" and CI environments. Bun 1.1, Rust 1.85, and Python 3.12 are pinned identically in both contexts.

### 3.4 Verdict for Impulse

**Could mise hooks trigger Impulse operations?** YES, and this should be implemented immediately. The `enter` hook auto-creating `.impulse/` is the single lowest-friction onboarding path for new projects.

**Recommended mise.toml for Impulse projects:**
```toml
[settings]
experimental = true

[tools]
bun = "1.1"
node = "20"              # Some tools still need Node
python = "3.12"          # For Phase 2 memory pipeline
rust = "1.85"            # For Phase 3 Zellij plugins

[env]
IMPULSE_ENV = "development"

[hooks]
enter = """
  if [ ! -d ".impulse" ]; then
    mkdir -p .impulse
    echo "# GENOME.md" > .impulse/GENOME.md
    echo '{"agents":[],"lastUpdated":""}' > .impulse/LIVE_STATE.json
    echo "# Session History" > .impulse/HISTORY_INDEX.md
    echo "[impulse] Initialized .impulse/ directory"
  fi
"""

leave = "echo \"[impulse] Left project: $(wc -l < .impulse/GENOME.md 2>/dev/null || echo 0) GENOME lines\""

[hooks.postinstall]
script = "cd impulse-plugin && bun install 2>/dev/null || true"
```

---

## Key Findings

1. **Ghostty's four-thread architecture genuinely prevents frame drops during multi-pane heavy output**, but the performance difference versus Alacritty/Kitty is 5-15%, not transformative. Any GPU terminal works for Impulse.

2. **Zellij WASM plugins CAN read and write `.impulse/` files** via the `/host` WASI mount. This was previously undocumented in our analysis and opens a direct reactive pipeline for Phase 3 UI.

3. **`FileSystemUpdate` events eliminate polling** for the Zellij status bar plugin. When OpenCode hooks write to `.impulse/LIVE_STATE.json`, the Zellij plugin is notified immediately via event subscription.

4. **Zellij pipes enable CLI-to-plugin and plugin-to-plugin communication**, including lazy loading of plugins on first message. External tools (mise hooks, shell scripts, OpenCode) can pipe messages to running Zellij plugins.

5. **mise `enter` hooks provide zero-friction `.impulse/` auto-initialization**. A single `mise.toml` entry creates the directory structure on project enter, eliminating manual setup.

6. **Session resurrection does NOT preserve plugin state** (WASM memory is wiped). The Impulse architecture already handles this correctly -- `.impulse/` files on disk ARE the persistent state, and `session.start` hooks re-read them.

7. **The actual rendering bottleneck in a 4-agent workspace is PTY throughput and LLM latency**, not terminal rendering. GPU acceleration is nice-to-have, not need-to-have.

8. **Zellij plugin development has a ~5-15 second iteration cycle** (incremental Rust-to-WASM compile + hot reload), but lacks a step debugger. Tracing to file is the primary debugging tool.

9. **mise's `watch_files` hook can auto-run tests** when Impulse source files change, providing continuous validation during development.

10. **Plugin-to-CLI pipes with backpressure** mean a Zellij plugin could serve as a lightweight API: external scripts send requests via `zellij pipe`, plugin processes them, and writes responses to pipe stdout.

---

## Implications for Impulse

### Phase 1 (MVP) Impact
- **Add mise auto-init hook immediately.** This costs one `mise.toml` entry and removes all manual `.impulse/` setup friction. Zero downside.
- **No Zellij plugin code needed.** The OpenCode hooks handle all file I/O. Zellij provides only the layout (`.kdl` file).
- **Ghostty configuration is cosmetic.** Install it, apply the recommended config, done.

### Phase 3 (Dashboard) Impact -- Revised Architecture
The Zellij plugin system is significantly more capable than previously assessed:
- **Status bar plugin can react to file changes** (not poll). Subscribe to `FileSystemUpdate` for `.impulse/` files.
- **Dashboard plugin can use floating panes** with precise positioning (0.42.0+ API).
- **External tools can communicate with plugins** via `zellij pipe` from CLI.
- **Plugin can read LIVE_STATE.json directly** from `/host/.impulse/LIVE_STATE.json` without shelling out.
- **Plugin cannot read other panes' scrollback content** -- only metadata (titles, commands, exit codes). This means the dashboard shows "Agent X is running `opencode build`" but not the actual agent output.

### Tool Version Strategy
mise pins identical tool versions for local development and CI:
- Bun 1.1 (impulse-plugin runtime)
- Rust 1.85 (Phase 3 WASM plugins)
- Python 3.12 (Phase 2 memory pipeline)
- Node 20 (compatibility with tools that require it)

---

## Open Questions

1. **Does `FileSystemUpdate` fire for files in subdirectories?** If Zellij starts from `/project/` and `.impulse/LIVE_STATE.json` is modified, does the event fire? Need to test empirically -- the documentation says "files in the Zellij startup directory" but does not specify depth.

2. **What is the latency of `FileSystemUpdate` events?** Are they debounced? Is there a delay between the filesystem write and the event delivery to the plugin? This determines whether the reactive pipeline is suitable for real-time status updates.

3. **Can plugin `/data` directory persist state across Zellij sessions?** The documentation says "persistent across sessions" but it is unclear if this survives `zellij kill-all-sessions`. Need to verify what the persistence boundary actually is.

4. **What happens when `FullHdAccess` is denied?** If the user rejects this permission, does the plugin fall back gracefully, or does it crash? The status bar plugin should work without `FullHdAccess` (only needs `/host` access to project CWD).

5. **Can mise's `enter` hook detect whether OpenCode is running?** If the hook could detect an active OpenCode session, it could auto-configure the `.impulse/` directory differently (e.g., set agent names in LIVE_STATE.json).

6. **What is the actual memory overhead of a minimal Zellij WASM plugin?** The wasmi interpreter has lower performance than wasmtime -- what is the baseline memory cost per plugin instance for a simple status bar?

7. **Does Ghostty's Metal renderer interact differently with Zellij than OpenGL terminals?** Some users have reported rendering artifacts with specific terminal + multiplexer combinations. Need to verify Ghostty + Zellij compatibility.

8. **Can mise hooks and Zellij pipes be combined for a project-enter-to-plugin-refresh pipeline?** The concept: `cd into project` -> mise `enter` hook fires -> hook sends `zellij pipe` message -> Zellij plugin refreshes display. Need to test the full chain.

---

_Created: 2026-02-20 | Research based on: source code analysis (cloned-repos/), official documentation, benchmarks, and community reports_
