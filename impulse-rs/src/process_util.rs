//! Shared subprocess helpers.
//!
//! [`run_with_timeout`] bounds an external command in time and in memory, so
//! a stuck child (a wedged CLI, a network credential fetch that never
//! returns) or a runaway one can't hold up or exhaust the caller.

use std::io::{self, Read};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

/// Most stdout a command may write. Past it the child is killed and the call
/// fails: a cut-off answer (half a JSON document, part of a secret) is not
/// one to act on.
pub const MAX_STDOUT_BYTES: usize = 32 * 1024 * 1024;

/// Most stderr kept. Stderr only feeds error messages, so the rest is read
/// and dropped rather than failing the call.
pub const MAX_STDERR_BYTES: usize = 64 * 1024;

/// How often the child is polled for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(15);

/// Once the child has exited, how long its pipes may take to reach end of
/// file even past the deadline: the readers may still be draining the last
/// buffered output of a child that finished just in time.
const DRAIN_GRACE: Duration = Duration::from_millis(250);

/// How long to wait for a killed child to be reaped. SIGKILL ends a normal
/// process at once; one stuck in uninterruptible I/O is left behind rather
/// than blocking the caller.
const REAP_GRACE: Duration = Duration::from_secs(2);

/// Read size for draining a pipe.
const CHUNK_BYTES: usize = 8 * 1024;

/// Run a command with a hard timeout and bounded output.
///
/// stdin is closed. stdout and stderr are read on background threads while
/// the child runs, so a child writing more than a pipe holds can't deadlock.
/// The call returns by `timeout` (plus a short drain grace) in every case:
///
/// - a child still running at the deadline is killed, and the call fails
///   with `TimedOut`;
/// - a child that exited while something it started still holds its stdout
///   or stderr open (a backgrounded grandchild) also fails with `TimedOut`
///   at the deadline, instead of waiting for that process to exit;
/// - stdout past [`MAX_STDOUT_BYTES`] kills the child and fails the call;
///   stderr past [`MAX_STDERR_BYTES`] is dropped.
///
/// The child stays in the caller's process group, so a terminal's Ctrl-C
/// still reaches it and it can prompt on the terminal (a secrets manager
/// asking to unlock). In exchange, only the child itself is killed at the
/// deadline, not processes it started.
pub fn run_with_timeout(command: Command, timeout: Duration) -> io::Result<Output> {
    run_with_limits(command, timeout, MAX_STDOUT_BYTES, MAX_STDERR_BYTES)
}

