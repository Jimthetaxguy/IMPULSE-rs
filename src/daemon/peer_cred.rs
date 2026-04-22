//! Peer credential introspection for Unix socket connections.
//!
//! # Purpose
//!
//! When the daemon accepts a connection on its Unix socket, it can determine
//! the peer's UID/PID via kernel-provided credentials. This is used for:
//! - Authorization (only the owning user's processes may connect)
//! - Auditing (log which process invoked which action)
//! - Same-user isolation on shared systems
//!
//! # Platform support
//!
//! | Platform | Mechanism | Returns |
//! |----------|-----------|---------|
//! | Linux    | `SO_PEERCRED` socket option | `uid`, `gid`, `pid` |
//! | macOS    | `LOCAL_PEERPID` + `getpeereid()` | `uid`, `gid`, `pid` |
//! | Other    | Not supported | `Err(PeerCredError::Unsupported)` |
//!
//! # Current status
//!
//! This module provides the skeleton — the function exists and is tested, but
//! it is **not yet wired into the daemon accept loop** at
//! [`super::handle_connection`]. Integration is tracked for a future loop.
//!
//! # First-principles alignment
//!
//! Rule #4 (Daemon as Library, Not Service): before the Phase 8 collapse, we
//! need an observable peer-identity API so library consumers can enforce their
//! own authorization policy.

use std::fmt;
use std::os::unix::net::UnixStream;

/// Peer credentials for a Unix socket connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// Effective user ID of the connecting peer.
    pub uid: u32,
    /// Effective group ID of the connecting peer.
    pub gid: u32,
    /// Process ID of the connecting peer (best-effort — may be 0 on some OSes
    /// if the kernel cannot determine it synchronously).
    pub pid: i32,
}

/// Error returned when peer credentials cannot be retrieved.
#[derive(Debug)]
pub enum PeerCredError {
    /// The underlying syscall returned an error.
    Io(std::io::Error),
    /// The current platform does not support peer credential lookup.
    Unsupported,
}

impl fmt::Display for PeerCredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerCredError::Io(e) => write!(f, "peer credentials I/O error: {e}"),
            PeerCredError::Unsupported => {
                write!(f, "peer credentials not supported on this platform")
            }
        }
    }
}

impl std::error::Error for PeerCredError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PeerCredError::Io(e) => Some(e),
            PeerCredError::Unsupported => None,
        }
    }
}

impl From<std::io::Error> for PeerCredError {
    fn from(e: std::io::Error) -> Self {
        PeerCredError::Io(e)
    }
}

/// Return the peer credentials for a connected Unix stream.
///
/// On Linux, uses the `SO_PEERCRED` socket option. On macOS, uses
/// `getpeereid()` for uid/gid and `LOCAL_PEERPID` via `getsockopt` for pid.
///
/// # Errors
///
/// - `PeerCredError::Io` if the syscall fails (e.g., the peer has
///   disconnected).
/// - `PeerCredError::Unsupported` on platforms other than Linux or macOS.
#[cfg(target_os = "linux")]
pub fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials, PeerCredError> {
    use std::os::unix::io::AsRawFd;

    // SO_PEERCRED returns a `struct ucred { pid_t pid; uid_t uid; gid_t gid; }`.
    #[repr(C)]
    #[derive(Default)]
    struct Ucred {
        pid: i32,
        uid: u32,
        gid: u32,
    }

    let fd = stream.as_raw_fd();
    let mut cred = Ucred::default();
    let mut len = std::mem::size_of::<Ucred>() as libc::socklen_t;

    // SAFETY: fd is a valid socket fd obtained from UnixStream::as_raw_fd.
    // `cred` is a stack-allocated POD struct large enough for SO_PEERCRED
    // output. `len` points to a valid socklen_t we own. `getsockopt` writes at
    // most `len` bytes and updates `len` to actual size written.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };

    if rc == -1 {
        return Err(PeerCredError::Io(std::io::Error::last_os_error()));
    }

    Ok(PeerCredentials {
        uid: cred.uid,
        gid: cred.gid,
        pid: cred.pid,
    })
}

