//! Tool dispatcher: serializes write tools, parallelizes read tools, and
//! enforces the risk floor per the agent-harness-patterns corpus.

use async_trait::async_trait;
use impulse_contracts::{
    ConcurrencyClass, RiskClass, ToolCallId, ToolEvent, ToolOutcome, ToolSpec,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::debug;

/// Result of dispatching a single tool call.
#[derive(Clone, Debug)]
pub struct DispatchResult {
    /// The call id.
    pub call_id: ToolCallId,
    /// The outcome.
    pub outcome: ToolOutcome,
    /// How long the call took.
    pub duration: Duration,
}

/// Errors raised by the tool dispatcher.
#[derive(Debug, Error)]
pub enum ToolExecutionError {
    /// Tool name was not registered.
    #[error("tool {0:?} not registered")]
    UnknownTool(String),

    /// Risk floor was raised above the tool's risk; we refused to lower it.
    #[error("risk floor for {tool:?} cannot be lowered from {applied:?} to {requested:?}")]
    RiskFloorViolated {
        /// Tool name.
        tool: String,
        /// Risk the orchestrator wants to apply.
        applied: RiskClass,
        /// Risk the tool's own spec says.
        requested: RiskClass,
    },

    /// Tool execution exceeded its timeout.
    #[error("tool {tool:?} exceeded {timeout:?} timeout")]
    Timeout {
        /// Tool name.
        tool: String,
        /// How long we waited.
        timeout: Duration,
    },

    /// Tool returned an error that the executor surfaced.
    #[error("tool {tool:?} failed: {reason}")]
    ExecutionFailed {
        /// Tool name.
        tool: String,
        /// Failure reason.
        reason: String,
    },
}

/// A user-implemented tool executor.
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute the tool with the given args.
    ///
    /// # Errors
    /// Implementations should return `ToolExecutionError::ExecutionFailed` for
    /// permanent failures and a different variant for transient ones.
    async fn execute(
        &self,
        spec: &ToolSpec,
        args: serde_json::Value,
    ) -> Result<String, ToolExecutionError>;
}

/// Tool dispatcher. Holds the per-concurrency-class semaphores and a registry
/// of tool specs.
pub struct ToolDispatcher {
    tools: Arc<Mutex<HashMap<String, ToolSpec>>>,
    write_serial: Arc<Semaphore>,
    read_serial: Arc<Semaphore>,
    special: Arc<Semaphore>,
    /// Cap on how long any single tool call may run.
    default_timeout: Duration,
}

impl ToolDispatcher {
    /// Create a dispatcher with default concurrency limits (1 write, 1 special, 4 read).
    #[must_use]
    pub fn new() -> Self {
        Self::with_limits(1, 1, 4, Duration::from_secs(60))
    }

    /// Create a dispatcher with explicit limits.
    #[must_use]
    pub fn with_limits(
        write_permits: usize,
        special_permits: usize,
        read_permits: usize,
        default_timeout: Duration,
    ) -> Self {
        Self {
            tools: Arc::new(Mutex::new(HashMap::new())),
            write_serial: Arc::new(Semaphore::new(write_permits)),
            read_serial: Arc::new(Semaphore::new(read_permits.max(1))),
            special: Arc::new(Semaphore::new(special_permits)),
            default_timeout,
        }
    }

    /// Register a tool.
    pub fn register(&self, spec: ToolSpec) {
        let name = spec.name.clone();
        spec.validate().expect("tool spec must be valid");
        self.tools.lock().insert(name, spec);
    }

