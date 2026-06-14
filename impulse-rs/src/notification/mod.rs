//! Notification system for multi-agent visibility and coordination
//!
//! This module provides a notification system that allows Impulse to track
//! and notify about agent activity across multiple coding agents (Claude Code,
//! Codex, OpenCode).
//!
//! ## Key Features
//!
//! - Agent activity tracking (start, end, tool use, errors)
//! - Event bus for real-time notifications
//! - Notification persistence to file
//! - Session-aware context for agents
//! - Webhook notifications for conflicts
//!
//! ## Design
//!
//! The notification system uses an event-driven architecture:
//! - `NotificationEvent` - represents something that happened
//! - `Notification` - a formatted notification with metadata
//! - `NotificationBus` - pub/sub system for notifications
//! - `NotificationStore` - persists notifications to disk
//! - `WebhookNotifier` - sends conflict notifications to external systems

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum notifications to keep in memory
const MAX_IN_MEMORY_NOTIFICATIONS: usize = 1000;

/// Maximum notifications to persist to disk (test-only: NotificationStore is test-gated)
#[cfg(test)]
const MAX_PERSISTED_NOTIFICATIONS: usize = 10000;

/// Maximum webhook retry attempts
const WEBHOOK_MAX_RETRIES: u32 = 3;

/// Webhook retry delay in milliseconds
const WEBHOOK_RETRY_DELAY_MS: u64 = 500;

/// Per-attempt webhook request timeout. Bounds each retry so a hung or slow
/// webhook endpoint can never block the notification path indefinitely.
const WEBHOOK_REQUEST_TIMEOUT_SECS: u64 = 15;
/// Webhook connection-establishment timeout (fail fast when unreachable).
const WEBHOOK_CONNECT_TIMEOUT_SECS: u64 = 5;

/// Represents an event that can trigger notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NotificationEvent {
    /// An agent session started
    AgentStarted {
        agent_id: String,
        agent_name: String,
        platform: String,
        working_directory: Option<String>,
    },
    /// An agent session ended
    AgentEnded {
        agent_id: String,
        agent_name: String,
        summary: Option<String>,
    },
    /// An agent used a tool
    ToolUsed {
        agent_id: String,
        tool_name: String,
        success: bool,
    },
    /// An error occurred with an agent
    AgentError { agent_id: String, error: String },
    /// Context was injected into an agent
    ContextInjected {
        agent_id: String,
        source: String,
        size_chars: usize,
    },
    /// A handoff occurred between agents
    AgentHandoff {
        from_agent: String,
        to_agent: String,
        task: String,
    },
    /// Multiple agents are active simultaneously
    MultiAgentDetected { agents: Vec<String> },
    /// Agent became active after being idle
    AgentBecameActive {
        agent_id: String,
        agent_name: String,
    },
    /// Agent became idle
    AgentBecameIdle {
        agent_id: String,
        agent_name: String,
        idle_duration_secs: i64,
    },
    /// Context window threshold crossed for an agent pane
    ContextThresholdCrossed {
        pane_id: usize,
        pane_name: String,
        threshold_pct: u8,
        tier: String,
    },
    /// Compaction detected in an agent pane
    CompactionDetected { pane_id: usize, pane_name: String },
    /// Context was refreshed for an agent pane
    ContextRefreshed {
        pane_id: usize,
        pane_name: String,
        tier: String,
        size_chars: usize,
    },
    /// A file conflict was detected between agents
    ConflictDetected {
        file_path: String,
        panes_involved: Vec<String>,
        description: String,
    },
    /// A file conflict was resolved
    ConflictResolved {
        file_path: String,
        resolution: String,
    },
}

