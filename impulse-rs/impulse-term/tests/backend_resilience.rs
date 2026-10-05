//! The reader thread keeps draining whatever the output does, `kill()` gives
//! up at its deadline, and sizes and scrollback offsets stay inside what
//! vt100 0.15 handles. Each test drives a real PTY running `sh`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use impulse_term::backend::OutputCallback;
use impulse_term::TerminalBackend;

fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

fn sh_args(script: &str, extra: &[String]) -> Vec<String> {
    let mut args = vec!["-c".to_string(), script.to_string(), "sh".to_string()];
    args.extend(extra.iter().cloned());
    args
}

fn exit_flag() -> (Arc<AtomicBool>, impulse_term::backend::ExitCallback) {
    let exited = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&exited);
    (exited, Arc::new(move || flag.store(true, Ordering::SeqCst)))
}

/// Review finding: after a narrowing resize cut a wide character, the next
/// redraw panicked inside vt100 and killed the reader thread. Output stopped,
/// the exit callback never ran, and `kill()` waited forever.
#[test]
fn reader_keeps_draining_after_a_resize_cuts_a_wide_character() {
    let script = r#"printf '%s' "$1"; sleep 1; printf '\033[1;1H\033[K'; sleep 0.5; printf 'after-marker\n'; sleep 0.3"#;
    let args = sh_args(script, &["\u{4e2d}".repeat(40)]);
    let (exited, on_exit) = exit_flag();
    let backend = Arc::new(
        TerminalBackend::spawn_with_callbacks(
            "sh",
            &args,
            None,
            &[],
            24,
            80,
            Some(1000),
            None,
            Some(on_exit),
        )
        .expect("spawn sh"),
    );
    assert!(wait_until(Duration::from_secs(3), || backend
        .screen_text()
        .contains('\u{4e2d}')));
    backend.resize(79, 24).expect("resize");

    assert!(
        wait_until(Duration::from_secs(5), || backend
            .screen_text()
            .contains("after-marker")),
        "output stopped after the parser panicked"
    );
    assert!(
        wait_until(Duration::from_secs(5), || exited.load(Ordering::SeqCst)),
        "the exit callback never ran"
    );
    assert!(!backend.is_alive());
    assert!(backend.kill_within(Duration::from_secs(5)).is_ok());
}

/// A panicking output callback used to kill the reader thread the same way.
#[test]
fn reader_keeps_draining_after_the_output_callback_panics() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let output: OutputCallback = Arc::new(move |_data: &[u8]| {
        if counted.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("output callback bug");
        }
    });
    let (exited, on_exit) = exit_flag();
    let args = sh_args(
        "printf first; sleep 0.5; printf ' second-marker'; sleep 0.3",
        &[],
    );
    let backend = TerminalBackend::spawn_with_callbacks(
        "sh",
        &args,
        None,
        &[],
        24,
        80,
        Some(100),
        Some(output),
        Some(on_exit),
    )
    .expect("spawn sh");

    assert!(
        wait_until(Duration::from_secs(5), || backend
            .screen_text()
            .contains("second-marker")),
        "output stopped after the callback panicked"
    );
    assert!(wait_until(Duration::from_secs(5), || exited.load(Ordering::SeqCst)));
    assert!(calls.load(Ordering::SeqCst) >= 2);
}

