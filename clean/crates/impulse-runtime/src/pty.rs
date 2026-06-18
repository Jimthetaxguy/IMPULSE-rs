//! PTY spawn + read loop.

use impulse_contracts::PaneId;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, warn};

/// What we need to spawn a child.
#[derive(Clone, Debug)]
pub struct PtySpawnSpec {
    /// Program path (resolved by caller).
    pub program: String,
    /// Args.
    pub args: Vec<String>,
    /// Environment variables.
    pub env: Vec<(String, String)>,
    /// Working directory.
    pub working_dir: PathBuf,
    /// Initial cols.
    pub cols: u16,
    /// Initial rows.
    pub rows: u16,
}

impl PtySpawnSpec {
    /// Build a spec from the contracts-layer `CliSubprocessSpec`.
    #[must_use]
    pub fn from_cli(
        cli: &impulse_contracts::CliSubprocessSpec,
        workspace_root: &std::path::Path,
        cols: u16,
        rows: u16,
    ) -> Self {
        let working_dir = cli
            .working_dir
            .clone()
            .unwrap_or_else(|| workspace_root.to_path_buf());
        let mut env: Vec<(String, String)> = cli
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Inherit PATH by default — useful for any backend that shells out.
        if let Ok(path) = std::env::var("PATH") {
            env.push(("PATH".to_owned(), path));
        }
        Self {
            program: cli.program.clone(),
            args: cli.args.clone(),
            env,
            working_dir,
            cols,
            rows,
        }
    }
}

/// What a [`PtyHandle`] produces on its output channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtyOutput {
    /// Pane id (multi-pane sessions only; for now always one pane per session).
    pub pane_id: PaneId,
    /// Bytes read from the child.
    pub bytes: Vec<u8>,
}

/// Live handle to a PTY child process.
pub struct PtyHandle {
    /// Pane id this handle is for.
    pub pane_id: PaneId,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    killer: Arc<Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>>,
    output_rx: mpsc::Receiver<PtyOutput>,
    exit_tx: broadcast::Sender<Option<i32>>,
}

impl PtyHandle {
    /// Spawn the PTY and start the read loop. Returns a handle that owns the child.
    ///
    /// # Errors
    /// Returns [`PtyError::Spawn`] if `portable_pty` cannot start the child.
    pub fn spawn(pane_id: PaneId, spec: PtySpawnSpec) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&spec.program);
        for arg in &spec.args {
            cmd.arg(arg);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        cmd.cwd(&spec.working_dir);

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        let killer = child.clone_killer();
        drop(pair.slave);

        let master = pair.master;
        let mut reader = master
            .try_clone_reader()
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let (output_tx, output_rx) = mpsc::channel::<PtyOutput>(256);
        let (exit_tx, _exit_rx) = broadcast::channel::<Option<i32>>(1);

        // Reader thread: read all bytes and forward to output channel.
        let pane_for_thread = pane_id;
        let output_tx_thread = output_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let chunk = PtyOutput {
                            pane_id: pane_for_thread,
                            bytes: buf[..n].to_vec(),
                        };
                        if output_tx_thread.blocking_send(chunk).is_err() {
                            break; // receiver dropped
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "pty read error");
                        break;
                    }
                }
            }
            debug!("pty reader thread exiting");
        });

        // Waiter thread: wait for child exit and forward the code.
        let exit_tx_thread = exit_tx.clone();
        std::thread::spawn(move || {
            let status = child.wait();
            let code = status.map(|s| s.exit_code() as i32).ok();
            let _ = exit_tx_thread.send(code);
            // Drain the output channel so the reader thread can exit cleanly.
            drop(output_tx);
        });

        Ok(Self {
            pane_id,
            master: Arc::new(Mutex::new(master)),
            killer: Arc::new(Mutex::new(Some(killer))),
            output_rx,
            exit_tx,
        })
    }

    /// Resize the PTY.
    pub fn resize(&self, cols: u16, rows: u16) {
        let master = self.master.lock();
        let _ = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Write bytes to the child stdin.
    ///
    /// # Errors
    /// Returns [`PtyError::Write`] if the master is closed.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        let mut master = self.master.lock();
        let mut writer = master
            .take_writer()
            .map_err(|e| PtyError::Write(e.to_string()))?;
        use std::io::Write;
        writer
            .write_all(bytes)
            .map_err(|e| PtyError::Write(e.to_string()))?;
        // take_writer() consumes the writer; that's fine, we'll get a new one next call.
        Ok(())
    }

    /// Close the child. Returns the exit code if the child had already exited.
    pub async fn close(&mut self) -> Result<Option<i32>, PtyError> {
        if let Some(mut killer) = self.killer.lock().take() {
            let _ = killer.kill();
        }
        // Wait for the exit channel to deliver a code.
        let mut exit_rx = self.exit_tx.subscribe();
        match exit_rx.recv().await {
            Ok(code) => Ok(code),
            Err(_) => Ok(None),
        }
    }

    /// Take the output receiver (consumes the handle). Use this to read the
    /// PTY output stream from another task.
    #[must_use]
    pub fn into_output_stream(self) -> PtyOutputStream {
        PtyOutputStream {
            inner: self.output_rx,
        }
    }

    /// Borrow the output receiver so you can read while still owning the handle.
    pub fn output_stream_mut(&mut self) -> &mut mpsc::Receiver<PtyOutput> {
        &mut self.output_rx
    }
}