    /// Number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.lock().len()
    }

    /// Whether the dispatcher has no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.lock().is_empty()
    }

    /// List tool names sorted.
    #[must_use]
    pub fn list_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.lock().keys().cloned().collect();
        names.sort();
        names
    }

    /// Dispatch a tool call.
    ///
    /// # Errors
    /// Returns [`ToolExecutionError::UnknownTool`] if the tool isn't registered,
    /// or the underlying executor's error otherwise.
    pub async fn dispatch(
        &self,
        tool_name: &str,
        args: serde_json::Value,
        applied_risk: RiskClass,
        executor: Arc<dyn ToolExecutor>,
    ) -> Result<DispatchResult, ToolExecutionError> {
        let spec = self
            .tools
            .lock()
            .get(tool_name)
            .cloned()
            .ok_or_else(|| ToolExecutionError::UnknownTool(tool_name.to_owned()))?;

        if applied_risk < spec.risk {
            return Err(ToolExecutionError::RiskFloorViolated {
                tool: tool_name.to_owned(),
                applied: applied_risk,
                requested: spec.risk,
            });
        }

        let permit = self.acquire_for(&spec.concurrency).await;
        let started = Instant::now();
        let result =
            tokio::time::timeout(self.default_timeout, executor.execute(&spec, args)).await;
        drop(permit);

        let outcome = match result {
            Err(_) => Err(ToolExecutionError::Timeout {
                tool: tool_name.to_owned(),
                timeout: self.default_timeout,
            }),
            Ok(Ok(summary)) => Ok(ToolOutcome::Success { summary }),
            Ok(Err(e)) => Err(e),
        };

        let duration = started.elapsed();
        let call_id = ToolCallId::new();
        debug!(tool = tool_name, ?duration, "tool dispatched");
        match outcome {
            Ok(outcome) => Ok(DispatchResult {
                call_id,
                outcome,
                duration,
            }),
            Err(ToolExecutionError::ExecutionFailed { reason, .. }) => Ok(DispatchResult {
                call_id,
                outcome: ToolOutcome::Failed { reason },
                duration,
            }),
            Err(e) => Err(e),
        }
    }

    async fn acquire_for(&self, class: &ConcurrencyClass) -> OwnedSemaphorePermit {
        match class {
            ConcurrencyClass::WriteSerial => Arc::clone(&self.write_serial)
                .acquire_owned()
                .await
                .expect("write semaphore closed"),
            ConcurrencyClass::ReadSerial => Arc::clone(&self.read_serial)
                .acquire_owned()
                .await
                .expect("read semaphore closed"),
            ConcurrencyClass::ReadParallel => Arc::clone(&self.read_serial)
                .acquire_owned()
                .await
                .expect("read semaphore closed"),
            ConcurrencyClass::Special => Arc::clone(&self.special)
                .acquire_owned()
                .await
                .expect("special semaphore closed"),
        }
    }

    /// Build a [`ToolEvent`] for a dispatched call. Use this when you want
    /// the dispatcher to also publish events to the orchestrator bus.
    #[must_use]
    pub fn event_for(
        &self,
        call_id: ToolCallId,
        spec: &ToolSpec,
        applied_risk: RiskClass,
        result: &DispatchResult,
        workspace_id: impulse_contracts::WorkspaceId,
        session_id: impulse_contracts::SessionId,
    ) -> ToolEvent {
        let mut ev = ToolEvent {
            id: call_id,
            session_id,
            workspace_id,
            tool: spec.clone(),
            applied_risk,
            started_at: chrono::Utc::now()
                - chrono::Duration::from_std(result.duration).unwrap_or_default(),
            finished_at: Some(chrono::Utc::now()),
            outcome: result.outcome.clone(),
        };
        ev.started_at =
            chrono::Utc::now() - chrono::Duration::from_std(result.duration).unwrap_or_default();
        ev
    }
}

impl Default for ToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    struct EchoExecutor;
    #[async_trait]
    impl ToolExecutor for EchoExecutor {
        async fn execute(
            &self,
            _spec: &ToolSpec,
            args: serde_json::Value,
        ) -> Result<String, ToolExecutionError> {
            Ok(args.to_string())
        }
    }

    struct FailingExecutor;
    #[async_trait]
    impl ToolExecutor for FailingExecutor {
        async fn execute(
            &self,
            spec: &ToolSpec,
            _args: serde_json::Value,
        ) -> Result<String, ToolExecutionError> {
            Err(ToolExecutionError::ExecutionFailed {
                tool: spec.name.clone(),
                reason: "boom".to_owned(),
            })
        }
    }

    fn spec(name: &str, risk: RiskClass, conc: ConcurrencyClass) -> ToolSpec {
        ToolSpec {
            name: name.to_owned(),
            description: String::new(),
            input_schema: json!({"type": "object"}),
            concurrency: conc,
            risk,
        }
    }

    #[test]
    fn dispatcher_rejects_lowering_risk_floor() {
        let d = ToolDispatcher::new();
        d.register(spec(
            "risky",
            RiskClass::High,
            ConcurrencyClass::WriteSerial,
        ));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async {
            d.dispatch("risky", json!({}), RiskClass::Low, Arc::new(EchoExecutor))
                .await
        });
        assert!(matches!(
            res,
            Err(ToolExecutionError::RiskFloorViolated { .. })
        ));
    }

    #[test]
    fn dispatcher_returns_unknown_tool_error() {
        let d = ToolDispatcher::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async {
            d.dispatch("nope", json!({}), RiskClass::Low, Arc::new(EchoExecutor))
                .await
        });
        assert!(matches!(res, Err(ToolExecutionError::UnknownTool(_))));
    }

    #[test]
    fn dispatcher_succeeds_for_registered_tool() {
        let d = ToolDispatcher::new();
        d.register(spec("echo", RiskClass::Low, ConcurrencyClass::ReadParallel));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async {
            d.dispatch(
                "echo",
                json!({"x": 1}),
                RiskClass::Low,
                Arc::new(EchoExecutor),
            )
            .await
        });
        let r = res.expect("dispatch ok");
        assert!(matches!(r.outcome, ToolOutcome::Success { .. }));
    }

    #[test]
    fn dispatcher_surfaces_executor_failure() {
        let d = ToolDispatcher::new();
        d.register(spec("boom", RiskClass::Low, ConcurrencyClass::ReadParallel));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let res = rt.block_on(async {
            d.dispatch("boom", json!({}), RiskClass::Low, Arc::new(FailingExecutor))
                .await
        });
        let r = res.expect("dispatch returns failure as Ok");
        assert!(matches!(r.outcome, ToolOutcome::Failed { .. }));
    }

    #[test]
    fn list_names_is_sorted() {
        let d = ToolDispatcher::new();
        d.register(spec("zeta", RiskClass::Low, ConcurrencyClass::ReadParallel));
        d.register(spec(
            "alpha",
            RiskClass::Low,
            ConcurrencyClass::ReadParallel,
        ));
        assert_eq!(d.list_names(), vec!["alpha".to_owned(), "zeta".to_owned()]);
    }
}
