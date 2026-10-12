//! Cancellation-safe ownership of a spawned Unix process group.
//!
//! Tokio's `kill_on_drop(true)` kills only the direct child. When that child
//! is a shell or harness wrapper, grandchildren can survive cancellation. A
//! guard created immediately after spawning synchronously kills the negative
//! process-group id from `Drop`, covering timeout returns, task aborts, and
//! unwinding without requiring an async cleanup future to keep running.

pub(crate) struct ProcessGroupGuard {
    #[cfg(unix)]
    pgid: Option<i32>,
    armed: bool,
}

impl ProcessGroupGuard {
    pub(crate) fn new(child_id: Option<u32>) -> Self {
        #[cfg(not(unix))]
        let _ = child_id;
        Self {
            #[cfg(unix)]
            pgid: child_id.and_then(|id| i32::try_from(id).ok()),
            armed: true,
        }
    }

    /// The direct child exited normally; ownership no longer needs cleanup.
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            // SAFETY: `pgid` came from the child created with
            // `process_group(0)`, so `-pgid` targets that isolated process
            // group. SIGKILL is best-effort; ESRCH simply means it already
            // exited. No Rust memory is shared with the signal operation.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
}