/// A detached PTY output stream.
pub struct PtyOutputStream {
    inner: mpsc::Receiver<PtyOutput>,
}

impl PtyOutputStream {
    /// Await the next chunk.
    pub async fn next(&mut self) -> Option<PtyOutput> {
        self.inner.recv().await
    }
}

/// Errors raised by the PTY layer.
#[derive(Debug, Error)]
pub enum PtyError {
    /// Could not open the PTY pair.
    #[error("open pty: {0}")]
    Open(String),

    /// Could not spawn the child.
    #[error("spawn child: {0}")]
    Spawn(String),

    /// Could not write to the child.
    #[error("write to child: {0}")]
    Write(String),

    /// The exit channel was unexpectedly closed.
    #[error("exit channel: {0}")]
    ExitChannel(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pty_spawn_spec_from_cli_sets_working_dir() {
        let cli = impulse_contracts::CliSubprocessSpec::new("echo");
        let spec = PtySpawnSpec::from_cli(&cli, std::path::Path::new("/tmp"), 80, 24);
        assert_eq!(spec.working_dir, PathBuf::from("/tmp"));
        assert_eq!(spec.cols, 80);
        assert_eq!(spec.rows, 24);
    }

    #[test]
    fn pty_spawn_spec_preserves_cli_working_dir() {
        let mut cli = impulse_contracts::CliSubprocessSpec::new("echo");
        cli.working_dir = Some(PathBuf::from("/var/tmp"));
        let spec = PtySpawnSpec::from_cli(&cli, std::path::Path::new("/tmp"), 80, 24);
        assert_eq!(spec.working_dir, PathBuf::from("/var/tmp"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spawn_echo_and_read_output() {
        use tokio::time::{timeout, Duration};
        let mut cli = impulse_contracts::CliSubprocessSpec::new("echo");
        cli.args = vec!["hello-impulse".to_owned()];
        let spec = PtySpawnSpec::from_cli(&cli, std::path::Path::new("/tmp"), 80, 24);
        let mut handle = PtyHandle::spawn(PaneId::new(), spec).expect("spawn");
        let mut stream = handle.output_stream_mut();
        let chunk = timeout(Duration::from_secs(2), stream.recv())
            .await
            .expect("timely output")
            .expect("some output");
        assert!(String::from_utf8_lossy(&chunk.bytes).contains("hello-impulse"));
    }
}
