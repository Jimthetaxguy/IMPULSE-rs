//! Opt-in acceptance observer for the real packaged Dioxus host.
//!
//! This hook is deliberately inert during normal product use. A release
//! verifier activates it with a fresh nonce and an isolated filesystem root,
//! then captures the single receipt written to stderr. The observer never
//! installs a host API, loads fallback assets, or writes an evidence file: it
//! can only exercise the product bridge that [`crate::host_bridge`] mounted.

#![cfg(not(feature = "legacy-tauri-runtime"))]

use std::collections::{HashMap, HashSet};
use std::env;
use std::future::Future;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::runtime::DesktopEvent;
use impulse_ops::{DaemonInstanceIdentity, DAEMON_PROTOCOL_VERSION};

pub const PACKAGED_ACCEPTANCE_NONCE_ENV: &str = "IMPULSE_PACKAGED_ACCEPTANCE_NONCE";
pub const PACKAGED_ACCEPTANCE_ROOT_ENV: &str = "IMPULSE_PACKAGED_ACCEPTANCE_ROOT";
pub const PACKAGED_PROVENANCE_SHA256_ENV: &str = "IMPULSE_PACKAGED_PROVENANCE_SHA256";
pub const PACKAGED_DAEMON_PID_ENV: &str = "IMPULSE_PACKAGED_DAEMON_PID";
pub const PACKAGED_HOST_RECEIPT_PREFIX: &str = "IMPULSE_PACKAGED_HOST_RECEIPT ";

const RECEIPT_SCHEMA: &str = "impulse.packaged-host/v1";
const PROVENANCE_RELATIVE_PATH: &str = "Resources/ReleaseProvenance.v1.tsv";
/// Maximum time the in-app observer may spend collecting live-host evidence.
/// The external harness derives its receipt deadline from this value and adds
/// cleanup/transport grace, so the harness can always capture this observer's
/// structured timeout receipt rather than racing it.
pub const PACKAGED_OBSERVATION_TIMEOUT_SECS: u64 = 105;
const OBSERVATION_TIMEOUT: Duration = Duration::from_secs(PACKAGED_OBSERVATION_TIMEOUT_SECS);
pub const PACKAGED_TERMINAL_CLEANUP_TIMEOUT_SECS: u64 = 8;
const TERMINAL_CLEANUP_TIMEOUT: Duration =
    Duration::from_secs(PACKAGED_TERMINAL_CLEANUP_TIMEOUT_SECS);
pub const PACKAGED_INVOKE_DRAIN_TIMEOUT_SECS: u64 = 2;
const INVOKE_DRAIN_TIMEOUT: Duration = Duration::from_secs(PACKAGED_INVOKE_DRAIN_TIMEOUT_SECS);
const MAX_FAILURE_REASONS: usize = 24;
const MAX_REASON_CHARS: usize = 240;

#[derive(Debug, Clone)]
struct PackagedAcceptanceConfig {
    nonce: String,
    root: PathBuf,
    workspace_root: PathBuf,
    home: PathBuf,
    impulse_home: PathBuf,
    tmpdir: PathBuf,
    socket_path: PathBuf,
    provenance_sha256: String,
    expected_daemon_identity: DaemonInstanceIdentity,
}

#[derive(Debug, Clone, Default)]
struct PackagedHostTranscript {
    nonce: String,
    workspace_root: String,
    marker: String,
    expected_terminal_open_request: Value,
    read_only_arrays: HashSet<String>,
    unknown_command_rejected: bool,
    workspace_registered: bool,
    workspace_listed: bool,
    terminal_session: Option<String>,
    terminal_spawned: bool,
    terminal_opened: bool,
    terminal_input: bool,
    terminal_resized: bool,
    terminal_focused: bool,
    terminal_close_succeeded: bool,
    terminal_closed: bool,
    runtime_sessions: HashSet<String>,
    terminal_outputs: HashMap<String, Vec<u8>>,
    terminal_exits: HashSet<String>,
    daemon_connection_seen: bool,
    daemon_connected_current: bool,
    daemon_connection_error: Option<String>,
    pending_daemon_identity: Option<DaemonInstanceIdentity>,
    daemon_identity_verified: Option<DaemonInstanceIdentity>,
    overflowed: bool,
}

static PACKAGED_HOST_TRANSCRIPT: OnceLock<Mutex<Option<PackagedHostTranscript>>> = OnceLock::new();

#[derive(Debug, Clone, Default)]
struct DaemonAcceptanceState {
    connection_seen: bool,
    connected: bool,
    error: Option<String>,
    pending_identity: Option<DaemonInstanceIdentity>,
    connected_identity: Option<DaemonInstanceIdentity>,
}

impl DaemonAcceptanceState {
    fn apply(&mut self, event: &DesktopEvent) {
        match event {
            DesktopEvent::DaemonIdentityVerified { identity } => {
                self.pending_identity = Some(identity.clone());
            }
            DesktopEvent::OpsConnectionUpdate { connected, error } => {
                self.connection_seen = true;
                self.error = error.clone();
                if *connected && error.is_none() {
                    self.connected_identity = self.pending_identity.take();
                    self.connected = self.connected_identity.is_some();
                } else {
                    self.connected = false;
                    self.pending_identity = None;
                    self.connected_identity = None;
                }
            }
            _ => {}
        }
    }
}

static LATEST_DAEMON_CONNECTION: OnceLock<Mutex<DaemonAcceptanceState>> = OnceLock::new();
static PACKAGED_HOST_TRANSCRIPT_ACTIVE: AtomicBool = AtomicBool::new(false);
static PACKAGED_ACCEPTANCE_CANCELLED: AtomicBool = AtomicBool::new(false);
static PACKAGED_ACCEPTANCE_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct PackagedAcceptanceInvokeGuard;