impl NotificationEvent {
    /// Get the severity level of this event
    pub fn severity(&self) -> NotificationSeverity {
        match self {
            NotificationEvent::AgentStarted { .. } => NotificationSeverity::Info,
            NotificationEvent::AgentEnded { .. } => NotificationSeverity::Info,
            NotificationEvent::ToolUsed { success, .. } => {
                if *success {
                    NotificationSeverity::Debug
                } else {
                    NotificationSeverity::Warning
                }
            }
            NotificationEvent::AgentError { .. } => NotificationSeverity::Error,
            NotificationEvent::ContextInjected { .. } => NotificationSeverity::Debug,
            NotificationEvent::AgentHandoff { .. } => NotificationSeverity::Info,
            NotificationEvent::MultiAgentDetected { .. } => NotificationSeverity::Info,
            NotificationEvent::AgentBecameActive { .. } => NotificationSeverity::Debug,
            NotificationEvent::AgentBecameIdle { .. } => NotificationSeverity::Debug,
            NotificationEvent::ContextThresholdCrossed { .. } => NotificationSeverity::Info,
            NotificationEvent::CompactionDetected { .. } => NotificationSeverity::Warning,
            NotificationEvent::ContextRefreshed { .. } => NotificationSeverity::Debug,
            NotificationEvent::ConflictDetected { .. } => NotificationSeverity::Warning,
            NotificationEvent::ConflictResolved { .. } => NotificationSeverity::Info,
        }
    }

    /// Get the agent ID involved in this event (if any)
    #[must_use]
    pub fn agent_id(&self) -> Option<&str> {
        match self {
            NotificationEvent::AgentStarted { agent_id, .. } => Some(agent_id),
            NotificationEvent::AgentEnded { agent_id, .. } => Some(agent_id),
            NotificationEvent::ToolUsed { agent_id, .. } => Some(agent_id),
            NotificationEvent::AgentError { agent_id, .. } => Some(agent_id),
            NotificationEvent::ContextInjected { agent_id, .. } => Some(agent_id),
            NotificationEvent::AgentHandoff {
                from_agent,
                to_agent: _,
                ..
            } => Some(from_agent.as_str()),
            NotificationEvent::MultiAgentDetected { .. } => None,
            NotificationEvent::AgentBecameActive { agent_id, .. } => Some(agent_id),
            NotificationEvent::AgentBecameIdle { agent_id, .. } => Some(agent_id),
            NotificationEvent::ContextThresholdCrossed { pane_name, .. } => Some(pane_name),
            NotificationEvent::CompactionDetected { pane_name, .. } => Some(pane_name),
            NotificationEvent::ContextRefreshed { pane_name, .. } => Some(pane_name),
            NotificationEvent::ConflictDetected { panes_involved, .. } => {
                panes_involved.first().map(String::as_str)
            }
            NotificationEvent::ConflictResolved { .. } => None,
        }
    }

    /// Format this event as a human-readable message
    #[must_use]
    pub fn format_message(&self) -> String {
        match self {
            NotificationEvent::AgentStarted {
                agent_id: _,
                agent_name,
                platform,
                working_directory,
            } => {
                let dir = working_directory
                    .as_ref()
                    .map(|d| format!(" in {}", d))
                    .unwrap_or_default();
                format!("Agent '{}' ({}) started{}", agent_name, platform, dir)
            }
            NotificationEvent::AgentEnded {
                agent_id: _,
                agent_name,
                summary,
            } => {
                let sum = summary
                    .as_ref()
                    .map(|s| format!(" - {}", s))
                    .unwrap_or_default();
                format!("Agent '{}' ended{}", agent_name, sum)
            }
            NotificationEvent::ToolUsed {
                agent_id,
                tool_name,
                success,
            } => {
                let status = if *success { "✓" } else { "✗" };
                format!("{} Agent {} used tool: {}", status, agent_id, tool_name)
            }
            NotificationEvent::AgentError { agent_id, error } => {
                format!("Error with agent {}: {}", agent_id, error)
            }
            NotificationEvent::ContextInjected {
                agent_id,
                source,
                size_chars,
            } => format!(
                "Injected {} chars from {} into agent {}",
                size_chars, source, agent_id
            ),
            NotificationEvent::AgentHandoff {
                from_agent,
                to_agent,
                task,
            } => format!("Handoff from {} to {}: {}", from_agent, to_agent, task),
            NotificationEvent::MultiAgentDetected { agents } => {
                format!("Multiple agents active: {}", agents.join(", "))
            }
            NotificationEvent::AgentBecameActive { agent_name, .. } => {
                format!("Agent '{}' became active", agent_name)
            }
            NotificationEvent::AgentBecameIdle {
                agent_id: _,
                agent_name,
                idle_duration_secs,
            } => {
                let mins = idle_duration_secs / 60;
                format!("Agent '{}' idle for {}m", agent_name, mins)
            }
            NotificationEvent::ContextThresholdCrossed {
                pane_id: _,
                pane_name,
                threshold_pct,
                tier,
            } => {
                format!(
                    "Context threshold {}% ({}) crossed for pane '{}'",
                    threshold_pct, tier, pane_name
                )
            }
            NotificationEvent::CompactionDetected {
                pane_id: _,
                pane_name,
            } => {
                format!("Compaction detected in pane '{}'", pane_name)
            }
            NotificationEvent::ContextRefreshed {
                pane_id: _,
                pane_name,
                tier,
                size_chars,
            } => {
                format!(
                    "Context refreshed ({}, {} chars) for pane '{}'",
                    tier, size_chars, pane_name
                )
            }
            NotificationEvent::ConflictDetected {
                file_path,
                panes_involved,
                description,
            } => {
                format!(
                    "Conflict detected: {} [{}] - {}",
                    file_path,
                    panes_involved.join(", "),
                    description
                )
            }
            NotificationEvent::ConflictResolved {
                file_path,
                resolution,
            } => {
                format!("Conflict resolved: {} ({})", file_path, resolution)
            }
        }
    }
}