/// Review finding: with nothing draining the PTY, a killed child cannot
/// finish exiting, and `kill()` waited for it without a limit. The desktop's
/// launch gate parks the output callback that way on its failure paths.
#[test]
fn kill_returns_by_its_deadline_while_output_is_not_drained() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let parked = Arc::clone(&gate);
    let output: OutputCallback = Arc::new(move |_data: &[u8]| {
        let (open, opened) = &*parked;
        let mut open = open.lock().unwrap();
        while !*open {
            open = opened.wait(open).unwrap();
        }
    });
    let backend = Arc::new(
        TerminalBackend::spawn_with_callbacks(
            "sh",
            &sh_args("seq 1 20000; sleep 30", &[]),
            None,
            &[],
            24,
            80,
            Some(1000),
            Some(output),
            None,
        )
        .expect("spawn sh"),
    );
    std::thread::sleep(Duration::from_millis(500));

    let (sent, received) = mpsc::channel();
    let killer = Arc::clone(&backend);
    std::thread::spawn(move || {
        let started = Instant::now();
        let result = killer
            .kill_within(Duration::from_millis(500))
            .map_err(|e| e.to_string());
        let _ = sent.send((started.elapsed(), result));
    });
    let outcome = received.recv_timeout(Duration::from_secs(5));
    {
        let (open, opened) = &*gate;
        *open.lock().unwrap() = true;
        opened.notify_all();
    }
    let (waited, result) = outcome.expect("kill() was still waiting after 5 s");
    assert!(waited < Duration::from_secs(3), "kill() waited {waited:?}");
    if let Err(message) = result {
        assert!(message.contains("had not exited"), "{message}");
    }
    // Once the output drains again, the child finishes exiting.
    assert!(wait_until(Duration::from_secs(10), || backend
        .kill_within(Duration::from_secs(1))
        .is_ok()));
}

/// Verification finding: `kill_within` held the child lock while it waited,
/// so `is_alive()` from a UI thread stalled for the whole deadline.
#[test]
fn is_alive_answers_while_kill_waits() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let parked = Arc::clone(&gate);
    let output: OutputCallback = Arc::new(move |_data: &[u8]| {
        let (open, opened) = &*parked;
        let mut open = open.lock().unwrap();
        while !*open {
            open = opened.wait(open).unwrap();
        }
    });
    let backend = Arc::new(
        TerminalBackend::spawn_with_callbacks(
            "sh",
            &sh_args("seq 1 20000; sleep 30", &[]),
            None,
            &[],
            24,
            80,
            Some(1000),
            Some(output),
            None,
        )
        .expect("spawn sh"),
    );
    std::thread::sleep(Duration::from_millis(500));

    let killer = Arc::clone(&backend);
    let kill = std::thread::spawn(move || killer.kill_within(Duration::from_secs(2)).is_ok());
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    let _ = backend.is_alive();
    let waited = started.elapsed();
    {
        let (open, opened) = &*gate;
        *open.lock().unwrap() = true;
        opened.notify_all();
    }
    let _ = kill.join();
    assert!(
        waited < Duration::from_millis(500),
        "is_alive() waited {waited:?}"
    );
    assert!(wait_until(Duration::from_secs(10), || backend
        .kill_within(Duration::from_secs(1))
        .is_ok()));
}

/// Review finding: vt100 0.15 panics on ordinary escape sequences in a
/// terminal narrower than 3 columns or shorter than 3 rows.
#[test]
fn terminals_smaller_than_three_by_three_are_raised_to_it() {
    let backend = TerminalBackend::spawn(
        "sh",
        &sh_args("printf 'abc\\033[K\\033[2J'; sleep 5", &[]),
        None,
        &[],
        1,
        1,
        Some(100),
    )
    .expect("spawn sh");
    assert_eq!(backend.size(), (3, 3));
    assert_eq!(backend.with_parser(|parser| parser.screen().size()), (3, 3));
    backend.resize(2, 1).expect("resize");
    assert_eq!(backend.size(), (3, 3));
    assert_eq!(backend.with_parser(|parser| parser.screen().size()), (3, 3));
    let _ = backend.kill();
}

/// Review findings: `scrollback_len` returned the current scroll offset, so
/// the view could not scroll back, and an offset past one screen underflowed
/// inside vt100 0.15 and left it stuck there.
#[test]
fn scrollback_reaches_one_screen_of_history_without_panicking() {
    let backend = TerminalBackend::spawn(
        "sh",
        &sh_args("seq 1 100; sleep 5", &[]),
        None,
        &[],
        10,
        40,
        Some(1000),
    )
    .expect("spawn sh");
    assert!(wait_until(Duration::from_secs(3), || backend
        .screen_text()
        .contains("100")));

    // About 91 lines of history, of which one screen can be shown.
    assert_eq!(backend.scrollback_len(), 10);
    let history = backend.scrollback_text(48);
    assert_eq!(history.lines().next(), Some("82"), "{history}");
    assert!(backend.screen_text().contains("100"));
    let _ = backend.kill();
}