impl Drop for PackagedAcceptanceInvokeGuard {
    fn drop(&mut self) {
        PACKAGED_ACCEPTANCE_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

enum AcceptanceActivation {
    Disabled,
    Ready(PackagedAcceptanceConfig),
    Invalid {
        nonce: String,
        provenance_sha256: String,
        failures: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct PackagedHostObservation {
    #[serde(default)]
    pub host_kind: String,
    #[serde(default)]
    pub host_status: String,
    #[serde(default)]
    pub xterm_loaded: bool,
    #[serde(default)]
    pub fit_addon_loaded: bool,
    #[serde(default)]
    pub stylesheet_loaded: bool,
    #[serde(default)]
    pub assets_local: bool,
    #[serde(default)]
    pub asset_paths_exact: bool,
    #[serde(default)]
    pub terminal_interop_mounted: bool,
    #[serde(default)]
    pub terminal_interop_degraded: bool,
    #[serde(default)]
    pub xterm_session_mounted: bool,
    #[serde(default)]
    pub ops_bridge_mounted: bool,
    #[serde(default)]
    pub tauri_absent: bool,
    #[serde(default)]
    pub injected_test_api_absent: bool,
    #[serde(default)]
    pub agent_snapshot_array: bool,
    #[serde(default)]
    pub agent_platforms_array: bool,
    #[serde(default)]
    pub workspaces_array: bool,
    #[serde(default)]
    pub mcp_descriptors_array: bool,
    #[serde(default)]
    pub review_queue_array: bool,
    #[serde(default)]
    pub unknown_command_rejected: bool,
    #[serde(default)]
    pub unknown_command_error: String,
    #[serde(default)]
    pub workspace_registered: bool,
    #[serde(default)]
    pub workspace_listed: bool,
    #[serde(default)]
    pub daemon_connected: bool,
    #[serde(default)]
    pub terminal_opened: bool,
    #[serde(default)]
    pub terminal_input: bool,
    #[serde(default)]
    pub terminal_output: bool,
    #[serde(default)]
    pub terminal_resized: bool,
    #[serde(default)]
    pub xterm_on_data_api_called: bool,
    #[serde(default)]
    pub xterm_on_resize_api_called: bool,
    #[serde(default)]
    pub xterm_output_buffer_rendered: bool,
    #[serde(default)]
    pub terminal_focused: bool,
    #[serde(default)]
    pub terminal_closed: bool,
    #[serde(default)]
    pub terminal_exited: bool,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PackagedHostReceipt<'a> {
    schema: &'static str,
    nonce: &'a str,
    pid: u32,
    crate_version: &'static str,
    provenance_sha256: &'a str,
    daemon_identity: Option<&'a DaemonInstanceIdentity>,
    rust_host_transcript_validated: bool,
    outcome: &'static str,
    observation: Option<&'a PackagedHostObservation>,
    failure_reasons: &'a [String],
}

const MAX_TRANSCRIPT_SESSIONS: usize = 64;
const MAX_TRANSCRIPT_OUTPUT_BYTES: usize = 64 * 1024;

fn transcript_slot() -> &'static Mutex<Option<PackagedHostTranscript>> {
    PACKAGED_HOST_TRANSCRIPT.get_or_init(|| Mutex::new(None))
}

fn expected_terminal_open_request(config: &PackagedAcceptanceConfig) -> Value {
    serde_json::json!({
        "session_id": null,
        "command": "/bin/sh",
        "args": [],
        "cwd": config.workspace_root,
        "env": {
            "HOME": config.home,
            "IMPULSE_HOME": config.impulse_home,
            "IMPULSE_SOCKET_PATH": config.socket_path,
            "TMPDIR": config.tmpdir,
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        },
        "workspace": null,
        "mcp_tools": [],
        "rows": 24,
        "cols": 80,
    })
}

fn begin_host_transcript(config: &PackagedAcceptanceConfig) {
    PACKAGED_ACCEPTANCE_CANCELLED.store(false, Ordering::Release);
    PACKAGED_ACCEPTANCE_INFLIGHT.store(0, Ordering::Release);
    // Keep the latest-daemon lock through transcript installation. This is the
    // same latest -> transcript order used by `record_host_event`, and closes
    // the gap where the only connected event could otherwise update the latch
    // before any transcript existed to receive it.
    let latest_daemon_guard = LATEST_DAEMON_CONNECTION
        .get_or_init(|| Mutex::new(DaemonAcceptanceState::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let latest_daemon = latest_daemon_guard.clone();
    let transcript = PackagedHostTranscript {
        nonce: config.nonce.clone(),
        workspace_root: config.workspace_root.to_string_lossy().into_owned(),
        marker: format!("IMPULSE_PACKAGED_PTY_RESULT_{}", config.nonce),
        expected_terminal_open_request: expected_terminal_open_request(config),
        daemon_connection_seen: latest_daemon.connection_seen,
        daemon_connected_current: latest_daemon.connected
            && latest_daemon.error.is_none()
            && latest_daemon.connected_identity.is_some(),
        daemon_connection_error: latest_daemon.error,
        pending_daemon_identity: latest_daemon.pending_identity,
        daemon_identity_verified: latest_daemon.connected_identity,
        ..Default::default()
    };
    let mut guard = transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(transcript);
    PACKAGED_HOST_TRANSCRIPT_ACTIVE.store(true, Ordering::Release);
    drop(guard);
    drop(latest_daemon_guard);
}

pub(crate) fn host_transcript_is_active() -> bool {
    PACKAGED_HOST_TRANSCRIPT_ACTIVE.load(Ordering::Acquire)
}

pub(crate) fn acceptance_is_cancelled() -> bool {
    PACKAGED_ACCEPTANCE_CANCELLED.load(Ordering::Acquire)
}

pub(crate) fn begin_acceptance_invoke(
    payload: &Value,
) -> Result<Option<PackagedAcceptanceInvokeGuard>, String> {
    if !host_transcript_is_active() && !PACKAGED_ACCEPTANCE_CANCELLED.load(Ordering::Acquire) {
        return Ok(None);
    }
    let acceptance_payload_matches = {
        let guard = transcript_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.as_ref().is_some_and(|transcript| {
            acceptance_nonce(payload) == Some(transcript.nonce.as_str())
                || is_exact_untagged_xterm_callback_payload(payload, transcript)
        })
    };
    if !acceptance_payload_matches {
        return Ok(None);
    }
    if PACKAGED_ACCEPTANCE_CANCELLED.load(Ordering::Acquire) {
        return Err("packaged acceptance command rejected after cancellation".to_string());
    }
    PACKAGED_ACCEPTANCE_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    if PACKAGED_ACCEPTANCE_CANCELLED.load(Ordering::Acquire) {
        PACKAGED_ACCEPTANCE_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        return Err("packaged acceptance command rejected after cancellation".to_string());
    }
    Ok(Some(PackagedAcceptanceInvokeGuard))
}

fn acceptance_nonce(payload: &Value) -> Option<&str> {
    payload.get("acceptance_nonce").and_then(Value::as_str)
}

fn request_body(payload: &Value) -> &Value {
    payload.get("request").unwrap_or(payload)
}

fn request_matches_session(payload: &Value, session: Option<&str>) -> bool {
    let Some(session) = session else {
        return false;
    };
    request_body(payload)
        .get("session_id")
        .and_then(Value::as_str)
        == Some(session)
}

fn request_is_exact_session(payload: &Value, session: Option<&str>) -> bool {
    let Some(session) = session else {
        return false;
    };
    request_body(payload) == &serde_json::json!({ "session_id": session })
}

fn request_is_exact_terminal_open(payload: &Value, transcript: &PackagedHostTranscript) -> bool {
    request_body(payload) == &transcript.expected_terminal_open_request
}

fn request_is_exact_resize(payload: &Value, session: Option<&str>) -> bool {
    let Some(session) = session else {
        return false;
    };
    request_body(payload) == &serde_json::json!({ "session_id": session, "rows": 31, "cols": 97 })
}

fn request_is_exact_agent_write(payload: &Value, transcript: &PackagedHostTranscript) -> bool {
    let Some(session) = transcript.terminal_session.as_deref() else {
        return false;
    };
    payload
        .as_object()
        .is_some_and(|object| object.len() == 1 && object.contains_key("request"))
        && request_body(payload)
            == &serde_json::json!({
                "agent_id": session,
                "data": expected_terminal_input(&transcript.nonce),
            })
}

fn is_exact_untagged_xterm_callback_payload(
    payload: &Value,
    transcript: &PackagedHostTranscript,
) -> bool {
    acceptance_nonce(payload).is_none()
        && (request_is_exact_agent_write(payload, transcript)
            || (payload
                .as_object()
                .is_some_and(|object| object.len() == 1 && object.contains_key("request"))
                && request_is_exact_resize(payload, transcript.terminal_session.as_deref())))
}

fn is_exact_product_xterm_callback_payload(
    command: &str,
    payload: &Value,
    transcript: &PackagedHostTranscript,
) -> bool {
    if acceptance_nonce(payload).is_some() {
        return false;
    }
    match command {
        "agent_write" => request_is_exact_agent_write(payload, transcript),
        "agent_resize" => {
            payload
                .as_object()
                .is_some_and(|object| object.len() == 1 && object.contains_key("request"))
                && request_is_exact_resize(payload, transcript.terminal_session.as_deref())
        }
        _ => false,
    }
}

fn expected_terminal_input(nonce: &str) -> Vec<u8> {
    let split = nonce.len().min(16);
    let (nonce_left, nonce_right) = nonce.split_at(split);
    format!("printf 'IMPULSE_PACKAGED_PTY_RESULT_%s%s\\n' '{nonce_left}' '{nonce_right}'\n")
        .into_bytes()
}

fn result_has_workspace(result: &Value, workspace_root: &str) -> bool {
    result.as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry.pointer("/target/root").and_then(Value::as_str) == Some(workspace_root)
        })
    })
}

/// Record a JS-to-Rust command result at the host dispatch boundary. Tagged
/// observer calls require the acceptance nonce. The only untagged calls
/// admitted are the exact active-session callbacks emitted by the product's
/// xterm `onData` and `onResize` handlers.
pub(crate) fn record_host_invoke(
    command: &str,
    payload: &Value,
    ok: bool,
    result: &Value,
    error: Option<&str>,
) {
    let mut guard = transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(transcript) = guard.as_mut() else {
        return;
    };
    let acceptance_tagged = acceptance_nonce(payload) == Some(transcript.nonce.as_str());
    let exact_product_callback =
        is_exact_product_xterm_callback_payload(command, payload, transcript);
    if !acceptance_tagged && !exact_product_callback {
        return;
    }

    match command {
        "agent_snapshot" | "agent_platforms" | "list_workspaces" | "mcp_descriptors"
        | "review_queue" => {
            if ok && result.is_array() {
                transcript.read_only_arrays.insert(command.to_string());
            }
            if command == "list_workspaces"
                && ok
                && result_has_workspace(result, &transcript.workspace_root)
            {
                transcript.workspace_listed = true;
            }
        }
        "__impulse_packaged_acceptance_unknown__" => {
            transcript.unknown_command_rejected =
                !ok && error.is_some_and(|message| message.contains("unknown host command"));
        }
        "register_workspace" => {
            transcript.workspace_registered = ok
                && result.pointer("/target/root").and_then(Value::as_str)
                    == Some(transcript.workspace_root.as_str());
        }
        "terminal_open" => {
            let session = result.get("session_id").and_then(Value::as_str);
            if ok && result.get("alive").and_then(Value::as_bool) == Some(true) {
                transcript.terminal_session = session.map(str::to_string);
                transcript.terminal_spawned = session.is_some_and(|value| !value.is_empty());
                transcript.terminal_opened = session.is_some_and(|value| !value.is_empty())
                    && request_is_exact_terminal_open(payload, transcript);
            }
        }
        "agent_write" => {
            transcript.terminal_input = ok && exact_product_callback;
        }
        "agent_resize" => {
            transcript.terminal_resized = ok && exact_product_callback;
        }
        "terminal_focus" => {
            transcript.terminal_focused =
                ok && request_is_exact_session(payload, transcript.terminal_session.as_deref());
        }
        "terminal_close" => {
            transcript.terminal_close_succeeded =
                ok && request_matches_session(payload, transcript.terminal_session.as_deref());
            transcript.terminal_closed =
                ok && request_is_exact_session(payload, transcript.terminal_session.as_deref());
        }
        _ => {}
    }
}

/// Record runtime events before they cross the Rust-to-JS boundary. Browser
/// observation remains necessary for DOM/assets, but cannot invent these host
/// events or successful dispatches.
pub(crate) fn record_host_event(event: &DesktopEvent) {
    let mut latest = LATEST_DAEMON_CONNECTION
        .get_or_init(|| Mutex::new(DaemonAcceptanceState::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    latest.apply(event);
    if !host_transcript_is_active() {
        return;
    }
    let mut guard = transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(transcript) = guard.as_mut() else {
        return;
    };
    match event {
        DesktopEvent::AgentRuntimeUpdate { snapshot } => {
            if transcript.runtime_sessions.len() >= MAX_TRANSCRIPT_SESSIONS {
                transcript.overflowed = true;
            } else {
                transcript
                    .runtime_sessions
                    .insert(snapshot.agent_id.clone());
            }
        }
        DesktopEvent::TerminalOutput { agent_id, data } => {
            if transcript.terminal_outputs.len() >= MAX_TRANSCRIPT_SESSIONS
                && !transcript.terminal_outputs.contains_key(agent_id)
            {
                transcript.overflowed = true;
                return;
            }
            let output = transcript
                .terminal_outputs
                .entry(agent_id.clone())
                .or_default();
            if output.len().saturating_add(data.len()) > MAX_TRANSCRIPT_OUTPUT_BYTES {
                transcript.overflowed = true;
            } else {
                output.extend_from_slice(data);
            }
        }
        DesktopEvent::TerminalExit { agent_id } => {
            if transcript.terminal_exits.len() >= MAX_TRANSCRIPT_SESSIONS {
                transcript.overflowed = true;
            } else {
                transcript.terminal_exits.insert(agent_id.clone());
            }
        }
        DesktopEvent::DaemonIdentityVerified { identity } => {
            transcript.pending_daemon_identity = Some(identity.clone());
        }
        DesktopEvent::OpsConnectionUpdate { connected, error } => {
            transcript.daemon_connection_seen = true;
            transcript.daemon_connection_error = error.clone();
            if *connected && error.is_none() {
                transcript.daemon_identity_verified = transcript.pending_daemon_identity.take();
                transcript.daemon_connected_current = transcript.daemon_identity_verified.is_some();
            } else {
                transcript.daemon_connected_current = false;
                transcript.pending_daemon_identity = None;
                transcript.daemon_identity_verified = None;
            }
        }
        _ => {}
    }
}

fn validate_host_transcript_snapshot(
    transcript: &PackagedHostTranscript,
    config: &PackagedAcceptanceConfig,
) -> Vec<String> {
    let mut failures = Vec::new();
    let mut require = |label: &str, value: bool| {
        if !value {
            failures.push(format!("Rust host transcript did not prove {label}"));
        }
    };
    require("nonce binding", transcript.nonce == config.nonce);
    require(
        "workspace binding",
        transcript.workspace_root == config.workspace_root.to_string_lossy(),
    );
    for command in [
        "agent_snapshot",
        "agent_platforms",
        "list_workspaces",
        "mcp_descriptors",
        "review_queue",
    ] {
        require(
            &format!("{command} array result"),
            transcript.read_only_arrays.contains(command),
        );
    }
    require(
        "typed unknown-command rejection",
        transcript.unknown_command_rejected,
    );
    require("workspace registration", transcript.workspace_registered);
    require("workspace listing", transcript.workspace_listed);
    require("terminal open", transcript.terminal_opened);
    require(
        "xterm onData agent_write callback",
        transcript.terminal_input,
    );
    require(
        "xterm onResize agent_resize callback",
        transcript.terminal_resized,
    );
    require("terminal focus", transcript.terminal_focused);
    require("terminal close", transcript.terminal_closed);
    require(
        "daemon connection event",
        transcript.daemon_connection_seen
            && transcript.daemon_connected_current
            && transcript.daemon_connection_error.is_none(),
    );
    require(
        "exact launched daemon identity",
        transcript.daemon_identity_verified.as_ref() == Some(&config.expected_daemon_identity),
    );
    require("bounded transcript", !transcript.overflowed);

    let session = transcript.terminal_session.as_deref();
    require(
        "terminal runtime update",
        session.is_some_and(|value| transcript.runtime_sessions.contains(value)),
    );
    require(
        "terminal output marker",
        session
            .and_then(|value| transcript.terminal_outputs.get(value))
            .is_some_and(|output| {
                String::from_utf8_lossy(output)
                    .lines()
                    .any(|line| line.trim_end_matches('\r') == transcript.marker)
            }),
    );
    require(
        "terminal exit event",
        session.is_some_and(|value| transcript.terminal_exits.contains(value)),
    );
    bound_failures(failures)
}

fn validate_host_transcript(config: &PackagedAcceptanceConfig) -> Vec<String> {
    let guard = transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_ref() {
        Some(transcript) => validate_host_transcript_snapshot(transcript, config),
        None => vec!["Rust host transcript was not initialized".to_string()],
    }
}

fn transcript_terminal_requiring_cleanup() -> Option<String> {
    let guard = transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .as_ref()
        .filter(|transcript| transcript.terminal_spawned && !transcript.terminal_close_succeeded)
        .and_then(|transcript| transcript.terminal_session.clone())
}

pub(crate) fn record_terminal_cleanup_succeeded(session_id: &str) {
    let mut guard = transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(transcript) = guard.as_mut() {
        if transcript.terminal_session.as_deref() == Some(session_id) {
            transcript.terminal_close_succeeded = true;
        }
    }
}

async fn cancel_acceptance_invokes_and_wait() -> Vec<String> {
    PACKAGED_ACCEPTANCE_CANCELLED.store(true, Ordering::Release);
    let drained = tokio::time::timeout(INVOKE_DRAIN_TIMEOUT, async {
        while PACKAGED_ACCEPTANCE_INFLIGHT.load(Ordering::Acquire) != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if drained.is_ok() {
        Vec::new()
    } else {
        vec![format!(
            "packaged acceptance host dispatches did not drain after {INVOKE_DRAIN_TIMEOUT:?}"
        )]
    }
}

async fn cleanup_transcript_terminal_if_open_with<F, Fut>(
    timeout: Duration,
    close_terminal: F,
) -> Vec<String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let Some(session_id) = transcript_terminal_requiring_cleanup() else {
        return Vec::new();
    };
    let cleanup_session = session_id.clone();
    match tokio::time::timeout(timeout, close_terminal(session_id)).await {
        Ok(Ok(())) => {
            record_terminal_cleanup_succeeded(&cleanup_session);
            Vec::new()
        }
        Ok(Err(error)) => vec![format!("Rust fallback terminal cleanup failed: {error}")],
        Err(_) => vec![format!(
            "Rust fallback terminal cleanup timed out after {timeout:?}"
        )],
    }
}

async fn cleanup_transcript_terminal_if_open() -> Vec<String> {
    cleanup_transcript_terminal_if_open_with(TERMINAL_CLEANUP_TIMEOUT, |session_id| {
        crate::host_bridge::close_packaged_acceptance_terminal(session_id)
    })
    .await
}

async fn contain_failed_acceptance() -> Vec<String> {
    let mut failures = cancel_acceptance_invokes_and_wait().await;
    failures.extend(cleanup_transcript_terminal_if_open().await);
    failures
}

fn env_value(key: &str) -> Option<String> {
    env::var_os(key).map(|value| value.to_string_lossy().into_owned())
}

/// Derive the exact daemon identity required by packaged acceptance.
///
/// Normal desktop launches have neither acceptance variable and return
/// `Ok(None)`. A partially configured acceptance launch fails closed before
/// the desktop daemon bridge can report a healthy connection.
pub fn expected_daemon_identity_for_packaged_acceptance(
    impulse_root: &Path,
) -> Result<Option<DaemonInstanceIdentity>, String> {
    let nonce = env_value(PACKAGED_ACCEPTANCE_NONCE_ENV);
    let daemon_pid = env_value(PACKAGED_DAEMON_PID_ENV);
    if nonce.is_none() && daemon_pid.is_none() {
        return Ok(None);
    }
    let nonce = nonce
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "{PACKAGED_ACCEPTANCE_NONCE_ENV} is required when {PACKAGED_DAEMON_PID_ENV} is set"
            )
        })?;
    if !is_lower_hex(&nonce, 32, 128) {
        return Err(format!(
            "{PACKAGED_ACCEPTANCE_NONCE_ENV} must be 32-128 lowercase hexadecimal characters"
        ));
    }
    let daemon_pid = daemon_pid
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            format!("{PACKAGED_DAEMON_PID_ENV} is required when packaged acceptance is enabled")
        })?
        .parse::<u32>()
        .map_err(|_| format!("{PACKAGED_DAEMON_PID_ENV} must be a non-zero process id"))?;
    if daemon_pid == 0 {
        return Err(format!(
            "{PACKAGED_DAEMON_PID_ENV} must be a non-zero process id"
        ));
    }
    let impulse_root = impulse_root
        .canonicalize()
        .map_err(|error| format!("packaged daemon Impulse root cannot be resolved: {error}"))?;
    if !impulse_root.is_absolute() {
        return Err("packaged daemon Impulse root must be absolute".to_string());
    }
    Ok(Some(DaemonInstanceIdentity {
        protocol_version: DAEMON_PROTOCOL_VERSION,
        pid: daemon_pid,
        impulse_root: impulse_root.to_string_lossy().into_owned(),
        instance_nonce_sha256: Some(impulse_ops::daemon_instance_nonce_sha256(&nonce)),
    }))
}

