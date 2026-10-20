//! Bounded capture of a child process's piped output.
//!
//! `Child::wait_with_output` keeps everything a child writes, so a command
//! such as `yes`, or `cat` on a large file, grows this process's memory
//! until the caller's timeout fires, and truncation only happens after the
//! fact. [`wait_with_capped_output`] keeps the first `cap` bytes of each
//! pipe and reads past the rest. Reading past the cap, rather than stopping
//! at it, means the child never blocks on a full pipe, so it can still exit
//! and report its status; the caller's timeout bounds a child that never
//! stops writing.

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Child;

/// Read size for draining a pipe.
const CHUNK_BYTES: usize = 64 * 1024;

/// What one pipe produced: its first bytes, up to the cap, and how many
/// bytes arrived in all.
#[derive(Debug)]
pub(crate) struct CappedOutput {
    pub(crate) bytes: Vec<u8>,
    pub(crate) total: u64,
}

impl CappedOutput {
    /// More arrived than was kept.
    pub(crate) fn truncated(&self) -> bool {
        self.total > self.bytes.len() as u64
    }
}

/// A finished child's exit status and its capped stdout and stderr.
#[derive(Debug)]
pub(crate) struct CappedChildOutput {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) stdout: CappedOutput,
    pub(crate) stderr: CappedOutput,
}

/// Reads `reader` to EOF, keeping the first `cap` bytes and counting the
/// rest.
pub(crate) async fn read_capped<R>(mut reader: R, cap: usize) -> std::io::Result<CappedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    let mut total = 0u64;
    let mut chunk = vec![0u8; CHUNK_BYTES];
    loop {
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            return Ok(CappedOutput { bytes, total });
        }
        total = total.saturating_add(n as u64);
        let room = cap.saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..n.min(room)]);
    }
}

/// Waits for `child` while reading its stdout and stderr through
/// [`read_capped`]. Both must have been spawned with `Stdio::piped()`.
///
/// Like `wait_with_output`, this returns once the child has exited and both
/// pipes are closed, so a backgrounded grandchild that keeps a pipe open
/// holds it until the caller's timeout.
pub(crate) async fn wait_with_capped_output(
    child: &mut Child,
    stdout_cap: usize,
    stderr_cap: usize,
) -> std::io::Result<CappedChildOutput> {
    let stdout = child.stdout.take().ok_or_else(|| not_piped("stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| not_piped("stderr"))?;
    let (status, stdout, stderr) = tokio::try_join!(
        child.wait(),
        read_capped(stdout, stdout_cap),
        read_capped(stderr, stderr_cap),
    )?;
    Ok(CappedChildOutput {
        status,
        stdout,
        stderr,
    })
}

fn not_piped(stream: &str) -> std::io::Error {
    std::io::Error::other(format!("the child's {stream} was not piped"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[tokio::test]
    async fn test_read_capped_keeps_the_prefix_and_counts_everything() {
        let input = (0..=255u8).cycle().take(200_000).collect::<Vec<u8>>();
        let out = read_capped(&input[..], 1000).await.unwrap();
        assert_eq!(out.bytes, input[..1000]);
        assert_eq!(out.total, 200_000);
        assert!(out.truncated());
    }

    #[tokio::test]
    async fn test_read_capped_under_the_cap_keeps_everything() {
        let out = read_capped(&b"hello"[..], 1000).await.unwrap();
        assert_eq!(out.bytes, b"hello");
        assert_eq!(out.total, 5);
        assert!(!out.truncated());
    }

    #[tokio::test]
    async fn test_read_capped_with_a_zero_cap_only_counts() {
        let out = read_capped(&b"abc"[..], 0).await.unwrap();
        assert!(out.bytes.is_empty());
        assert_eq!(out.total, 3);
        assert!(out.truncated());

        let empty = read_capped(&b""[..], 0).await.unwrap();
        assert!(!empty.truncated());
    }

    /// Each stream is far larger than a pipe buffer, so the child only
    /// exits because both pipes keep being drained past their caps.
    #[tokio::test]
    #[cfg(unix)]
    async fn test_wait_with_capped_output_drains_both_pipes_past_their_caps() {
        let mut child = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("head -c 3000000 /dev/zero; head -c 2000000 /dev/zero >&2; exit 3")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();

        let out = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            wait_with_capped_output(&mut child, 1000, 2000),
        )
        .await
        .expect("draining must let the child exit")
        .unwrap();

        assert_eq!(out.status.code(), Some(3));
        assert_eq!(out.stdout.bytes.len(), 1000);
        assert_eq!(out.stdout.total, 3_000_000);
        assert_eq!(out.stderr.bytes.len(), 2000);
        assert_eq!(out.stderr.total, 2_000_000);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_wait_with_capped_output_requires_piped_streams() {
        let mut child = tokio::process::Command::new("true")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let err = wait_with_capped_output(&mut child, 10, 10)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("stdout was not piped"), "{err}");
        let _ = child.wait().await;
    }
}