fn run_with_limits(
    mut command: Command,
    timeout: Duration,
    stdout_cap: usize,
    stderr_cap: usize,
) -> io::Result<Output> {
    // `None` when `timeout` is too long to represent: no deadline at all.
    let deadline = Instant::now().checked_add(timeout);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let readers = spawn_reader(child.stdout.take(), stdout_cap, OnCap::Stop).and_then(|out| {
        spawn_reader(child.stderr.take(), stderr_cap, OnCap::Drain).map(|err| (out, err))
    });
    let (stdout_rx, stderr_rx) = match readers {
        Ok(readers) => readers,
        Err(e) => {
            kill_and_reap(&mut child);
            return Err(e);
        }
    };

    // stdout can finish before the child exits (it closed stdout, or wrote
    // past the cap); keep that result rather than polling for it again.
    let mut stdout = None;
    let status = loop {
        if stdout.is_none() {
            stdout = stdout_rx.try_recv().ok();
        }
        if stdout.as_ref().is_some_and(|out: &Captured| out.over_cap) {
            kill_and_reap(&mut child);
            return Err(stdout_over_cap(stdout_cap));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                kill_and_reap(&mut child);
                return Err(e);
            }
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            kill_and_reap(&mut child);
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("command timed out after {timeout:?}"),
            ));
        }
        thread::sleep(POLL_INTERVAL);
    };

    // The output is complete once both pipes reach end of file, which a
    // process the child started can postpone indefinitely by holding them.
    let drain_until = deadline.map(|deadline| deadline.max(Instant::now() + DRAIN_GRACE));
    let stdout = match stdout {
        Some(stdout) => stdout,
        None => receive(&stdout_rx, drain_until, timeout)?,
    };
    if stdout.over_cap {
        return Err(stdout_over_cap(stdout_cap));
    }
    let stderr = receive(&stderr_rx, drain_until, timeout)?;
    Ok(Output {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

/// What a reader does once its pipe has delivered `cap` bytes.
#[derive(Clone, Copy)]
enum OnCap {
    /// Stop reading and close the pipe; the result is marked over the cap.
    Stop,
    /// Keep reading and dropping bytes, so the child never blocks on a full
    /// pipe.
    Drain,
}

/// The first bytes a pipe delivered, up to the cap.
struct Captured {
    bytes: Vec<u8>,
    over_cap: bool,
}

/// Reads `pipe` on its own thread, which sends one [`Captured`] when the
/// pipe reaches end of file (or the cap, under [`OnCap::Stop`]).
fn spawn_reader<R>(
    pipe: Option<R>,
    cap: usize,
    on_cap: OnCap,
) -> io::Result<mpsc::Receiver<Captured>>
where
    R: Read + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("process-output".to_string())
        .spawn(move || {
            let captured = match pipe {
                Some(pipe) => read_capped(pipe, cap, on_cap),
                None => Captured {
                    bytes: Vec::new(),
                    over_cap: false,
                },
            };
            // The caller may have stopped waiting (timeout); nothing to do.
            let _ = tx.send(captured);
        })?;
    Ok(rx)
}

fn read_capped(mut pipe: impl Read, cap: usize, on_cap: OnCap) -> Captured {
    let mut bytes = Vec::new();
    let mut over_cap = false;
    let mut chunk = [0u8; CHUNK_BYTES];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let room = cap.saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..n.min(room)]);
                if n > room {
                    over_cap = true;
                    if matches!(on_cap, OnCap::Stop) {
                        break;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // A broken pipe ends the output like end of file does.
            Err(_) => break,
        }
    }
    Captured { bytes, over_cap }
}

/// Waits for a reader's result until `until` (forever when `None`).
fn receive(
    rx: &mpsc::Receiver<Captured>,
    until: Option<Instant>,
    timeout: Duration,
) -> io::Result<Captured> {
    let received = match until {
        Some(until) => rx.recv_timeout(until.saturating_duration_since(Instant::now())),
        None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
    };
    match received {
        Ok(captured) => Ok(captured),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "command exited, but its output was still open after {timeout:?}; \
                 a process it started may be holding it"
            ),
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::other(
            "command output reader stopped without a result",
        )),
    }
}

fn stdout_over_cap(cap: usize) -> io::Error {
    io::Error::other(format!("command wrote more than {cap} bytes to stdout"))
}

/// Kills the child and reaps it, waiting at most [`REAP_GRACE`].
fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let give_up = Instant::now() + REAP_GRACE;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= give_up => return,
            Ok(None) => thread::sleep(POLL_INTERVAL),
        }
    }
}

/// Unique `sleep` processes for tests that leave one running and must stop
/// exactly that one afterwards.
#[cfg(test)]
pub(crate) mod test_sleep {
    use std::sync::atomic::{AtomicU32, Ordering};

    static NEXT: AtomicU32 = AtomicU32::new(0);

    /// A `sleep` argument of about `whole_seconds` whose fraction no other
    /// call shares, in this process or any other running one. (The clock
    /// can't provide that: on macOS `SystemTime` nanoseconds are whole
    /// microseconds, so `subsec_nanos() % 1000` is always 0.)
    pub(crate) fn unique_duration(whole_seconds: u32) -> String {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        format!("{whole_seconds}.{:010}{n:05}", std::process::id())
    }

