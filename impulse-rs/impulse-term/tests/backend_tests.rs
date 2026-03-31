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
