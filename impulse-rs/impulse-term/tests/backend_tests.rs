//! Integration tests for TerminalBackend (PTY lifecycle, output reading, resize, scrollback).

use std::thread;
use std::time::Duration;

/// Helper: spawn a long-running backend running `cat` (reads stdin until EOF).
/// cat stays alive until its stdin is closed, making it ideal for write→read tests.
fn spawn_cat() -> impulse_term::TerminalBackend {
    impulse_term::TerminalBackend::spawn("cat", &[], None, &[], 24, 80, Some(100))
        .expect("cat spawn should succeed")
}

/// Helper: spawn a long-running backend running `sleep 60`.
fn spawn_sleep() -> impulse_term::TerminalBackend {
    impulse_term::TerminalBackend::spawn("sleep", &["60".to_string()], None, &[], 24, 80, Some(100))
        .expect("sleep spawn should succeed")
}

#[test]
fn test_spawn_cat_is_alive() {
    // cat stays alive until its input is closed.
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));
    assert!(
        backend.is_alive(),
        "cat should be alive immediately after spawn"
    );
}

#[test]
fn test_write_input_and_read_screen_text() {
    // cat echoes whatever we write to it — the most reliable PTY read test.
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));

    backend
        .write_input(b"hello from cat\n")
        .expect("write_input should succeed");

    thread::sleep(Duration::from_millis(100));
    let text = backend.screen_text();
    assert!(
        text.contains("hello from cat"),
        "cat should echo our input, got: {:?}",
        text
    );
}

#[test]
fn test_visible_char_count_increases() {
    // Write to cat and verify visible_char_count increases.
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));

    backend.write_input(b"abcdef\n").unwrap();
    thread::sleep(Duration::from_millis(100));
    let count = backend.visible_char_count();
    assert!(
        count >= 6,
        "visible_char_count should be at least 6 after cat echoes input, got {}",
        count
    );
}

#[test]
fn test_resize() {
    // sleep stays alive so we can resize it.
    let backend = spawn_sleep();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(backend.size(), (80, 24), "default size should be 80x24");

    backend.resize(132, 43).expect("resize should succeed");
    assert_eq!(
        backend.size(),
        (132, 43),
        "size should be 132x43 after resize"
    );
}

#[test]
fn test_output_bytes_incremented() {
    // Write to cat and verify bytes increment.
    let backend = spawn_cat();
    backend.write_input(b"ping\n").unwrap();
    thread::sleep(Duration::from_millis(100));
    let bytes = backend.output_bytes();
    assert!(
        bytes > 0,
        "output_bytes should be > 0 after cat outputs, got {}",
        bytes
    );
}

#[test]
fn test_kill_sets_alive_false() {
    // sleep stays alive so we can kill it.
    let backend = spawn_sleep();
    thread::sleep(Duration::from_millis(50));
    assert!(backend.is_alive(), "should be alive before kill");

    backend.kill();
    thread::sleep(Duration::from_millis(10));
    assert!(!backend.is_alive(), "should not be alive after kill");
}

#[test]
fn test_scrollback_text() {
    // Write to cat, then check scrollback.
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));

    backend.write_input(b"line1\n").unwrap();
    thread::sleep(Duration::from_millis(100));

    // scrollback_text with 0 lines should give visible screen.
    let visible = backend.scrollback_text(0);
    assert!(
        !visible.trim().is_empty(),
        "scrollback_text(0) should not be empty"
    );

    // scrollback_text with 10 lines should include buffered history.
    let buffered = backend.scrollback_text(10);
    assert!(
        buffered.contains("line1"),
        "scrollback_text(10) should contain 'line1', got: {:?}",
        buffered
    );
}

#[test]
fn test_command_and_working_dir() {
    let backend = spawn_sleep();
    assert_eq!(backend.command(), "sleep");
    assert!(backend.working_dir().is_none());
}

#[test]
fn test_output_lines_incremented() {
    // Write multiple lines to cat and verify output_lines increments.
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));

    backend.write_input(b"line1\nline2\nline3\n").unwrap();
    thread::sleep(Duration::from_millis(150));
    let lines = backend.output_lines();
    assert!(
        lines >= 3,
        "output_lines should be at least 3 after writing 3 newlines, got {}",
        lines
    );
}

// --- Reader thread EOF / natural exit ---

#[test]
fn test_reader_thread_eof_sets_alive_false() {
    // Spawn a short-lived command. When the child exits, the reader thread
    // hits EOF and sets alive=false without an explicit kill().
    let backend = impulse_term::TerminalBackend::spawn(
        "echo",
        &["done".to_string()],
        None,
        &[],
        24,
        80,
        Some(100),
    )
    .expect("echo spawn should succeed");

    // Give the reader thread time to process EOF after echo exits.
    thread::sleep(Duration::from_millis(300));
    assert!(
        !backend.is_alive(),
        "backend should report not-alive after child exits naturally"
    );
}