    /// Kills the `sleep` started with `duration`, if it is still running.
    pub(crate) fn stop(duration: &str) {
        let _ = std::process::Command::new("pkill")
            .arg("-f")
            .arg(format!("^sleep {}$", duration.replace('.', "\\.")))
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script);
        cmd
    }

    #[test]
    fn test_run_with_timeout_captures_output() {
        let mut cmd = Command::new("printf");
        cmd.arg("hello-proc");
        let output = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hello-proc");
    }

    #[test]
    fn test_run_with_timeout_kills_slow_command() {
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let start = Instant::now();
        let err = run_with_timeout(cmd, Duration::from_millis(150)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        // The call must return promptly after the timeout, not after `sleep`.
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed-out command should return promptly, took {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn test_run_with_timeout_nonzero_exit() {
        // `false` exits non-zero quickly; status must reflect failure.
        let cmd = Command::new("false");
        let output = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert!(!output.status.success());
    }

    #[test]
    fn test_run_with_timeout_missing_program_is_an_error() {
        let cmd = Command::new("impulse-no-such-program-for-process-util");
        let err = run_with_timeout(cmd, Duration::from_secs(5)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// Review finding: the deadline only covered `wait`, so a child that
    /// exited while a backgrounded process still held stdout kept the call
    /// waiting for that process, far past the timeout.
    #[test]
    fn test_run_with_timeout_returns_at_the_deadline_when_a_grandchild_holds_stdout() {
        let duration = test_sleep::unique_duration(5);
        let start = Instant::now();
        let result = run_with_timeout(
            sh(&format!("sleep {duration} & echo started")),
            Duration::from_millis(500),
        );
        let elapsed = start.elapsed();
        test_sleep::stop(&duration);

        let err = result.expect_err("output still held open is not a finished command");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(err.to_string().contains("still open"), "{err}");
        assert!(
            elapsed < Duration::from_secs(2),
            "a 500 ms timeout took {elapsed:?}"
        );
    }

    #[test]
    fn test_run_with_limits_fails_when_stdout_passes_the_cap() {
        let err = run_with_limits(
            sh("head -c 100000 /dev/zero"),
            Duration::from_secs(10),
            1000,
            1000,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("more than 1000 bytes to stdout"),
            "{err}"
        );
    }

    /// Review finding: nothing capped the output, so a child that never
    /// stopped writing grew the caller's memory until the deadline. Past the
    /// cap it is now killed at once.
    #[test]
    fn test_run_with_limits_kills_an_endless_writer_at_the_cap_not_the_deadline() {
        let start = Instant::now();
        let err =
            run_with_limits(Command::new("yes"), Duration::from_secs(10), 1000, 1000).unwrap_err();
        assert!(err.to_string().contains("more than 1000 bytes"), "{err}");
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "took {:?}; the cap should end the call long before the 10 s deadline",
            start.elapsed()
        );
    }

    #[test]
    fn test_run_with_limits_keeps_stdout_of_exactly_the_cap() {
        let output = run_with_limits(
            sh("head -c 1000 /dev/zero"),
            Duration::from_secs(10),
            1000,
            1000,
        )
        .unwrap();
        assert_eq!(output.stdout.len(), 1000);
    }

    #[test]
    fn test_run_with_limits_cuts_stderr_and_still_succeeds() {
        let output = run_with_limits(
            sh("head -c 100000 /dev/zero >&2; echo ok"),
            Duration::from_secs(10),
            1000,
            1000,
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stderr.len(), 1000);
        assert_eq!(output.stdout, b"ok\n");
    }

    #[test]
    fn test_read_capped_counts_a_cap_of_zero_as_over_on_the_first_byte() {
        let stopped = read_capped(&b"abc"[..], 0, OnCap::Stop);
        assert!(stopped.bytes.is_empty());
        assert!(stopped.over_cap);

        let empty = read_capped(&b""[..], 0, OnCap::Stop);
        assert!(!empty.over_cap);
    }
}