fn load_activation() -> AcceptanceActivation {
    let nonce = env_value(PACKAGED_ACCEPTANCE_NONCE_ENV);
    let root = env_value(PACKAGED_ACCEPTANCE_ROOT_ENV);
    let provenance_sha256 = env_value(PACKAGED_PROVENANCE_SHA256_ENV);
    let daemon_pid = env_value(PACKAGED_DAEMON_PID_ENV);
    if nonce.is_none() && root.is_none() && provenance_sha256.is_none() && daemon_pid.is_none() {
        return AcceptanceActivation::Disabled;
    }

    let displayed_nonce = nonce.clone().unwrap_or_default();
    let displayed_provenance = provenance_sha256.clone().unwrap_or_default();
    let mut failures = Vec::new();
    let nonce = required_value(nonce, PACKAGED_ACCEPTANCE_NONCE_ENV, &mut failures);
    let root = required_value(root, PACKAGED_ACCEPTANCE_ROOT_ENV, &mut failures);
    let provenance_sha256 = required_value(
        provenance_sha256,
        PACKAGED_PROVENANCE_SHA256_ENV,
        &mut failures,
    );

    if !is_lower_hex(&nonce, 32, 128) {
        failures.push(format!(
            "{PACKAGED_ACCEPTANCE_NONCE_ENV} must be 32-128 lowercase hexadecimal characters"
        ));
    }
    if !is_lower_hex(&provenance_sha256, 64, 64) {
        failures.push(format!(
            "{PACKAGED_PROVENANCE_SHA256_ENV} must be a 64-character lowercase SHA-256 digest"
        ));
    }

    let root = PathBuf::from(root);
    validate_absolute_path("acceptance root", &root, &mut failures);
    let canonical_root = canonicalize_required("acceptance root", &root, &mut failures);

    let home = required_path_env("HOME", &mut failures);
    let cfixed_home = required_path_env("CFFIXED_USER_HOME", &mut failures);
    let impulse_home = required_path_env("IMPULSE_HOME", &mut failures);
    let tmpdir = required_path_env("TMPDIR", &mut failures);
    let socket_path = required_path_env("IMPULSE_SOCKET_PATH", &mut failures);
    let expected_daemon_identity =
        match expected_daemon_identity_for_packaged_acceptance(&impulse_home) {
            Ok(Some(identity)) => Some(identity),
            Ok(None) => {
                failures.push(format!(
                    "{PACKAGED_DAEMON_PID_ENV} is required when packaged acceptance is enabled"
                ));
                None
            }
            Err(error) => {
                failures.push(error);
                None
            }
        };
    let current_dir = env::current_dir().unwrap_or_else(|error| {
        failures.push(format!("current directory is unavailable: {error}"));
        PathBuf::new()
    });
    let workspace_root = root.join("workspace");

    if let Some(canonical_root) = canonical_root.as_deref() {
        for (label, path, must_exist) in [
            ("HOME", home.as_path(), true),
            ("CFFIXED_USER_HOME", cfixed_home.as_path(), true),
            ("IMPULSE_HOME", impulse_home.as_path(), true),
            ("TMPDIR", tmpdir.as_path(), true),
            ("current directory", current_dir.as_path(), true),
            ("acceptance workspace", workspace_root.as_path(), true),
            ("IMPULSE_SOCKET_PATH", socket_path.as_path(), false),
        ] {
            validate_contained_path(label, path, canonical_root, must_exist, &mut failures);
        }
    }

    validate_packaged_executable(&provenance_sha256, &mut failures);

    if !failures.is_empty() {
        return AcceptanceActivation::Invalid {
            nonce: displayed_nonce,
            provenance_sha256: displayed_provenance,
            failures: bound_failures(failures),
        };
    }

    AcceptanceActivation::Ready(PackagedAcceptanceConfig {
        nonce,
        root,
        workspace_root,
        home,
        impulse_home,
        tmpdir,
        socket_path,
        provenance_sha256,
        expected_daemon_identity: expected_daemon_identity
            .expect("validated packaged daemon identity"),
    })
}