/// Return the peer credentials for a connected Unix stream (macOS).
#[cfg(target_os = "macos")]
pub fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials, PeerCredError> {
    use std::os::unix::io::AsRawFd;

    let fd = stream.as_raw_fd();

    // getpeereid() gives uid/gid directly.
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: fd is a valid socket fd. uid/gid are valid pointers to owned
    // stack storage.
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    if rc == -1 {
        return Err(PeerCredError::Io(std::io::Error::last_os_error()));
    }

    // LOCAL_PEERPID via getsockopt(SOL_LOCAL, LOCAL_PEERPID, ...).
    // SOL_LOCAL is 0 on Darwin; LOCAL_PEERPID is 2.
    const SOL_LOCAL: libc::c_int = 0;
    const LOCAL_PEERPID: libc::c_int = 2;
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: fd valid, pid is a pointer to owned stack storage of the
    // expected size. getsockopt writes at most `len` bytes.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            SOL_LOCAL,
            LOCAL_PEERPID,
            &mut pid as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    // LOCAL_PEERPID is best-effort — if it fails, fall back to pid=0 rather
    // than failing the whole call, since uid/gid are the primary auth signal.
    let pid = if rc == -1 { 0 } else { pid };

    Ok(PeerCredentials { uid, gid, pid })
}

/// Fallback for unsupported platforms.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn peer_credentials(_stream: &UnixStream) -> Result<PeerCredentials, PeerCredError> {
    Err(PeerCredError::Unsupported)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream as StdUnixStream;

    #[test]
    fn test_peer_credentials_returns_current_user_on_socketpair() {
        // On a socketpair, both ends are owned by the current process, so the
        // peer credentials should match the current uid/gid.
        let (a, _b) = StdUnixStream::pair().expect("socketpair");
        let result = peer_credentials(&a);

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let creds = result.expect("peer_credentials should succeed on socketpair");
            // SAFETY: getuid/getgid always succeed — they have no failure mode.
            let current_uid = unsafe { libc::getuid() };
            let current_gid = unsafe { libc::getgid() };
            assert_eq!(
                creds.uid, current_uid,
                "peer uid {} != current uid {}",
                creds.uid, current_uid
            );
            assert_eq!(
                creds.gid, current_gid,
                "peer gid {} != current gid {}",
                creds.gid, current_gid
            );
            // pid may be 0 on macOS if LOCAL_PEERPID failed, but on Linux
            // SO_PEERCRED always fills it. Allow 0 as a valid fallback.
            if creds.pid != 0 {
                // SAFETY: getpid always succeeds.
                let current_pid = unsafe { libc::getpid() };
                assert_eq!(
                    creds.pid, current_pid,
                    "peer pid should be current pid when non-zero"
                );
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            assert!(matches!(result, Err(PeerCredError::Unsupported)));
        }
    }

    #[test]
    fn test_peer_cred_error_display_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "peer gone");
        let err = PeerCredError::Io(io_err);
        let msg = format!("{err}");
        assert!(msg.contains("peer credentials I/O error"));
        assert!(msg.contains("peer gone"));
    }

    #[test]
    fn test_peer_cred_error_display_unsupported() {
        let err = PeerCredError::Unsupported;
        let msg = format!("{err}");
        assert!(msg.contains("not supported"));
    }

    #[test]
    fn test_peer_cred_error_source_chain() {
        use std::error::Error;
        let io_err = std::io::Error::other("x");
        let err = PeerCredError::Io(io_err);
        assert!(err.source().is_some());

        let err = PeerCredError::Unsupported;
        assert!(err.source().is_none());
    }

    #[test]
    fn test_peer_cred_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: PeerCredError = io_err.into();
        assert!(matches!(err, PeerCredError::Io(_)));
    }

    #[test]
    fn test_peer_credentials_equality() {
        let a = PeerCredentials {
            uid: 501,
            gid: 20,
            pid: 1234,
        };
        let b = PeerCredentials {
            uid: 501,
            gid: 20,
            pid: 1234,
        };
        let c = PeerCredentials {
            uid: 502,
            gid: 20,
            pid: 1234,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_peer_credentials_debug_output() {
        let creds = PeerCredentials {
            uid: 501,
            gid: 20,
            pid: 1234,
        };
        let debug = format!("{creds:?}");
        assert!(debug.contains("501"));
        assert!(debug.contains("1234"));
    }
}