/// Severity level for notifications
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationSeverity {
    Debug,
    Info,
    Warning,
    Error,
}

impl std::fmt::Display for NotificationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationSeverity::Debug => write!(f, "debug"),
            NotificationSeverity::Info => write!(f, "info"),
            NotificationSeverity::Warning => write!(f, "warning"),
            NotificationSeverity::Error => write!(f, "error"),
        }
    }
}

/// A notification ready for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Unique ID for this notification
    pub id: String,
    /// When this notification was created
    pub timestamp: DateTime<Utc>,
    /// The event that triggered this notification
    pub event: NotificationEvent,
    /// Human-readable message
    pub message: String,
    /// Severity level
    pub severity: NotificationSeverity,
    /// Whether this notification has been read
    pub read: bool,
    /// Related agent IDs
    pub agent_ids: Vec<String>,
}

impl Notification {
    /// Create a new notification from an event
    pub fn new(event: NotificationEvent) -> Self {
        let message = event.format_message();
        let severity = event.severity();
        let agent_ids: Vec<String> = event
            .agent_id()
            .map(|id| vec![id.to_string()])
            .unwrap_or_default();

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event,
            message,
            severity,
            read: false,
            agent_ids,
        }
    }

    /// Mark this notification as read
    pub fn mark_read(&mut self) {
        self.read = true;
    }
}

/// Subscriber callback type
type NotificationSubscriber = Arc<dyn Fn(&Notification) + Send + Sync>;

/// The notification bus - pub/sub for notifications
pub struct NotificationBus {
    notifications: RwLock<VecDeque<Notification>>,
    subscribers: RwLock<Vec<NotificationSubscriber>>,
}

impl Default for NotificationBus {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationBus {
    /// Create a new notification bus
    pub fn new() -> Self {
        Self {
            notifications: RwLock::new(VecDeque::with_capacity(MAX_IN_MEMORY_NOTIFICATIONS)),
            subscribers: RwLock::new(Vec::new()),
        }
    }

    /// Publish a notification
    pub async fn publish(&self, event: NotificationEvent) {
        let notification = Notification::new(event);

        // Add to in-memory store
        {
            let mut notifications = self.notifications.write().await;
            if notifications.len() >= MAX_IN_MEMORY_NOTIFICATIONS {
                notifications.pop_front();
            }
            notifications.push_back(notification.clone());
        }

        // Notify subscribers
        {
            let subscribers = self.subscribers.read().await;
            for subscriber in subscribers.iter() {
                subscriber(&notification);
            }
        }
    }

    /// Get recent notifications (test-only: no production callers)
    #[cfg(test)]
    pub async fn recent(&self, limit: usize) -> Vec<Notification> {
        let notifications = self.notifications.read().await;
        notifications.iter().rev().take(limit).cloned().collect()
    }

    /// Get unread notifications (test-only: no production callers)
    #[cfg(test)]
    pub async fn unread(&self) -> Vec<Notification> {
        let notifications = self.notifications.read().await;
        notifications.iter().filter(|n| !n.read).cloned().collect()
    }

