# Platform Setup Guide

Impulse runs on macOS, Linux, and (partially) Windows.

## macOS

### Supported Versions

- macOS 12 (Monterey) and later
- Apple Silicon (M1/M2/M3) and Intel

### Installation

```bash
# Option 1: Build from source (recommended)
git clone https://github.com/jamespustorino/impulse-rs.git
cd impulse-rs
cargo install --path .

# Option 2: Homebrew (coming soon)
# brew install impulse-rs
```

### Prerequisites

```bash
# Install Rust if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install at least one AI agent CLI
npm install -g @anthropic-ai/claude-code    # Claude Code
pip install opencode                         # OpenCode
npm install -g @openai/codex                 # Codex (optional)
```

### API Keys

```bash
# For Claude Code chat in daemon
export ANTHROPIC_API_KEY=sk-ant-your-key-here

# Add to shell profile for persistence
echo 'export ANTHROPIC_API_KEY=sk-ant-...' >> ~/.zshrc
```

### Running

```bash
# Launch TUI
impulse-rs run

# Or use CLI commands
impulse-rs status
impulse-rs session-start -n my-project -p claude-code
```

### Known Issues

- **Keychain** — Credential storage uses macOS Keychain (enabled by default)
- **Terminal** — Works best with iTerm2 or Terminal.app

---

## Linux

### Supported Distributions

- Ubuntu 20.04+
- Fedora 36+
- Arch Linux
- Debian 11+
- Most modern Linux distros

### Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone and build
git clone https://github.com/jamespustorino/impulse-rs.git
cd impulse-rs
cargo install --path .
```

### Prerequisites

```bash
# System dependencies (Ubuntu/Debian)
sudo apt-get install build-essential pkg-config libssl-dev

# AI agent CLIs
npm install -g @anthropic-ai/claude-code    # Claude Code
pip install opencode                         # OpenCode
npm install -g @openai/codex                 # Codex (optional)
```

### API Keys

```bash
# Add to ~/.bashrc or ~/.zshrc
export ANTHROPIC_API_KEY=sk-ant-your-key-here
export OPENAI_API_KEY=sk-your-key-here
```

### Running

```bash
# Launch TUI
impulse-rs run

# Check status
impulse-rs status
```

### Terminal Compatibility

Impulse works with:
- GNOME Terminal
- Konsole
- Alacritty (recommended for performance)
- kitty
- tmux (run inside tmux or vice versa)

### Known Issues

- **Keychain** — Falls back to file-based credential storage on Linux
- **pty** — Requires kernel pty support (standard on all modern Linux)

---

## Windows

### Status: Partial Support

Native Windows support is **not fully tested**. We recommend:

### Option 1: WSL2 (Recommended)

```bash
# Install WSL2
wsl --install -d Ubuntu

# Inside Ubuntu VM, follow Linux instructions above
wsl -e bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
wsl -e bash -c "git clone https://github.com/jamespustorino/impulse-rs.git"
```

### Option 2: Native (Experimental)

```bash
# Install Rust for Windows
rustup default stable-x86_64-pc-windows-msvc

# Build
git clone https://github.com/jamespustorino/impulse-rs.git
cd impulse-rs
cargo build --release
```

### Known Windows Issues

- **portable-pty** — May have issues on native Windows
- **Credentials** — Keychain not supported, use env vars
- **Paths** — Use forward slashes or convert paths

### Troubleshooting WSL

```bash
# Run Impulse from WSL, display on Windows
export DISPLAY=:0

# Or use VcXsrv / X410
```

---

## Platform Comparison

| Feature | macOS | Linux | Windows |
|---------|-------|-------|---------|
| TUI | ✅ | ✅ | ⚠️ WSL |
| PTY | ✅ | ✅ | ⚠️ WSL |
| Keychain | ✅ | ❌ (file) | ❌ (file) |
| Daemon | ✅ | ✅ | ⚠️ WSL |
| Search | ✅ | ✅ | ⚠️ WSL |

---

## Docker (Alternative)

Run Impulse in Docker on any platform:

```dockerfile
FROM rust:1.75-bookworm

RUN apt-get update && apt-get install -y \
    build-essential pkg-config libssl-dev \
    && cargo install impulse-rs

WORKDIR /app
CMD ["impulse-rs", "run"]
```

```bash
docker build -t impulse .
docker run -it -v $(pwd):/app impulse
```

---

## Verifying Your Setup

```bash
# Check version
impulse-rs --version

# Run tests
cargo test

# Check status
impulse-rs status

# List tools
impulse-rs tooling-list
```

If all commands work, you're ready to go!
