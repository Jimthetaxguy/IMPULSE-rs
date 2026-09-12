# Adding CLI Commands to Impulse

Step-by-step guide for adding a new CLI subcommand.

---

## Steps

### 1. Add the enum variant in `src/cli.rs`

`src/main.rs` is a thin entrypoint (it only calls `cli::Cli::parse()` and dispatches) — the
`Commands` enum (clap derive) itself lives in `src/cli.rs`. Find it and add a new variant:

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing commands ...

    /// Description of what this command does
    MyNewCommand {
        /// Required argument
        #[arg(long)]
        name: String,

        /// Optional argument with default
        #[arg(long, default_value = "default")]
        format: String,

        /// Flag
        #[arg(long)]
        verbose: bool,
    },
}
```

For feature-gated commands:
```rust
#[cfg(feature = "my-feature")]
MyFeatureCommand { /* ... */ },
```

### 2. Add the match arm in the dispatch handlers

The handler is shared, not duplicated per execution mode: `direct_dispatch.rs` and
`daemon_dispatch.rs` both match on the same `cli::Commands` variant. Add the new arm to
whichever dispatcher(s) the command applies to:

```rust
Commands::MyNewCommand { name, format, verbose } => {
    handle_my_new_command(&name, &format, verbose, &impulse_dir)?;
}
```

Keep the match arm thin — just extract args and call a handler function. If the command is also
exposed from the `ion` binary (`src/bin/ion.rs`), add a matching variant on `ion`'s own minimal
`IonCommand` enum whose handler still calls the same shared function in `handlers::` — never a
second implementation of the command's actual logic.

### 3. Create the handler function

Place handler in the appropriate module (or create a new one in `src/`):

```rust
pub fn handle_my_new_command(
    name: &str,
    format: &str,
    verbose: bool,
    impulse_dir: &Path,
) -> Result<()> {
    // Load state if needed
    let state = crate::state::State::load(impulse_dir)?;

    // Do the work
    let result = process(name, &state)?;

    // Output (respect --json flag if applicable)
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("{result}");
    }

    Ok(())
}
```

### 4. Add tests

In the handler module's `mod tests`:

```rust
#[test]
fn test_my_new_command_basic() {
    let dir = tempfile::TempDir::new().unwrap();
    let impulse_dir = dir.path().join(".impulse");
    std::fs::create_dir_all(&impulse_dir).unwrap();

    // Initialize minimal state
    let state = State::default();
    state.save(&impulse_dir).unwrap();

    let result = handle_my_new_command("test", "text", false, &impulse_dir);
    assert!(result.is_ok());
}
```

### 5. If the command needs daemon access

Add a corresponding `DaemonRequest` variant — see `references/adding-daemon-ipc.md`.

---

## Checklist

- [ ] Enum variant added with doc comment and clap attributes
- [ ] Match arm in main() calls handler function
- [ ] Handler function returns `Result<()>`
- [ ] Handler uses `State::load()` if it needs state
- [ ] Handler uses `atomic_write()` if it writes files
- [ ] Tests with `tempfile::TempDir` for isolation
- [ ] `cargo test` passes
- [ ] `cargo clippy -- -D warnings` clean