    /// Get notifications for a specific agent (test-only: no production callers)
    #[cfg(test)]
    pub async fn for_agent(&self, agent_id: &str) -> Vec<Notification> {
        let notifications = self.notifications.read().await;
        notifications
            .iter()
            .filter(|n| n.agent_ids.contains(&agent_id.to_string()))
            .cloned()
            .collect()
    }

    /// Mark notifications as read (test-only: no production callers)
    #[cfg(test)]
    pub async fn mark_read(&self, ids: &[String]) {
        let mut notifications = self.notifications.write().await;
        for notification in notifications.iter_mut() {
            if ids.contains(&notification.id) {
                notification.mark_read();
            }
        }
    }

    /// Subscribe to notifications (test-only: no production callers)
    #[cfg(test)]
    pub async fn subscribe<F>(&self, callback: F)
    where
        F: Fn(&Notification) + Send + Sync + 'static,
    {
        let mut subscribers = self.subscribers.write().await;
        subscribers.push(Arc::new(callback));
    }
}

/// Store for persisting notifications to disk (test-only: no production callers)
#[cfg(test)]
pub struct NotificationStore {
    base_path: PathBuf,
}

#[cfg(test)]
impl NotificationStore {
    /// Create a new notification store
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Get the path to the notifications file
    fn notifications_path(&self) -> PathBuf {
        self.base_path.join("notifications.jsonl")
    }

    /// Save a notification to disk
    pub fn save(&self, notification: &Notification) -> std::io::Result<()> {
        use std::io::Write;

        let path = self.notifications_path();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;

        let json = serde_json::to_string(notification)?;
        writeln!(file, "{}", json)?;

        // Trim if too large
        self.trim()?;

        Ok(())
    }

    /// Load recent notifications from disk
    pub fn load_recent(&self, limit: usize) -> std::io::Result<Vec<Notification>> {
        let path = self.notifications_path();
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&path)?;
        let notifications: Vec<Notification> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .rev()
            .take(limit)
            .collect();

        Ok(notifications)
    }

    /// Trim the notifications file if it exceeds the limit.
    /// Uses atomic temp-file + rename to prevent data loss on crash.
    fn trim(&self) -> std::io::Result<()> {
        let path = self.notifications_path();
        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&path)?;
        let lines: Vec<&str> = content.lines().collect();

        if lines.len() > MAX_PERSISTED_NOTIFICATIONS {
            let to_keep: Vec<&str> = lines
                .iter()
                .rev()
                .take(MAX_PERSISTED_NOTIFICATIONS)
                .cloned()
                .collect();

            let trimmed = to_keep.into_iter().rev().collect::<Vec<_>>().join("\n");

            // Atomic write: temp file + rename
            let tmp_path = path.with_extension(format!(
                "tmp.{}.{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            std::fs::write(&tmp_path, &trimmed)?;
            std::fs::rename(&tmp_path, &path)?;
        }

        Ok(())
    }
}

/// Payload sent to webhook for conflict notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictWebhookPayload {
    pub event_type: String,
    pub timestamp: DateTime<Utc>,
    pub file_path: String,
    pub panes_involved: Vec<String>,
    pub description: String,
}

/// Webhook notifier for sending conflict notifications to external systems
pub struct WebhookNotifier {
    client: reqwest::Client,
}

impl Default for WebhookNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl WebhookNotifier {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(WEBHOOK_REQUEST_TIMEOUT_SECS))
            .connect_timeout(std::time::Duration::from_secs(WEBHOOK_CONNECT_TIMEOUT_SECS))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { client }
    }

    /// Send conflict notification to webhook URL with retry logic
    pub async fn notify_conflict(
        &self,
        webhook_url: &str,
        file_path: &str,
        panes_involved: Vec<String>,
        description: &str,
    ) -> Result<(), String> {
        let payload = ConflictWebhookPayload {
            event_type: "conflict_detected".to_string(),
            timestamp: Utc::now(),
            file_path: file_path.to_string(),
            panes_involved,
            description: description.to_string(),
        };

        let mut last_error = None;

        for attempt in 1..=WEBHOOK_MAX_RETRIES {
            match self.send_webhook(webhook_url, &payload).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < WEBHOOK_MAX_RETRIES {
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            WEBHOOK_RETRY_DELAY_MS * attempt as u64,
                        ))
                        .await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| "Unknown webhook error".to_string()))
    }

    async fn send_webhook(
        &self,
        webhook_url: &str,
        payload: &ConflictWebhookPayload,
    ) -> Result<(), String> {
        self.client
            .post(webhook_url)
            .json(payload)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?
            .error_for_status()
            .map_err(|e| format!("HTTP error: {}", e))?;

        Ok(())
    }
}
pub async fn emit_conflict_detected(
    bus: &NotificationBus,
    file_path: &str,
    panes_involved: Vec<String>,
    description: &str,
) {
    bus.publish(NotificationEvent::ConflictDetected {
        file_path: file_path.to_string(),
        panes_involved,
        description: description.to_string(),
    })
    .await;
}

