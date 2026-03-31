# Adding Daemon IPC Messages to Impulse

Step-by-step guide for adding a new daemon request/response pair.

---

## Steps

### 1. Add DaemonRequest variant in `src/daemon/mod.rs`

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonRequest {
    // ... existing variants ...

    /// Description of what this request does
    MyNewRequest {
        param_one: String,
        #[serde(default)]
        optional_param: Option<i32>,
    },
}
```

### 2. Add DaemonResponse variant

```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum DaemonResponse {
    // ... existing variants ...

    MyNewResult {
        data: Vec<String>,
        count: usize,
    },
}
```

### 3. Add handler match arm

Find the main request dispatch match in the daemon handler function:

```rust
DaemonRequest::MyNewRequest { param_one, optional_param } => {
    match handle_my_new_request(&state, &param_one, optional_param).await {
        Ok(result) => DaemonResponse::MyNewResult {
            data: result.items,
            count: result.items.len(),
        },
        Err(e) => DaemonResponse::Error {
            message: format!("{e:#}"),
        },
    }
}
```

### 4. Implement the handler

```rust
async fn handle_my_new_request(
    state: &SharedState,
    param_one: &str,
    optional_param: Option<i32>,
) -> Result<MyNewResultData> {
    // Read state (acquire read lock, release quickly)
    let data = {
        let guard = state.read().await;
        guard.get_relevant_data(param_one)
    };

    // Process (no lock held during computation)
    let processed = process_data(data, optional_param)?;

    Ok(processed)
}
```

**Key rules:**
- Never hold the state lock across I/O or computation
- Clone/snapshot data out of the lock guard
- Return `DaemonResponse::Error` on failure, never panic

### 5. Add serialization test

```rust
#[test]
fn test_my_new_request_serialization() {
    let req = DaemonRequest::MyNewRequest {
        param_one: "test".to_string(),
        optional_param: Some(42),
    };
    let json = serde_json::to_string(&req).unwrap();
    let deserialized: DaemonRequest = serde_json::from_str(&json).unwrap();

    match deserialized {
        DaemonRequest::MyNewRequest { param_one, optional_param } => {
            assert_eq!(param_one, "test");
            assert_eq!(optional_param, Some(42));
        }
        _ => panic!("Wrong variant"),
    }
}
```

### 6. Add integration test (if complex)

```rust
#[tokio::test]
async fn test_my_new_request_via_ipc() {
    let guard = DaemonGuard::spawn(&tempdir).await.unwrap();

    let response = guard.send(DaemonRequest::MyNewRequest {
        param_one: "test".to_string(),
        optional_param: None,
    }).await.unwrap();

    match response {
        DaemonResponse::MyNewResult { count, .. } => {
            assert!(count >= 0);
        }
        DaemonResponse::Error { message } => panic!("Unexpected error: {message}"),
        _ => panic!("Wrong response variant"),
    }
}
```

### 7. Wire CLI (if exposed as a subcommand)

Add a CLI command that connects to the daemon and sends the request:

```rust
Command::MyNewCommand { param_one } => {
    let socket = find_socket_path()?;
    let response = send_to_daemon(&socket, DaemonRequest::MyNewRequest {
        param_one,
        optional_param: None,
    })?;
    print_response(response, cli.json);
}
```

---

## Checklist

- [ ] DaemonRequest variant with serde derives
- [ ] DaemonResponse variant with serde derives
- [ ] Handler match arm in dispatch function
- [ ] Handler function with proper state lock hygiene
- [ ] Serialization round-trip test
- [ ] Integration test with DaemonGuard (if complex)
- [ ] CLI subcommand wired (if user-facing)
- [ ] `cargo test` passes
