//! Shared subprocess helpers.
//!
//! [`run_with_timeout`] bounds any external command so a stuck or hung child
//! (a wedged CLI, a network credential fetch that never returns) can't block
//! the caller — or the daemon — indefinitely.

use std::io::{self, Read};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Run a command with a hard timeout.
///
/// stdout/stderr are drained on background threads so a child producing more
/// than the pipe buffer can hold can't deadlock (and thus never exit). If the
/// child exceeds `timeout` it is killed and a `TimedOut` error is returned.
pub fn run_with_timeout(mut command: Command, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let child_stdout = child.stdout.take();
    let child_stderr = child.stderr.take();
    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = child_stdout {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = child_stderr {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("command timed out after {}s", timeout.as_secs()),
            ));
        }
        std::thread::sleep(Duration::from_millis(15));
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
