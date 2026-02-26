//! Shared state between the background IPC poller and the UI thread.
//!
//! The background thread writes daemon data into `SharedState` via `Arc<Mutex<>>`.
//! The UI thread reads from it each frame. The background thread calls
//! `ctx.request_repaint()` to wake the UI after updates.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui;

use crate::ipc::{DaemonClient, DaemonStatus, Genome, HistoryEntry, SearchResult, Session};

// ---------------------------------------------------------------------------
// Poll intervals
// ---------------------------------------------------------------------------

const STATUS_POLL: Duration = Duration::from_secs(2);
const HISTORY_POLL: Duration = Duration::from_secs(10);
const GENOME_POLL: Duration = Duration::from_secs(30);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    #[allow(dead_code)]
    Connecting,
    Connected,
}

/// All daemon data cached for the UI.
pub struct SharedState {
    pub connection: ConnectionStatus,
    pub daemon_status: Option<DaemonStatus>,
    pub sessions: Vec<Session>,
    pub history: Vec<HistoryEntry>,
    pub genome: Option<Genome>,
    pub search_results: Vec<SearchResult>,
    pub search_query: String,
    pub search_in_progress: bool,
    pub last_poll: Option<Instant>,
    pub error: Option<String>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            connection: ConnectionStatus::Disconnected,
            daemon_status: None,
            sessions: Vec::new(),
            history: Vec::new(),
            genome: None,
            search_results: Vec::new(),
            search_query: String::new(),
            search_in_progress: false,
            last_poll: None,
            error: None,
        }
    }
}

pub type StateHandle = Arc<Mutex<SharedState>>;

/// Channel for sending one-off commands to the background poller.
#[derive(Debug)]
pub enum PollerCommand {
    /// Trigger an immediate refresh of all data.
    Refresh,
    /// Run a search query.
    Search(String),
    /// Shut down the poller thread.
    Shutdown,
}

/// Start the background daemon poller.
///
/// Returns a handle to the shared state, a command sender, and the thread join handle.
pub fn start_poller(
    ctx: egui::Context,
) -> (
    StateHandle,
    std::sync::mpsc::Sender<PollerCommand>,
    JoinHandle<()>,
) {
    let state: StateHandle = Arc::new(Mutex::new(SharedState::default()));
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<PollerCommand>();

    let state_clone = Arc::clone(&state);
    let handle = thread::Builder::new()
        .name("impulse-poller".into())
        .spawn(move || {
            poller_loop(state_clone, cmd_rx, ctx);
        })
        .expect("failed to spawn poller thread");

    (state, cmd_tx, handle)
}

/// Main loop for the background poller thread.
fn poller_loop(
    state: StateHandle,
    cmd_rx: std::sync::mpsc::Receiver<PollerCommand>,
    ctx: egui::Context,
) {
    let mut client = DaemonClient::discover();

    let mut last_status = Instant::now() - STATUS_POLL;
    let mut last_history = Instant::now() - HISTORY_POLL;
    let mut last_genome = Instant::now() - GENOME_POLL;
    let mut connected = false;

    loop {
        // Check for commands (non-blocking).
        match cmd_rx.try_recv() {
            Ok(PollerCommand::Shutdown) => {
                log::info!("Poller thread shutting down");
                return;
            }
            Ok(PollerCommand::Refresh) => {
                // Reset timers to force immediate refresh.
                last_status = Instant::now() - STATUS_POLL;
                last_history = Instant::now() - HISTORY_POLL;
                last_genome = Instant::now() - GENOME_POLL;
            }
            Ok(PollerCommand::Search(query)) => {
                run_search(&mut client, &state, &query);
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                log::info!("Poller command channel closed");
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        let now = Instant::now();

        // Try to connect / verify connection.
        if !connected || now.duration_since(last_status) >= STATUS_POLL {
            match client.ping() {
                Ok(true) => {
                    if !connected {
                        log::info!("Connected to daemon");
                        set_connection(&state, ConnectionStatus::Connected);
                        connected = true;
                        // Fetch everything on first connect.
                        last_history = Instant::now() - HISTORY_POLL;
                        last_genome = Instant::now() - GENOME_POLL;
                        ctx.request_repaint();
                    }

                    // Fetch status + sessions.
                    if let Ok(status) = client.status() {
                        if let Ok(sessions) = client.list_sessions() {
                            if let Ok(mut s) = state.lock() {
                                s.daemon_status = Some(status);
                                s.sessions = sessions;
                                s.last_poll = Some(Instant::now());
                                s.error = None;
                            }
                        }
                    }
                    last_status = now;
                    ctx.request_repaint();
                }
                _ => {
                    if connected {
                        log::warn!("Lost connection to daemon");
                        connected = false;
                    }
                    set_connection(&state, ConnectionStatus::Disconnected);
                    ctx.request_repaint();
                    thread::sleep(RECONNECT_DELAY);
                    last_status = now;
                    continue;
                }
            }
        }

        // Fetch history periodically.
        if connected && now.duration_since(last_history) >= HISTORY_POLL {
            if let Ok(entries) = client.list_history() {
                if let Ok(mut s) = state.lock() {
                    s.history = entries;
                }
                ctx.request_repaint();
            }
            last_history = now;
        }

        // Fetch genome periodically.
        if connected && now.duration_since(last_genome) >= GENOME_POLL {
            if let Ok(genome) = client.read_genome() {
                if let Ok(mut s) = state.lock() {
                    s.genome = Some(genome);
                }
                ctx.request_repaint();
            }
            last_genome = now;
        }

        // Sleep a bit to avoid busy-spinning.
        thread::sleep(Duration::from_millis(500));
    }
}

fn set_connection(state: &StateHandle, status: ConnectionStatus) {
    if let Ok(mut s) = state.lock() {
        s.connection = status;
    }
}

fn run_search(client: &mut DaemonClient, state: &StateHandle, query: &str) {
    if let Ok(mut s) = state.lock() {
        s.search_in_progress = true;
    }

    let results = client.search(query);

    if let Ok(mut s) = state.lock() {
        s.search_in_progress = false;
        match results {
            Ok(r) => {
                s.search_results = r;
                s.error = None;
            }
            Err(e) => {
                s.search_results.clear();
                s.error = Some(format!("Search failed: {}", e));
            }
        }
    }
}