fn required_value(value: Option<String>, key: &str, failures: &mut Vec<String>) -> String {
    match value.filter(|value| !value.trim().is_empty()) {
        Some(value) => value,
        None => {
            failures.push(format!(
                "{key} is required when packaged acceptance is enabled"
            ));
            String::new()
        }
    }
}

fn required_path_env(key: &str, failures: &mut Vec<String>) -> PathBuf {
    PathBuf::from(required_value(env_value(key), key, failures))
}

fn is_lower_hex(value: &str, min_len: usize, max_len: usize) -> bool {
    (min_len..=max_len).contains(&value.len())
        && value.len() % 2 == 0
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_absolute_path(label: &str, path: &Path, failures: &mut Vec<String>) {
    if !path.is_absolute() {
        failures.push(format!("{label} must be absolute"));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        failures.push(format!("{label} may not contain parent traversal"));
    }
}

fn canonicalize_required(label: &str, path: &Path, failures: &mut Vec<String>) -> Option<PathBuf> {
    match path.canonicalize() {
        Ok(path) => Some(path),
        Err(error) => {
            failures.push(format!("{label} cannot be resolved: {error}"));
            None
        }
    }
}

fn validate_contained_path(
    label: &str,
    path: &Path,
    canonical_root: &Path,
    must_exist: bool,
    failures: &mut Vec<String>,
) {
    validate_absolute_path(label, path, failures);
    let candidate = if must_exist {
        canonicalize_required(label, path, failures)
    } else {
        let Some(parent) = path.parent() else {
            failures.push(format!("{label} has no parent directory"));
            return;
        };
        canonicalize_required(&format!("{label} parent"), parent, failures)
            .map(|parent| parent.join(path.file_name().unwrap_or_default()))
    };
    if let Some(candidate) = candidate {
        if !candidate.starts_with(canonical_root) {
            failures.push(format!("{label} escapes the acceptance root"));
        }
    }
}

fn validate_packaged_executable(provenance_sha256: &str, failures: &mut Vec<String>) {
    let executable = match env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            failures.push(format!("current executable is unavailable: {error}"));
            return;
        }
    };
    let Some(macos_dir) = executable.parent() else {
        failures.push("packaged executable has no MacOS parent".to_string());
        return;
    };
    let Some(contents_dir) = macos_dir.parent() else {
        failures.push("packaged executable has no Contents parent".to_string());
        return;
    };
    let Some(app_dir) = contents_dir.parent() else {
        failures.push("packaged executable has no app-bundle parent".to_string());
        return;
    };
    if executable.file_name().and_then(|name| name.to_str()) != Some("impulse-desktop")
        || macos_dir.file_name().and_then(|name| name.to_str()) != Some("MacOS")
        || contents_dir.file_name().and_then(|name| name.to_str()) != Some("Contents")
        || app_dir.extension().and_then(|extension| extension.to_str()) != Some("app")
    {
        failures.push(
            "current executable is not Contents/MacOS/impulse-desktop in an app bundle".to_string(),
        );
        return;
    }

    let manifest = contents_dir.join(PROVENANCE_RELATIVE_PATH);
    match sha256_file(&manifest) {
        Ok(actual) if actual == provenance_sha256 => {}
        Ok(actual) => failures.push(format!(
            "embedded provenance digest mismatch: expected {provenance_sha256}, observed {actual}"
        )),
        Err(error) => failures.push(format!(
            "embedded provenance manifest cannot be hashed: {error}"
        )),
    }
}

fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn observer_script(config: &PackagedAcceptanceConfig) -> String {
    let values = serde_json::json!({
        "nonce": config.nonce,
        "root": config.root,
        "workspaceRoot": config.workspace_root,
        "home": config.home,
        "impulseHome": config.impulse_home,
        "tmpdir": config.tmpdir,
        "socketPath": config.socket_path,
    });
    PACKAGED_ACCEPTANCE_SCRIPT.replace(
        "__IMPULSE_PACKAGED_ACCEPTANCE_CONFIG__",
        &values.to_string(),
    )
}

fn validate_observation(observation: &PackagedHostObservation) -> Vec<String> {
    let mut failures = observation.errors.clone();
    require_exact(&mut failures, "host_kind", &observation.host_kind, "dioxus");
    require_exact(
        &mut failures,
        "host_status",
        &observation.host_status,
        crate::host_bridge::LIVE_HOST_BRIDGE_STATUS,
    );
    for (label, value) in [
        ("xterm_loaded", observation.xterm_loaded),
        ("fit_addon_loaded", observation.fit_addon_loaded),
        ("stylesheet_loaded", observation.stylesheet_loaded),
        ("assets_local", observation.assets_local),
        ("asset_paths_exact", observation.asset_paths_exact),
        (
            "terminal_interop_mounted",
            observation.terminal_interop_mounted,
        ),
        ("xterm_session_mounted", observation.xterm_session_mounted),
        ("ops_bridge_mounted", observation.ops_bridge_mounted),
        ("tauri_absent", observation.tauri_absent),
        (
            "injected_test_api_absent",
            observation.injected_test_api_absent,
        ),
        ("agent_snapshot_array", observation.agent_snapshot_array),
        ("agent_platforms_array", observation.agent_platforms_array),
        ("workspaces_array", observation.workspaces_array),
        ("mcp_descriptors_array", observation.mcp_descriptors_array),
        ("review_queue_array", observation.review_queue_array),
        (
            "unknown_command_rejected",
            observation.unknown_command_rejected,
        ),
        ("workspace_registered", observation.workspace_registered),
        ("workspace_listed", observation.workspace_listed),
        ("daemon_connected", observation.daemon_connected),
        ("terminal_opened", observation.terminal_opened),
        ("terminal_input", observation.terminal_input),
        ("terminal_output", observation.terminal_output),
        ("terminal_resized", observation.terminal_resized),
        (
            "xterm_on_data_api_called",
            observation.xterm_on_data_api_called,
        ),
        (
            "xterm_on_resize_api_called",
            observation.xterm_on_resize_api_called,
        ),
        (
            "xterm_output_buffer_rendered",
            observation.xterm_output_buffer_rendered,
        ),
        ("terminal_focused", observation.terminal_focused),
        ("terminal_closed", observation.terminal_closed),
        ("terminal_exited", observation.terminal_exited),
    ] {
        if !value {
            failures.push(format!("{label} was not proven"));
        }
    }
    if observation.terminal_interop_degraded {
        failures.push("terminal_interop_degraded must be false".to_string());
    }
    if !observation
        .unknown_command_error
        .contains("unknown host command")
    {
        failures.push("unknown command did not return the typed host error".to_string());
    }
    bound_failures(failures)
}

fn require_exact(failures: &mut Vec<String>, label: &str, actual: &str, expected: &str) {
    if actual != expected {
        failures.push(format!(
            "{label} mismatch: expected {expected:?}, observed {actual:?}"
        ));
    }
}

fn bound_failures(failures: Vec<String>) -> Vec<String> {
    failures
        .into_iter()
        .take(MAX_FAILURE_REASONS)
        .map(|failure| failure.chars().take(MAX_REASON_CHARS).collect())
        .collect()
}

fn current_daemon_identity() -> Option<DaemonInstanceIdentity> {
    transcript_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .and_then(|transcript| transcript.daemon_identity_verified.clone())
}

fn emit_receipt(
    nonce: &str,
    provenance_sha256: &str,
    observation: Option<&PackagedHostObservation>,
    rust_host_transcript_validated: bool,
    failures: &[String],
) {
    let daemon_identity = current_daemon_identity();
    let receipt = PackagedHostReceipt {
        schema: RECEIPT_SCHEMA,
        nonce,
        pid: std::process::id(),
        crate_version: env!("CARGO_PKG_VERSION"),
        provenance_sha256,
        daemon_identity: daemon_identity.as_ref(),
        rust_host_transcript_validated,
        outcome: if failures.is_empty() {
            "passed"
        } else {
            "failed"
        },
        observation,
        failure_reasons: failures,
    };
    let encoded = serde_json::to_string(&receipt).unwrap_or_else(|error| {
        format!(
            r#"{{"schema":"{RECEIPT_SCHEMA}","outcome":"failed","failure_reasons":["receipt serialization failed: {error}"]}}"#
        )
    });
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "{PACKAGED_HOST_RECEIPT_PREFIX}{encoded}");
    let _ = stderr.flush();
    PACKAGED_ACCEPTANCE_CANCELLED.store(true, Ordering::Release);
    PACKAGED_HOST_TRANSCRIPT_ACTIVE.store(false, Ordering::Release);
}

/// Mount the passive packaged-host observer. Normal launches do no work
/// because all acceptance environment variables are absent.
pub fn use_packaged_acceptance() {
    use_future(move || async move {
        let config = match load_activation() {
            AcceptanceActivation::Disabled => return,
            AcceptanceActivation::Invalid {
                nonce,
                provenance_sha256,
                failures,
            } => {
                emit_receipt(&nonce, &provenance_sha256, None, false, &failures);
                return;
            }
            AcceptanceActivation::Ready(config) => config,
        };

        begin_host_transcript(&config);
        let script = observer_script(&config);
        let mut eval = document::eval(&script);
        let received =
            tokio::time::timeout(OBSERVATION_TIMEOUT, eval.recv::<PackagedHostObservation>()).await;
        match received {
            Ok(Ok(observation)) => {
                let mut failures = validate_observation(&observation);
                let transcript_failures = validate_host_transcript(&config);
                let rust_host_transcript_validated = transcript_failures.is_empty();
                failures.extend(transcript_failures);
                if !failures.is_empty() {
                    failures.extend(contain_failed_acceptance().await);
                }
                let failures = bound_failures(failures);
                emit_receipt(
                    &config.nonce,
                    &config.provenance_sha256,
                    Some(&observation),
                    rust_host_transcript_validated,
                    &failures,
                );
            }
            Ok(Err(error)) => {
                let mut failures = vec![format!("packaged observer channel failed: {error}")];
                failures.extend(contain_failed_acceptance().await);
                let failures = bound_failures(failures);
                emit_receipt(
                    &config.nonce,
                    &config.provenance_sha256,
                    None,
                    false,
                    &failures,
                );
            }
            Err(_) => {
                let mut failures = vec!["packaged observer timed out".to_string()];
                failures.extend(contain_failed_acceptance().await);
                let failures = bound_failures(failures);
                emit_receipt(
                    &config.nonce,
                    &config.provenance_sha256,
                    None,
                    false,
                    &failures,
                );
            }
        }
    });
}