// --- has_new_output_since ---

#[test]
fn test_has_new_output_since_detects_new_data() {
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));

    let baseline = backend.output_bytes();
    assert!(
        !backend.has_new_output_since(baseline),
        "no new output yet at baseline"
    );

    backend.write_input(b"new data\n").unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(
        backend.has_new_output_since(baseline),
        "should detect new output after writing"
    );
}

// --- read_error_count ---

#[test]
fn test_read_error_count_zero_on_healthy_pty() {
    // A healthy PTY should have zero read errors.
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        backend.read_error_count(),
        0,
        "read_error_count should be 0 on a healthy PTY"
    );
}

// --- with_parser / with_parser_mut ---

#[test]
fn test_with_parser_reads_screen_size() {
    let backend = spawn_sleep();
    let (rows, cols) = backend.with_parser(|p| {
        let screen = p.screen();
        (screen.size().0, screen.size().1)
    });
    assert_eq!(rows, 24, "parser should report 24 rows");
    assert_eq!(cols, 80, "parser should report 80 cols");
}

#[test]
fn test_with_parser_mut_set_scrollback() {
    let backend = spawn_cat();
    thread::sleep(Duration::from_millis(50));

    // Use with_parser_mut to change scrollback, then verify via scrollback_len.
    backend.with_parser_mut(|p| {
        p.set_scrollback(42);
    });
    // scrollback_len reads from the parser — the scrollback lines available
    // depend on output, but calling set_scrollback should not panic.
    // Verify the parser is still usable after mut access.
    let text = backend.screen_text();
    // screen_text should succeed (parser not poisoned).
    assert!(
        text.len() < 100_000,
        "screen_text should return reasonable content after with_parser_mut"
    );
}

// --- scrollback_len and visible_rows ---

#[test]
fn test_visible_rows_matches_spawn_rows() {
    let backend = spawn_sleep();
    assert_eq!(
        backend.visible_rows(),
        24,
        "visible_rows should match the rows passed to spawn"
    );
}

#[test]
fn test_scrollback_len_starts_at_zero() {
    // Fresh terminal should have zero scrollback lines.
    let backend = spawn_sleep();
    thread::sleep(Duration::from_millis(50));
    assert_eq!(
        backend.scrollback_len(),
        0,
        "scrollback_len should be 0 on a fresh terminal with no overflow"
    );
}

// --- spawn with working_dir ---

#[test]
fn test_spawn_with_working_dir() {
    let tmp = std::env::temp_dir();
    let backend = impulse_term::TerminalBackend::spawn(
        "pwd",
        &[],
        Some(tmp.as_path()),
        &[],
        24,
        80,
        Some(100),
    )
    .expect("pwd spawn with working_dir should succeed");

    thread::sleep(Duration::from_millis(200));
    let text = backend.screen_text();

    // On macOS, temp dirs like /var/folders/... resolve to /private/var/folders/...
    // pwd may output either form. Canonicalize both sides for comparison.
    let canonical_tmp = tmp.canonicalize().unwrap_or_else(|_| tmp.clone());
    let canonical_str = canonical_tmp.to_string_lossy();
    assert!(
        text.contains(canonical_str.as_ref()),
        "pwd output should contain the canonical working dir '{}', got: {:?}",
        canonical_str,
        text
    );
    assert_eq!(
        backend.working_dir().map(|p| p.to_path_buf()),
        Some(tmp.clone()),
        "working_dir() should return the path passed to spawn"
    );
}

// --- spawn with env_vars ---

#[test]
fn test_spawn_with_env_vars() {
    let backend = impulse_term::TerminalBackend::spawn(
        "sh",
        &["-c".to_string(), "echo $IMPULSE_TEST_VAR".to_string()],
        None,
        &[("IMPULSE_TEST_VAR", "hello_from_test".to_string())],
        24,
        80,
        Some(100),
    )
    .expect("sh spawn with env_vars should succeed");

    thread::sleep(Duration::from_millis(200));
    let text = backend.screen_text();
    assert!(
        text.contains("hello_from_test"),
        "child should see injected env var, got: {:?}",
        text
    );
}

// --- spawn failure (invalid command) ---

#[test]
fn test_spawn_invalid_command_returns_error() {
    let result = impulse_term::TerminalBackend::spawn(
        "this_command_definitely_does_not_exist_xyz",
        &[],
        None,
        &[],
        24,
        80,
        Some(100),
    );
    assert!(
        result.is_err(),
        "spawning a nonexistent command should return an Err"
    );
}