pub async fn emit_conflict_resolved(bus: &NotificationBus, file_path: &str, resolution: &str) {
    bus.publish(NotificationEvent::ConflictResolved {
        file_path: file_path.to_string(),
        resolution: resolution.to_string(),
    })
    .await;
}

/// Send conflict notification to webhook if configured
pub async fn send_conflict_webhook(
    webhook_url: Option<&str>,
    file_path: &str,
    panes_involved: Vec<String>,
    description: &str,
) {
    if let Some(url) = webhook_url {
        let notifier = WebhookNotifier::new();
        if let Err(e) = notifier
            .notify_conflict(url, file_path, panes_involved, description)
            .await
        {
            tracing::warn!("Failed to send conflict webhook: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_notification_creation() {
        let event = NotificationEvent::AgentStarted {
            agent_id: "test-1".to_string(),
            agent_name: "Claude".to_string(),
            platform: "claude-code".to_string(),
            working_directory: Some("/test".to_string()),
        };
        let notification = Notification::new(event);

        assert!(!notification.id.is_empty());
        assert_eq!(notification.severity, NotificationSeverity::Info);
        assert!(!notification.read);
    }

    #[tokio::test]
    async fn test_notification_bus_publish() {
        let bus = NotificationBus::new();

        bus.publish(NotificationEvent::AgentStarted {
            agent_id: "test-1".to_string(),
            agent_name: "Claude".to_string(),
            platform: "claude-code".to_string(),
            working_directory: None,
        })
        .await;

        let recent = bus.recent(10).await;
        assert_eq!(recent.len(), 1);
    }

    #[tokio::test]
    async fn test_notification_bus_unread() {
        let bus = NotificationBus::new();

        bus.publish(NotificationEvent::AgentStarted {
            agent_id: "test-1".to_string(),
            agent_name: "Claude".to_string(),
            platform: "claude-code".to_string(),
            working_directory: None,
        })
        .await;

        let unread = bus.unread().await;
        assert_eq!(unread.len(), 1);

        let all = bus.recent(10).await;
        bus.mark_read(&[all[0].id.clone()]).await;

        let unread_after = bus.unread().await;
        assert_eq!(unread_after.len(), 0);
    }

    #[tokio::test]
    async fn test_notification_bus_for_agent() {
        let bus = NotificationBus::new();

        bus.publish(NotificationEvent::AgentStarted {
            agent_id: "agent-1".to_string(),
            agent_name: "Claude".to_string(),
            platform: "claude-code".to_string(),
            working_directory: None,
        })
        .await;

        bus.publish(NotificationEvent::ToolUsed {
            agent_id: "agent-2".to_string(),
            tool_name: "test".to_string(),
            success: true,
        })
        .await;

        let for_agent_1 = bus.for_agent("agent-1").await;
        assert_eq!(for_agent_1.len(), 1);

        let for_agent_2 = bus.for_agent("agent-2").await;
        assert_eq!(for_agent_2.len(), 1);
    }

    #[tokio::test]
    async fn test_notification_bus_subscribe() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let bus = NotificationBus::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let counter = call_count.clone();

        bus.subscribe(move |_notification| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .await;

        bus.publish(NotificationEvent::AgentStarted {
            agent_id: "test-sub".to_string(),
            agent_name: "Test".to_string(),
            platform: "test".to_string(),
            working_directory: None,
        })
        .await;

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_notification_store_save_load() {
        let temp = tempfile::TempDir::new().unwrap();
        let store = NotificationStore::new(temp.path().to_path_buf());

        let event = NotificationEvent::AgentStarted {
            agent_id: "store-test".to_string(),
            agent_name: "StoreTest".to_string(),
            platform: "test".to_string(),
            working_directory: Some("/tmp".to_string()),
        };
        let notification = Notification::new(event);
        store.save(&notification).unwrap();

        let loaded = store.load_recent(10).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].agent_ids, vec!["store-test".to_string()]);
    }

    #[tokio::test]
    async fn test_webhook_notifier_creates_client() {
        let notifier = WebhookNotifier::new();
        // Verify the client can be used to build requests
        let _request = notifier
            .client
            .request(reqwest::Method::GET, "http://localhost");
    }

    #[tokio::test]
    async fn test_webhook_payload_serialization() {
        let payload = ConflictWebhookPayload {
            event_type: "conflict_detected".to_string(),
            timestamp: Utc::now(),
            file_path: "/path/to/file.rs".to_string(),
            panes_involved: vec!["pane-1".to_string(), "pane-2".to_string()],
            description: "File modified by multiple agents".to_string(),
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("conflict_detected"));
        assert!(json.contains("/path/to/file.rs"));
    }

    #[tokio::test]
    async fn test_webhook_fails_for_invalid_url() {
        let notifier = WebhookNotifier::new();

        let result = notifier
            .notify_conflict(
                "http://invalid-domain-that-does-not-exist.local",
                "/test/file.rs",
                vec!["pane-1".to_string()],
                "Test conflict",
            )
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_notification_severity_round_trip() {
        for severity in [
            NotificationSeverity::Debug,
            NotificationSeverity::Info,
            NotificationSeverity::Warning,
            NotificationSeverity::Error,
        ] {
            let json = serde_json::to_string(&severity).unwrap();
            let recovered: NotificationSeverity = serde_json::from_str(&json).unwrap();
            assert_eq!(severity, recovered);
        }
    }

    #[test]
    fn test_notification_event_agent_started_round_trip() {
        let event = NotificationEvent::AgentStarted {
            agent_id: "agent-42".to_string(),
            agent_name: "Claude".to_string(),
            platform: "claude-code".to_string(),
            working_directory: Some("/home/dev/project".to_string()),
        };

        let json = serde_json::to_string(&event).unwrap();
        let recovered: NotificationEvent = serde_json::from_str(&json).unwrap();

        match recovered {
            NotificationEvent::AgentStarted {
                agent_id,
                agent_name,
                platform,
                working_directory,
            } => {
                assert_eq!(agent_id, "agent-42");
                assert_eq!(agent_name, "Claude");
                assert_eq!(platform, "claude-code");
                assert_eq!(working_directory, Some("/home/dev/project".to_string()));
            }
            other => panic!("Expected AgentStarted, got {:?}", other),
        }
    }

    #[test]
    fn test_conflict_webhook_payload_round_trip() {
        let payload = ConflictWebhookPayload {
            event_type: "conflict_detected".to_string(),
            timestamp: Utc::now(),
            file_path: "/src/main.rs".to_string(),
            panes_involved: vec!["pane-1".to_string(), "pane-2".to_string()],
            description: "Concurrent modification".to_string(),
        };

        let json = serde_json::to_string(&payload).unwrap();
        let recovered: ConflictWebhookPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.event_type, payload.event_type);
        assert_eq!(recovered.timestamp, payload.timestamp);
        assert_eq!(recovered.file_path, payload.file_path);
        assert_eq!(recovered.panes_involved, payload.panes_involved);
        assert_eq!(recovered.description, payload.description);
    }

    #[test]
    fn test_notification_round_trip() {
        let event = NotificationEvent::ToolUsed {
            agent_id: "agent-1".to_string(),
            tool_name: "file_write".to_string(),
            success: true,
        };
        let notification = Notification::new(event);

        let json = serde_json::to_string(&notification).unwrap();
        let recovered: Notification = serde_json::from_str(&json).unwrap();
        assert_eq!(recovered.id, notification.id);
        assert_eq!(recovered.timestamp, notification.timestamp);
        assert_eq!(recovered.message, notification.message);
        assert_eq!(recovered.severity, notification.severity);
        assert_eq!(recovered.read, notification.read);
        assert_eq!(recovered.agent_ids, notification.agent_ids);
    }
}