const PACKAGED_ACCEPTANCE_SCRIPT: &str = r#"
(async () => {
  const config = __IMPULSE_PACKAGED_ACCEPTANCE_CONFIG__;
  const observation = {
    host_kind: "",
    host_status: "",
    xterm_loaded: false,
    fit_addon_loaded: false,
    stylesheet_loaded: false,
    assets_local: false,
    asset_paths_exact: false,
    terminal_interop_mounted: false,
    terminal_interop_degraded: true,
    xterm_session_mounted: false,
    ops_bridge_mounted: false,
    tauri_absent: false,
    injected_test_api_absent: false,
    agent_snapshot_array: false,
    agent_platforms_array: false,
    workspaces_array: false,
    mcp_descriptors_array: false,
    review_queue_array: false,
    unknown_command_rejected: false,
    unknown_command_error: "",
    workspace_registered: false,
    workspace_listed: false,
    daemon_connected: false,
    terminal_opened: false,
    terminal_input: false,
    terminal_output: false,
    terminal_resized: false,
    xterm_on_data_api_called: false,
    xterm_on_resize_api_called: false,
    xterm_output_buffer_rendered: false,
    terminal_focused: false,
    terminal_closed: false,
    terminal_exited: false,
    errors: [],
  };
  const errors = [];
  const unlisten = [];
  const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
  const errorText = (error) => String(error?.message || error || "unknown error").slice(0, 240);
  const waitUntil = async (predicate, attempts = 120, delay = 100) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (predicate()) return true;
      await sleep(delay);
    }
    return false;
  };
  const payloadOf = (event) => event?.payload ?? event ?? {};
  const bufferHasExactLine = (terminal, expected) => {
    if (!terminal || !terminal.buffer) return false;
    const active = terminal.buffer.active;
    if (!active || typeof active.getLine !== "function" || !Number.isInteger(active.length)) {
      return false;
    }
    for (let index = 0; index < active.length; index += 1) {
      const line = active.getLine(index);
      if (line && line.translateToString(true) === expected) return true;
    }
    return false;
  };

  try {
    let host;
    const hostReady = await waitUntil(() => {
      host = window.__IMPULSE_DESKTOP_HOST;
      return typeof host?.invoke === "function"
        && typeof host?.listen === "function"
        && host?.hostKind === "dioxus"
        && host?.status === "dioxus-eval-bridge-ready";
    });
    if (!hostReady) throw new Error("real Dioxus host did not become ready");

    observation.host_kind = String(host.hostKind || "");
    observation.host_status = String(host.status || "");
    observation.tauri_absent = typeof window["__TAURI__"] === "undefined";
    observation.injected_test_api_absent = typeof window["__IMPULSE_TEST_HOST_API"] === "undefined";

    const expectedAssets = [
      ["xterm-css", "assets/vendor/xterm/xterm.css", "href"],
      ["xterm-js", "assets/vendor/xterm/xterm.js", "src"],
      ["xterm-fit-addon", "assets/vendor/xterm/addon-fit.js", "src"],
    ];
    const assetsReady = await waitUntil(() => {
      const TerminalCtor = window.Terminal || window.XTerm?.Terminal;
      const FitAddonCtor = window.FitAddon?.FitAddon || window.FitAddon;
      const stylesheetLoaded = Array.from(document.styleSheets || []).some((sheet) =>
        String(sheet.href || "").endsWith("/assets/vendor/xterm/xterm.css")
      );
      return Boolean(TerminalCtor && FitAddonCtor && stylesheetLoaded);
    });
    const TerminalCtor = window.Terminal || window.XTerm?.Terminal;
    const assetUrls = expectedAssets.map(([tag, path, attribute]) => {
      const element = document.querySelector(`[data-impulse-terminal-asset='${tag}']`);
      const raw = String(element?.getAttribute(attribute) || "");
      const resolved = String(element?.[attribute] || raw);
      return { path, raw, resolved };
    });
    observation.xterm_loaded = Boolean(window.Terminal || window.XTerm?.Terminal);
    observation.fit_addon_loaded = Boolean(window.FitAddon?.FitAddon || window.FitAddon);
    observation.stylesheet_loaded = Array.from(document.styleSheets || []).some((sheet) =>
      String(sheet.href || "").endsWith("/assets/vendor/xterm/xterm.css")
    );
    observation.asset_paths_exact = assetUrls.every(({ path, raw, resolved }) =>
      raw === path || raw.endsWith(`/${path}`) || resolved.endsWith(`/${path}`)
    );
    observation.assets_local = assetUrls.every(({ resolved }) => {
      try {
        return new URL(resolved, document.baseURI).protocol === "dioxus:";
      } catch (_) {
        return false;
      }
    });
    if (!assetsReady) errors.push("packaged xterm assets did not become ready");

    await waitUntil(() => window.__impulseOpsBridge?.mounted && window.__impulseTerminalInterop?.mounted);
    observation.ops_bridge_mounted = window.__impulseOpsBridge?.mounted === true
      && window.__impulseOpsBridge?.degraded !== true;
    observation.terminal_interop_mounted = window.__impulseTerminalInterop?.mounted === true;
    observation.terminal_interop_degraded = window.__impulseTerminalInterop?.degraded === true;

    let daemonConnected = window.__impulseOpsBridge?.daemonConnected === true;
    let activeSession = "";
    let activeTerminal = null;
    unlisten.push(await host.listen("ops_connection_update", (event) => {
      const payload = payloadOf(event);
      daemonConnected = payload?.connected === true;
    }));

    const readOnly = [
      ["agent_snapshot", "agent_snapshot_array"],
      ["agent_platforms", "agent_platforms_array"],
      ["list_workspaces", "workspaces_array"],
      ["mcp_descriptors", "mcp_descriptors_array"],
      ["review_queue", "review_queue_array"],
    ];
    for (const [command, field] of readOnly) {
      try {
        observation[field] = Array.isArray(await host.invoke(command, {
          acceptance_nonce: config.nonce,
        }));
      } catch (error) {
        errors.push(`${command}: ${errorText(error)}`);
      }
    }

    try {
      await host.invoke("__impulse_packaged_acceptance_unknown__", {
        acceptance_nonce: config.nonce,
      });
      errors.push("unknown host command unexpectedly succeeded");
    } catch (error) {
      observation.unknown_command_error = errorText(error);
      observation.unknown_command_rejected = observation.unknown_command_error.includes("unknown host command");
    }

    try {
      const entry = await host.invoke("register_workspace", {
        acceptance_nonce: config.nonce,
        request: {
          root: config.workspaceRoot,
          label: "packaged-acceptance",
          purpose: "isolated packaged host acceptance",
          project_notes: null,
        },
      });
      observation.workspace_registered = entry?.target?.root === config.workspaceRoot;
      const listed = await host.invoke("list_workspaces", {
        acceptance_nonce: config.nonce,
      });
      observation.workspace_listed = Array.isArray(listed)
        && listed.some((item) => item?.target?.root === config.workspaceRoot);
    } catch (error) {
      errors.push(`register_workspace: ${errorText(error)}`);
    }

    const daemonBecameConnected = daemonConnected
      || await waitUntil(() => daemonConnected || window.__impulseOpsBridge?.daemonConnected === true, 150, 100);

    const marker = `IMPULSE_PACKAGED_PTY_RESULT_${config.nonce}`;
    try {
      const session = await host.invoke("terminal_open", {
        acceptance_nonce: config.nonce,
        request: {
          session_id: null,
          command: "/bin/sh",
          args: [],
          cwd: config.workspaceRoot,
          env: {
            HOME: config.home,
            IMPULSE_HOME: config.impulseHome,
            IMPULSE_SOCKET_PATH: config.socketPath,
            TMPDIR: config.tmpdir,
            PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
          },
          workspace: null,
          mcp_tools: [],
          rows: 24,
          cols: 80,
        },
      });
      activeSession = String(session?.session_id || "");
      observation.terminal_opened = Boolean(activeSession && session?.alive === true);
      try {
        observation.xterm_session_mounted = await waitUntil(() => {
          const interop = window.__impulseTerminalInterop;
          const mount = Array.from(document.querySelectorAll("[data-xterm-mount='true']"))
            .find((candidate) => candidate?.dataset?.agentId === activeSession);
          return Boolean(
            interop?.degraded !== true
            && interop?.terminals?.[activeSession]
            && mount?.getAttribute("data-xterm-state") === "mounted"
          );
        });
        activeTerminal = window.__impulseTerminalInterop?.terminals?.[activeSession] || null;
        if (!TerminalCtor || !(activeTerminal instanceof TerminalCtor)
          || typeof activeTerminal.input !== "function"
          || typeof activeTerminal.resize !== "function"
          || !activeTerminal.buffer || !activeTerminal.buffer.active) {
          throw new Error("mounted xterm Terminal public API was unavailable");
        }
        activeTerminal.resize(97, 31);
        observation.xterm_on_resize_api_called = true;
        observation.terminal_resized = true;
        await host.invoke("terminal_focus", { acceptance_nonce: config.nonce, request: { session_id: activeSession } });
        observation.terminal_focused = true;
        const nonceLeft = config.nonce.slice(0, 16);
        const nonceRight = config.nonce.slice(16);
        const inputText =
          `printf 'IMPULSE_PACKAGED_PTY_RESULT_%s%s\\n' '${nonceLeft}' '${nonceRight}'\n`
        ;
        activeTerminal.input(inputText, true);
        observation.xterm_on_data_api_called = true;
        observation.terminal_input = true;
        observation.xterm_output_buffer_rendered = await waitUntil(() =>
          bufferHasExactLine(activeTerminal, marker),
          100,
          100
        );
        observation.terminal_output = observation.xterm_output_buffer_rendered;
      } finally {
        if (activeSession) {
          try {
            await host.invoke("terminal_close", { acceptance_nonce: config.nonce, request: { session_id: activeSession } });
            observation.terminal_closed = true;
          } catch (error) {
            errors.push(`terminal cleanup: ${errorText(error)}`);
          }
          observation.terminal_exited = await waitUntil(() =>
            bufferHasExactLine(activeTerminal, "[process exited]"),
            50,
            100
          );
        }
      }
    } catch (error) {
      errors.push(`terminal lifecycle: ${errorText(error)}`);
    }
    observation.daemon_connected = daemonBecameConnected
      && daemonConnected
      && window.__impulseOpsBridge?.daemonConnected === true;
  } catch (error) {
    errors.push(errorText(error));
  } finally {
    for (const stop of unlisten) {
      try { if (typeof stop === "function") await stop(); } catch (_) {}
    }
    observation.errors = errors.slice(0, 24);
    try {
      dioxus.send(observation);
    } catch (_) {}
  }
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn test_config() -> PackagedAcceptanceConfig {
        let nonce = "ab".repeat(16);
        let impulse_home = PathBuf::from("/private/tmp/acceptance/impulse-home");
        let expected_daemon_identity = DaemonInstanceIdentity {
            protocol_version: DAEMON_PROTOCOL_VERSION,
            pid: 4242,
            impulse_root: impulse_home.to_string_lossy().into_owned(),
            instance_nonce_sha256: Some(impulse_ops::daemon_instance_nonce_sha256(&nonce)),
        };
        PackagedAcceptanceConfig {
            nonce,
            root: PathBuf::from("/private/tmp/acceptance"),
            workspace_root: PathBuf::from("/private/tmp/acceptance/workspace"),
            home: PathBuf::from("/private/tmp/acceptance/home"),
            impulse_home,
            tmpdir: PathBuf::from("/private/tmp/acceptance/tmp"),
            socket_path: PathBuf::from("/private/tmp/acceptance/.impulse/sockets/impulse.sock"),
            provenance_sha256: "cd".repeat(32),
            expected_daemon_identity,
        }
    }

    fn passing_transcript(config: &PackagedAcceptanceConfig) -> PackagedHostTranscript {
        let session = "packaged-session".to_string();
        let marker = format!("IMPULSE_PACKAGED_PTY_RESULT_{}", config.nonce);
        PackagedHostTranscript {
            nonce: config.nonce.clone(),
            workspace_root: config.workspace_root.to_string_lossy().into_owned(),
            marker: marker.clone(),
            expected_terminal_open_request: expected_terminal_open_request(config),
            read_only_arrays: [
                "agent_snapshot",
                "agent_platforms",
                "list_workspaces",
                "mcp_descriptors",
                "review_queue",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            unknown_command_rejected: true,
            workspace_registered: true,
            workspace_listed: true,
            terminal_session: Some(session.clone()),
            terminal_spawned: true,
            terminal_opened: true,
            terminal_input: true,
            terminal_resized: true,
            terminal_focused: true,
            terminal_close_succeeded: true,
            terminal_closed: true,
            runtime_sessions: [session.clone()].into_iter().collect(),
            terminal_outputs: [(session.clone(), format!("{marker}\r\n").into_bytes())]
                .into_iter()
                .collect(),
            terminal_exits: [session].into_iter().collect(),
            daemon_connection_seen: true,
            daemon_connected_current: true,
            daemon_connection_error: None,
            pending_daemon_identity: None,
            daemon_identity_verified: Some(config.expected_daemon_identity.clone()),
            overflowed: false,
        }
    }

    fn passing_observation() -> PackagedHostObservation {
        PackagedHostObservation {
            host_kind: "dioxus".to_string(),
            host_status: crate::host_bridge::LIVE_HOST_BRIDGE_STATUS.to_string(),
            xterm_loaded: true,
            fit_addon_loaded: true,
            stylesheet_loaded: true,
            assets_local: true,
            asset_paths_exact: true,
            terminal_interop_mounted: true,
            terminal_interop_degraded: false,
            xterm_session_mounted: true,
            ops_bridge_mounted: true,
            tauri_absent: true,
            injected_test_api_absent: true,
            agent_snapshot_array: true,
            agent_platforms_array: true,
            workspaces_array: true,
            mcp_descriptors_array: true,
            review_queue_array: true,
            unknown_command_rejected: true,
            unknown_command_error: "unknown host command `sentinel`".to_string(),
            workspace_registered: true,
            workspace_listed: true,
            daemon_connected: true,
            terminal_opened: true,
            terminal_input: true,
            terminal_output: true,
            terminal_resized: true,
            xterm_on_data_api_called: true,
            xterm_on_resize_api_called: true,
            xterm_output_buffer_rendered: true,
            terminal_focused: true,
            terminal_closed: true,
            terminal_exited: true,
            errors: Vec::new(),
        }
    }

    #[test]
    fn passing_observation_is_accepted_only_by_rust_validator() {
        assert!(validate_observation(&passing_observation()).is_empty());
    }

    #[test]
    fn rust_host_transcript_is_required_in_addition_to_browser_observation() {
        let config = test_config();
        let mut transcript = passing_transcript(&config);
        assert!(validate_host_transcript_snapshot(&transcript, &config).is_empty());

        transcript.terminal_closed = false;
        let failures = validate_host_transcript_snapshot(&transcript, &config);
        assert!(failures
            .iter()
            .any(|failure| failure.contains("terminal close")));
    }

    #[test]
    fn exact_terminal_request_contract_fails_closed_on_isolation_or_size_drift() {
        let config = test_config();
        let transcript = passing_transcript(&config);
        let mut open_payload = serde_json::json!({
            "acceptance_nonce": config.nonce,
            "request": transcript.expected_terminal_open_request.clone(),
        });
        assert!(request_is_exact_terminal_open(&open_payload, &transcript));
        open_payload["request"]["cwd"] = Value::String("/unexpected".to_string());
        assert!(!request_is_exact_terminal_open(&open_payload, &transcript));

        let session = transcript.terminal_session.as_deref();
        let mut resize_payload = serde_json::json!({
            "request": { "session_id": session, "rows": 31, "cols": 97 },
        });
        assert!(request_is_exact_resize(&resize_payload, session));
        resize_payload["request"]["cols"] = Value::from(98);
        assert!(!request_is_exact_resize(&resize_payload, session));
    }

    #[test]
    fn generated_observer_emits_exact_shell_bytes_and_requires_an_exact_buffer_line() {
        if Command::new("node").arg("--version").output().is_err() {
            return;
        }
        let config = test_config();
        let script = observer_script(&config);

        let template_start = script
            .find("`printf 'IMPULSE_PACKAGED_PTY_RESULT_%s%s")
            .expect("generated observer terminal-input template");
        let template_end = template_start
            + 1
            + script[template_start + 1..]
                .find('`')
                .expect("terminal-input template closing backtick");
        let input_template = &script[template_start..=template_end];

        let nonce_split = config.nonce.len().min(16);
        let (nonce_left, nonce_right) = config.nonce.split_at(nonce_split);
        let marker = format!("IMPULSE_PACKAGED_PTY_RESULT_{}", config.nonce);
        let nonce_left_json = serde_json::to_string(nonce_left).expect("serialize left nonce");
        let nonce_right_json = serde_json::to_string(nonce_right).expect("serialize right nonce");
        let node_script = [
            "const nonceLeft = ",
            nonce_left_json.as_str(),
            ";\nconst nonceRight = ",
            nonce_right_json.as_str(),
            ";\nconst input = Array.from(new TextEncoder().encode(",
            input_template,
            "));\nconsole.log(JSON.stringify({ input }));\n",
        ]
        .concat();
        let output = Command::new("node")
            .args(["-e", node_script.as_str()])
            .output()
            .expect("evaluate generated terminal observer expressions");
        assert!(
            output.status.success(),
            "Node evaluation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value =
            serde_json::from_slice(&output.stdout).expect("parse generated-observer result");
        let observed_input = result["input"]
            .as_array()
            .expect("Node input byte array")
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|byte| u8::try_from(byte).ok())
                    .expect("valid input byte")
            })
            .collect::<Vec<_>>();
        assert_eq!(observed_input, expected_terminal_input(&config.nonce));
        assert_eq!(observed_input.last(), Some(&b'\n'));
        assert!(!String::from_utf8_lossy(&observed_input).contains(&marker));
        assert!(script.contains("line.translateToString(true) === expected"));
        assert!(!script.contains("line.trim() === marker"));
        assert!(script.contains("activeTerminal instanceof TerminalCtor"));
    }

    #[tokio::test]
    async fn failed_observer_cleanup_is_bounded_and_reaps_a_real_resistant_pty() {
        let config = test_config();
        let mut timed_out_transcript = passing_transcript(&config);
        timed_out_transcript.terminal_close_succeeded = false;
        timed_out_transcript.terminal_closed = false;
        {
            let mut guard = transcript_slot()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = Some(timed_out_transcript);
        }
        let timeout_failures =
            cleanup_transcript_terminal_if_open_with(Duration::from_millis(5), |_| async {
                std::future::pending::<Result<(), String>>().await
            })
            .await;
        assert!(timeout_failures
            .iter()
            .any(|failure| failure.contains("timed out after 5ms")));

        let (sink, mut events) = crate::host_bridge::channel_event_sink();
        let runtime = crate::runtime::DesktopRuntime::builder()
            .with_event_sink(sink)
            .build();
        let cwd = tempfile::tempdir().expect("temporary fallback-cleanup cwd");
        let opened = crate::host_commands::terminal_open(
            &runtime,
            crate::bridge::TerminalOpenRequest {
                session_id: Some("packaged-fallback-resistant-pty".to_string()),
                command: "/bin/sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "trap '' HUP TERM; while :; do read ignored || :; done".to_string(),
                ],
                cwd: Some(cwd.path().to_string_lossy().into_owned()),
                env: HashMap::new(),
                workspace: None,
                mcp_tools: Vec::new(),
                rows: 24,
                cols: 80,
            },
        )
        .await
        .expect("open resistant fallback-cleanup PTY");

        let mut open_transcript = passing_transcript(&config);
        open_transcript.terminal_session = Some(opened.session_id.clone());
        open_transcript.terminal_spawned = true;
        open_transcript.terminal_opened = false;
        open_transcript.terminal_close_succeeded = false;
        open_transcript.terminal_closed = false;
        open_transcript.terminal_exits.clear();
        {
            let mut guard = transcript_slot()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = Some(open_transcript);
        }

        let close_runtime = runtime.clone();
        let cleanup_failures = cleanup_transcript_terminal_if_open_with(
            Duration::from_secs(2),
            move |session_id| async move {
                crate::host_bridge::close_packaged_acceptance_terminal_on_runtime(
                    &close_runtime,
                    session_id,
                )
                .await
            },
        )
        .await;
        let session_still_present = runtime
            .snapshot_agents()
            .iter()
            .any(|snapshot| snapshot.agent_id == opened.session_id);
        if session_still_present {
            let _ = runtime.close_agent(crate::bridge::TerminalCloseRequest {
                session_id: opened.session_id.clone(),
            });
        }
        let mut exit_seen = false;
        while let Ok(event) = events.try_recv() {
            if matches!(
                event,
                DesktopEvent::TerminalExit { ref agent_id } if agent_id == &opened.session_id
            ) {
                exit_seen = true;
            }
        }
        assert!(cleanup_failures.is_empty(), "{cleanup_failures:?}");
        assert!(!session_still_present, "fallback left PTY runtime state");
        assert!(exit_seen, "fallback did not emit terminal-exit truth");

        {
            let mut guard = transcript_slot()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard
                .as_mut()
                .expect("test transcript")
                .terminal_close_succeeded = true;
            guard.as_mut().expect("test transcript").terminal_closed = true;
        }
        assert_eq!(transcript_terminal_requiring_cleanup(), None);
    }

    #[test]
    fn every_false_runtime_invariant_fails_closed() {
        let mut observation = passing_observation();
        observation.daemon_connected = false;
        observation.terminal_output = false;
        observation.tauri_absent = false;
        let failures = validate_observation(&observation);
        assert!(failures
            .iter()
            .any(|failure| failure.contains("daemon_connected")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("terminal_output")));
        assert!(failures
            .iter()
            .any(|failure| failure.contains("tauri_absent")));
    }

    #[test]
    fn nonce_and_digest_are_strict_lowercase_hex() {
        assert!(is_lower_hex(&"a1".repeat(16), 32, 128));
        assert!(is_lower_hex(&"af".repeat(32), 64, 64));
        assert!(!is_lower_hex(&"AF".repeat(32), 64, 64));
        assert!(!is_lower_hex("abc", 32, 128));
        assert!(!is_lower_hex(&"0".repeat(130), 32, 128));
    }

    #[test]
    fn observer_script_is_passive_and_uses_real_product_commands() {
        let config = test_config();
        let script = observer_script(&config);
        for command in [
            "agent_snapshot",
            "agent_platforms",
            "list_workspaces",
            "mcp_descriptors",
            "review_queue",
            "register_workspace",
            "terminal_open",
            "terminal_focus",
            "terminal_close",
        ] {
            assert!(script.contains(command), "observer omitted {command}");
        }
        for product_api in [
            ".input(inputText, true)",
            ".resize(97, 31)",
            ".buffer.active",
            "activeTerminal instanceof TerminalCtor",
        ] {
            assert!(
                script.contains(product_api),
                "observer omitted real xterm API {product_api}"
            );
        }
        for bypass in [
            "host.listen(\"terminal_output\"",
            "host.listen(\"terminal_exit\"",
            "host.invoke(\"terminal_write\"",
            "host.invoke(\"terminal_resize\"",
        ] {
            assert!(
                !script.contains(bypass),
                "observer retained bypass {bypass}"
            );
        }
        for forbidden in [
            ["window.__IMPULSE_DESKTOP_HOST", " ="].concat(),
            ["window.__IMPULSE_TEST_HOST_API", " ="].concat(),
            ["window.__TAURI__", " ="].concat(),
        ] {
            assert!(!script.contains(&forbidden));
        }
    }

    #[test]
    fn untagged_xterm_callbacks_are_exact_and_session_bound() {
        let config = test_config();
        let transcript = passing_transcript(&config);
        let session = transcript
            .terminal_session
            .as_deref()
            .expect("passing transcript session");
        let write = serde_json::json!({
            "request": {
                "agent_id": session,
                "data": expected_terminal_input(&config.nonce),
            }
        });
        let resize = serde_json::json!({
            "request": { "session_id": session, "cols": 97, "rows": 31 }
        });

        assert!(is_exact_product_xterm_callback_payload(
            "agent_write",
            &write,
            &transcript
        ));
        assert!(is_exact_product_xterm_callback_payload(
            "agent_resize",
            &resize,
            &transcript
        ));
        assert!(!is_exact_product_xterm_callback_payload(
            "terminal_write",
            &write,
            &transcript
        ));

        let mut wrong_session = write.clone();
        wrong_session["request"]["agent_id"] = Value::String("other-session".to_string());
        assert!(!is_exact_product_xterm_callback_payload(
            "agent_write",
            &wrong_session,
            &transcript
        ));

        let mut wrong_bytes = write;
        wrong_bytes["request"]["data"] = serde_json::json!([120]);
        assert!(!is_exact_product_xterm_callback_payload(
            "agent_write",
            &wrong_bytes,
            &transcript
        ));
    }

    #[test]
    fn daemon_connection_requires_the_preceding_verified_identity() {
        let config = test_config();
        let connected = DesktopEvent::OpsConnectionUpdate {
            connected: true,
            error: None,
        };
        let mut state = DaemonAcceptanceState::default();
        state.apply(&connected);
        assert!(!state.connected);
        assert!(state.connected_identity.is_none());

        state.apply(&DesktopEvent::DaemonIdentityVerified {
            identity: config.expected_daemon_identity.clone(),
        });
        state.apply(&connected);
        assert!(state.connected);
        assert_eq!(
            state.connected_identity.as_ref(),
            Some(&config.expected_daemon_identity)
        );

        state.apply(&DesktopEvent::OpsConnectionUpdate {
            connected: false,
            error: Some("socket closed".to_string()),
        });
        assert!(!state.connected);
        assert!(state.connected_identity.is_none());
        assert!(state.pending_identity.is_none());
    }
}
