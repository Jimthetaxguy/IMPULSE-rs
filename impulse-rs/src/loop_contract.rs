//! Canonical loop contract for Impulse-owned iterative loops (ADR-0017).
//!
//! Before this module, loop safety lived in scattered per-call-site
//! constants: the Ion tool loop capped rounds and wall-clock time, and the
//! harness subprocess query had its own timeout. Nothing declared, in one
//! typed place, what a loop may spend, when it must stop, and what it leaves
//! behind when it stops. This module supplies that contract:
//!
//! - [`LoopBudget`] / [`LoopContract`]: the declared budget a loop runs under.
//! - [`LoopBreaker`]: the per-run state machine that evaluates every trip
//!   condition on every round (a circuit breaker in the CLOSED state; a trip
//!   opens it and the loop must stop).
//! - [`LoopTrip`] / [`LoopTermination`] / [`LoopReport`]: typed termination
//!   evidence. A report is loop evidence for operators and future harness
//!   diagnosis; it decides nothing on its own.
//!
//! The module is deliberately independent of provider, tool, and daemon
//! types so the same contract can bound Ion tool loops today and governed
//! Builder iterations later. Wall-clock enforcement stays with the caller
//! (`tokio::time::timeout`); the breaker records the trip so the report is
//! complete either way.

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default round cap for the Ion tool loop (TUI_SPEC.md T9).
pub const ION_DEFAULT_MAX_ROUNDS: usize = 10;
/// Default wall-clock budget for one Ion tool-loop exchange.
pub const ION_DEFAULT_WALL_CLOCK: Duration = Duration::from_secs(180);
/// Default consecutive identical tool calls that trip the Ion loop.
pub const ION_DEFAULT_REPEATED_CALL_STREAK: usize = 3;
/// Default consecutive identical tool errors that trip the Ion loop.
pub const ION_DEFAULT_SAME_ERROR_STREAK: usize = 3;

/// Longest error signature retained for same-error detection and reports.
const ERROR_SIGNATURE_MAX_CHARS: usize = 120;

/// What one loop may spend before it must stop.
///
/// `max_rounds` and `wall_clock` are hard caps and always enforced. The two
/// streak limits detect a loop that is technically still running but no
/// longer making progress: a model re-issuing the exact same tool call, or a
/// tool failing the same way over and over. `None` disables that detector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopBudget {
    /// Maximum model round trips in one loop run. Must be non-zero.
    pub max_rounds: usize,
    /// Wall-clock budget for the entire run. Must be non-zero.
    pub wall_clock: Duration,
    /// Trip when this many consecutive tool calls are identical
    /// (same tool name and structurally equal input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_repeated_call_streak: Option<usize>,
    /// Trip when this many consecutive tool results are errors from the same
    /// tool with the same error signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_same_error_streak: Option<usize>,
}

impl LoopBudget {
    /// The budget the Ion REPL tool loop runs under by default.
    pub fn ion_default() -> Self {
        Self {
            max_rounds: ION_DEFAULT_MAX_ROUNDS,
            wall_clock: ION_DEFAULT_WALL_CLOCK,
            max_repeated_call_streak: Some(ION_DEFAULT_REPEATED_CALL_STREAK),
            max_same_error_streak: Some(ION_DEFAULT_SAME_ERROR_STREAK),
        }
    }
}

/// A named loop plus the budget it declares before running.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopContract {
    /// Stable loop identity used in reports (for example `ion_tool_loop`).
    pub name: String,
    pub budget: LoopBudget,
}

/// Why a [`LoopContract`] cannot be run.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LoopContractError {
    #[error("loop contract '{name}' must allow at least one round")]
    ZeroRounds { name: String },
    #[error("loop contract '{name}' must have a non-zero wall-clock budget")]
    ZeroWallClock { name: String },
    #[error("loop contract '{name}' streak limit '{limit}' must be non-zero when set")]
    ZeroStreak { name: String, limit: &'static str },
}

impl LoopContract {
    /// The contract the Ion REPL tool loop runs under by default.
    pub fn ion_tool_loop() -> Self {
        Self {
            name: "ion_tool_loop".to_string(),
            budget: LoopBudget::ion_default(),
        }
    }

    /// Rejects budgets that could never run or could never trip.
    pub fn validate(&self) -> Result<(), LoopContractError> {
        if self.budget.max_rounds == 0 {
            return Err(LoopContractError::ZeroRounds {
                name: self.name.clone(),
            });
        }
        if self.budget.wall_clock.is_zero() {
            return Err(LoopContractError::ZeroWallClock {
                name: self.name.clone(),
            });
        }
        if self.budget.max_repeated_call_streak == Some(0) {
            return Err(LoopContractError::ZeroStreak {
                name: self.name.clone(),
                limit: "max_repeated_call_streak",
            });
        }
        if self.budget.max_same_error_streak == Some(0) {
            return Err(LoopContractError::ZeroStreak {
                name: self.name.clone(),
                limit: "max_same_error_streak",
            });
        }
        Ok(())
    }
}

/// The condition that opened the breaker and stopped the loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoopTrip {
    /// The round cap was reached without a final reply.
    RoundCap { rounds: usize },
    /// The wall-clock budget elapsed. Recorded by the caller that enforces
    /// the timeout.
    WallClock { seconds: u64 },
    /// The model issued the same tool call `streak` times in a row.
    RepeatedCall { tool: String, streak: usize },
    /// The same tool failed with the same error signature `streak` times in
    /// a row.
    SameError {
        tool: String,
        streak: usize,
        signature: String,
    },
}

impl fmt::Display for LoopTrip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoopTrip::RoundCap { rounds } => {
                write!(f, "round cap of {rounds} reached without a final reply")
            }
            LoopTrip::WallClock { seconds } => {
                write!(f, "wall-clock budget of {seconds}s elapsed")
            }
            LoopTrip::RepeatedCall { tool, streak } => write!(
                f,
                "tool '{tool}' was requested with identical input {streak} times in a row"
            ),
            LoopTrip::SameError {
                tool,
                streak,
                signature,
            } => write!(
                f,
                "tool '{tool}' failed {streak} times in a row with the same error: {signature}"
            ),
        }
    }
}

/// How a loop run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum LoopTermination {
    /// The loop produced its final result inside budget.
    Completed,
    /// A trip condition stopped the loop before a final result.
    Tripped { trip: LoopTrip },
}

/// Typed evidence left behind by one loop run, whether it completed or
/// tripped. Counts are facts about the run; they carry no verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopReport {
    pub contract: String,
    pub termination: LoopTermination,
    /// Rounds that actually began.
    pub rounds_used: usize,
    /// Tool calls executed across all rounds.
    pub tool_calls: usize,
    /// Tool calls whose result was an error.
    pub tool_errors: usize,
    pub elapsed_ms: u64,
}

/// Outcome of one executed tool call, as the breaker needs to see it.
#[derive(Debug, Clone, Copy)]
pub struct CallOutcome<'a> {
    pub is_error: bool,
    pub content: &'a str,
}

/// Per-run breaker state. Create one per loop run, call
/// [`LoopBreaker::begin_round`] before each model round and
/// [`LoopBreaker::observe_call`] after each executed tool call, and read
/// [`LoopBreaker::report`] when the run ends.
#[derive(Debug)]
pub struct LoopBreaker {
    contract: LoopContract,
    started: Instant,
    rounds_used: usize,
    tool_calls: usize,
    tool_errors: usize,
    last_call_key: Option<String>,
    repeated_call_streak: usize,
    last_error_key: Option<(String, String)>,
    same_error_streak: usize,
}

impl LoopBreaker {
    pub fn new(contract: LoopContract) -> Self {
        Self {
            contract,
            started: Instant::now(),
            rounds_used: 0,
            tool_calls: 0,
            tool_errors: 0,
            last_call_key: None,
            repeated_call_streak: 0,
            last_error_key: None,
            same_error_streak: 0,
        }
    }

    pub fn contract(&self) -> &LoopContract {
        &self.contract
    }

    pub fn rounds_used(&self) -> usize {
        self.rounds_used
    }

    /// Admits the next model round, returning its zero-based index, or trips
    /// when the round cap is already spent.
    pub fn begin_round(&mut self) -> Result<usize, LoopTrip> {
        let max_rounds = self.contract.budget.max_rounds;
        if self.rounds_used >= max_rounds {
            return Err(LoopTrip::RoundCap { rounds: max_rounds });
        }
        let index = self.rounds_used;
        self.rounds_used += 1;
        Ok(index)
    }

    /// Records one executed tool call and returns a trip if it pushed a
    /// streak past its limit. A different call resets the repeated-call
    /// streak; a non-error result or a different error resets the same-error
    /// streak.
    pub fn observe_call(
        &mut self,
        tool: &str,
        input: &serde_json::Value,
        outcome: CallOutcome<'_>,
    ) -> Option<LoopTrip> {
        self.tool_calls += 1;

        let call_key = format!("{tool}\u{1f}{}", canonical_json(input));
        if self.last_call_key.as_deref() == Some(call_key.as_str()) {
            self.repeated_call_streak += 1;
        } else {
            self.last_call_key = Some(call_key);
            self.repeated_call_streak = 1;
        }

        if outcome.is_error {
            self.tool_errors += 1;
            let key = (tool.to_string(), error_signature(outcome.content));
            if self.last_error_key.as_ref() == Some(&key) {
                self.same_error_streak += 1;
            } else {
                self.last_error_key = Some(key);
                self.same_error_streak = 1;
            }
        } else {
            self.last_error_key = None;
            self.same_error_streak = 0;
        }

        if let Some(limit) = self.contract.budget.max_repeated_call_streak {
            if self.repeated_call_streak >= limit {
                return Some(LoopTrip::RepeatedCall {
                    tool: tool.to_string(),
                    streak: self.repeated_call_streak,
                });
            }
        }
        if let Some(limit) = self.contract.budget.max_same_error_streak {
            if self.same_error_streak >= limit {
                if let Some((_, signature)) = &self.last_error_key {
                    return Some(LoopTrip::SameError {
                        tool: tool.to_string(),
                        streak: self.same_error_streak,
                        signature: signature.clone(),
                    });
                }
            }
        }
        None
    }

    /// The termination report for this run so far.
    pub fn report(&self, termination: LoopTermination) -> LoopReport {
        LoopReport {
            contract: self.contract.name.clone(),
            termination,
            rounds_used: self.rounds_used,
            tool_calls: self.tool_calls,
            tool_errors: self.tool_errors,
            elapsed_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
        }
    }
}

/// First line of an error, trimmed and bounded, so two failures that differ
/// only in trailing detail still count as the same failure.
pub fn error_signature(content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("").trim();
    first_line.chars().take(ERROR_SIGNATURE_MAX_CHARS).collect()
}

/// Serializes `value` with object keys sorted at every level, so two inputs
/// that differ only in key order compare equal.
pub fn canonical_json(value: &serde_json::Value) -> String {
    fn write(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (i, key) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::Value::String((*key).clone()).to_string());
                    out.push(':');
                    write(&map[*key], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn contract(
        max_rounds: usize,
        repeated: Option<usize>,
        same_error: Option<usize>,
    ) -> LoopContract {
        LoopContract {
            name: "test_loop".to_string(),
            budget: LoopBudget {
                max_rounds,
                wall_clock: Duration::from_secs(5),
                max_repeated_call_streak: repeated,
                max_same_error_streak: same_error,
            },
        }
    }

    const OK: CallOutcome<'static> = CallOutcome {
        is_error: false,
        content: "fine",
    };

    fn err(content: &str) -> CallOutcome<'_> {
        CallOutcome {
            is_error: true,
            content,
        }
    }

    #[test]
    fn round_trip_loop_budget() {
        let original = LoopBudget::ion_default();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: LoopBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_loop_budget_without_streaks() {
        let original = LoopBudget {
            max_rounds: 1,
            wall_clock: Duration::from_millis(1),
            max_repeated_call_streak: None,
            max_same_error_streak: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        assert!(
            !json.contains("streak"),
            "None streaks must be omitted: {json}"
        );
        let recovered: LoopBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_loop_contract() {
        let original = LoopContract::ion_tool_loop();
        let json = serde_json::to_string(&original).unwrap();
        let recovered: LoopContract = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn round_trip_loop_trip_every_variant() {
        let variants = [
            LoopTrip::RoundCap { rounds: 3 },
            LoopTrip::WallClock { seconds: 9 },
            LoopTrip::RepeatedCall {
                tool: "bash_exec".into(),
                streak: 3,
            },
            LoopTrip::SameError {
                tool: "file_read".into(),
                streak: 2,
                signature: "no such file".into(),
            },
        ];
        for original in variants {
            let json = serde_json::to_string(&original).unwrap();
            let recovered: LoopTrip = serde_json::from_str(&json).unwrap();
            assert_eq!(original, recovered);
        }
    }

    #[test]
    fn round_trip_loop_report() {
        let original = LoopReport {
            contract: "ion_tool_loop".into(),
            termination: LoopTermination::Tripped {
                trip: LoopTrip::RoundCap { rounds: 10 },
            },
            rounds_used: 10,
            tool_calls: 9,
            tool_errors: 2,
            elapsed_ms: 1234,
        };
        let json = serde_json::to_string(&original).unwrap();
        let recovered: LoopReport = serde_json::from_str(&json).unwrap();
        assert_eq!(original, recovered);

        let completed = LoopReport {
            termination: LoopTermination::Completed,
            ..original
        };
        let json = serde_json::to_string(&completed).unwrap();
        assert!(json.contains("\"outcome\":\"completed\""), "{json}");
        let recovered: LoopReport = serde_json::from_str(&json).unwrap();
        assert_eq!(completed, recovered);
    }

    #[test]
    fn test_ion_defaults_match_documented_constants() {
        let contract = LoopContract::ion_tool_loop();
        assert_eq!(contract.name, "ion_tool_loop");
        assert_eq!(contract.budget.max_rounds, 10);
        assert_eq!(contract.budget.wall_clock, Duration::from_secs(180));
        assert_eq!(contract.budget.max_repeated_call_streak, Some(3));
        assert_eq!(contract.budget.max_same_error_streak, Some(3));
        assert_eq!(contract.validate(), Ok(()));
    }

    #[test]
    fn test_validate_rejects_zero_rounds() {
        let err = contract(0, None, None).validate().unwrap_err();
        assert!(matches!(err, LoopContractError::ZeroRounds { .. }));
        assert!(format!("{err}").contains("test_loop"));
    }

    #[test]
    fn test_validate_rejects_zero_wall_clock() {
        let mut c = contract(1, None, None);
        c.budget.wall_clock = Duration::ZERO;
        let err = c.validate().unwrap_err();
        assert!(matches!(err, LoopContractError::ZeroWallClock { .. }));
        assert!(format!("{err}").contains("wall-clock"));
    }

    #[test]
    fn test_validate_rejects_zero_streak_limits() {
        let err = contract(1, Some(0), None).validate().unwrap_err();
        assert!(matches!(
            err,
            LoopContractError::ZeroStreak {
                limit: "max_repeated_call_streak",
                ..
            }
        ));
        let err = contract(1, None, Some(0)).validate().unwrap_err();
        assert!(matches!(
            err,
            LoopContractError::ZeroStreak {
                limit: "max_same_error_streak",
                ..
            }
        ));
        assert!(format!("{err}").contains("max_same_error_streak"));
    }

    #[test]
    fn test_begin_round_trips_when_cap_spent() {
        let mut breaker = LoopBreaker::new(contract(2, None, None));
        assert_eq!(breaker.begin_round(), Ok(0));
        assert_eq!(breaker.begin_round(), Ok(1));
        assert_eq!(breaker.begin_round(), Err(LoopTrip::RoundCap { rounds: 2 }));
        assert_eq!(breaker.rounds_used(), 2);
    }

    #[test]
    fn test_repeated_identical_call_trips_at_limit() {
        let mut breaker = LoopBreaker::new(contract(10, Some(3), None));
        let input = json!({"command": "ls"});
        assert_eq!(breaker.observe_call("bash_exec", &input, OK), None);
        assert_eq!(breaker.observe_call("bash_exec", &input, OK), None);
        assert_eq!(
            breaker.observe_call("bash_exec", &input, OK),
            Some(LoopTrip::RepeatedCall {
                tool: "bash_exec".into(),
                streak: 3,
            })
        );
    }

    #[test]
    fn test_different_call_resets_repeated_streak() {
        let mut breaker = LoopBreaker::new(contract(10, Some(3), None));
        let a = json!({"command": "ls"});
        let b = json!({"command": "pwd"});
        assert_eq!(breaker.observe_call("bash_exec", &a, OK), None);
        assert_eq!(breaker.observe_call("bash_exec", &a, OK), None);
        assert_eq!(breaker.observe_call("bash_exec", &b, OK), None);
        assert_eq!(breaker.observe_call("bash_exec", &a, OK), None);
        assert_eq!(breaker.observe_call("bash_exec", &a, OK), None);
        assert!(matches!(
            breaker.observe_call("bash_exec", &a, OK),
            Some(LoopTrip::RepeatedCall { streak: 3, .. })
        ));
    }

    #[test]
    fn test_same_tool_name_different_tool_is_not_repeated() {
        let mut breaker = LoopBreaker::new(contract(10, Some(2), None));
        let input = json!({"path": "x"});
        assert_eq!(breaker.observe_call("file_read", &input, OK), None);
        assert_eq!(breaker.observe_call("file_write", &input, OK), None);
    }

    #[test]
    fn test_repeated_call_ignores_key_order() {
        let mut breaker = LoopBreaker::new(contract(10, Some(2), None));
        let a = json!({"a": 1, "b": {"c": [1, 2], "d": "x"}});
        let b = json!({"b": {"d": "x", "c": [1, 2]}, "a": 1});
        assert_eq!(breaker.observe_call("t", &a, OK), None);
        assert!(matches!(
            breaker.observe_call("t", &b, OK),
            Some(LoopTrip::RepeatedCall { streak: 2, .. })
        ));
    }

    #[test]
    fn test_same_error_streak_trips_at_limit() {
        let mut breaker = LoopBreaker::new(contract(10, None, Some(3)));
        let mut inputs = (0..).map(|i| json!({"n": i}));
        assert_eq!(
            breaker.observe_call("bash_exec", &inputs.next().unwrap(), err("boom: 1")),
            None
        );
        assert_eq!(
            breaker.observe_call("bash_exec", &inputs.next().unwrap(), err("boom: 1\ndetail")),
            None
        );
        assert_eq!(
            breaker.observe_call("bash_exec", &inputs.next().unwrap(), err("  boom: 1  ")),
            Some(LoopTrip::SameError {
                tool: "bash_exec".into(),
                streak: 3,
                signature: "boom: 1".into(),
            })
        );
    }

    #[test]
    fn test_success_or_different_error_resets_same_error_streak() {
        let mut breaker = LoopBreaker::new(contract(10, None, Some(2)));
        let mut inputs = (0..).map(|i| json!({"n": i}));
        assert_eq!(
            breaker.observe_call("t", &inputs.next().unwrap(), err("boom")),
            None
        );
        assert_eq!(breaker.observe_call("t", &inputs.next().unwrap(), OK), None);
        assert_eq!(
            breaker.observe_call("t", &inputs.next().unwrap(), err("boom")),
            None
        );
        assert_eq!(
            breaker.observe_call("t", &inputs.next().unwrap(), err("other")),
            None
        );
        assert!(matches!(
            breaker.observe_call("t", &inputs.next().unwrap(), err("other")),
            Some(LoopTrip::SameError { streak: 2, .. })
        ));
    }

    #[test]
    fn test_same_error_from_different_tool_does_not_count() {
        let mut breaker = LoopBreaker::new(contract(10, None, Some(2)));
        assert_eq!(breaker.observe_call("a", &json!(1), err("boom")), None);
        assert_eq!(breaker.observe_call("b", &json!(2), err("boom")), None);
    }

    #[test]
    fn test_disabled_streak_detectors_never_trip() {
        let mut breaker = LoopBreaker::new(contract(100, None, None));
        let input = json!({"same": true});
        for _ in 0..50 {
            assert_eq!(breaker.observe_call("t", &input, err("boom")), None);
        }
        let report = breaker.report(LoopTermination::Completed);
        assert_eq!(report.tool_calls, 50);
        assert_eq!(report.tool_errors, 50);
    }

    #[test]
    fn test_report_counts_rounds_calls_and_errors() {
        let mut breaker = LoopBreaker::new(contract(5, None, None));
        breaker.begin_round().unwrap();
        breaker.observe_call("t", &json!(1), OK);
        breaker.observe_call("u", &json!(2), err("x"));
        breaker.begin_round().unwrap();
        let report = breaker.report(LoopTermination::Tripped {
            trip: LoopTrip::WallClock { seconds: 5 },
        });
        assert_eq!(report.contract, "test_loop");
        assert_eq!(report.rounds_used, 2);
        assert_eq!(report.tool_calls, 2);
        assert_eq!(report.tool_errors, 1);
        assert!(matches!(
            report.termination,
            LoopTermination::Tripped {
                trip: LoopTrip::WallClock { seconds: 5 }
            }
        ));
    }

    #[test]
    fn test_loop_trip_display_names_tool_and_streak() {
        let trip = LoopTrip::RepeatedCall {
            tool: "bash_exec".into(),
            streak: 3,
        };
        let msg = format!("{trip}");
        assert!(msg.contains("bash_exec"), "{msg}");
        assert!(msg.contains('3'), "{msg}");

        let trip = LoopTrip::SameError {
            tool: "file_read".into(),
            streak: 2,
            signature: "no such file".into(),
        };
        let msg = format!("{trip}");
        assert!(
            msg.contains("file_read") && msg.contains("no such file"),
            "{msg}"
        );

        assert!(format!("{}", LoopTrip::RoundCap { rounds: 10 }).contains("10"));
        assert!(format!("{}", LoopTrip::WallClock { seconds: 180 }).contains("180"));
    }

    #[test]
    fn test_error_signature_uses_trimmed_first_line_bounded() {
        assert_eq!(error_signature("  first \nsecond"), "first");
        assert_eq!(error_signature(""), "");
        let long = "x".repeat(500);
        assert_eq!(
            error_signature(&long).chars().count(),
            ERROR_SIGNATURE_MAX_CHARS
        );
    }

    #[test]
    fn test_canonical_json_sorts_nested_keys_and_preserves_arrays() {
        let value = json!({"z": [3, {"b": 1, "a": 2}], "a": null, "m": "s"});
        assert_eq!(
            canonical_json(&value),
            r#"{"a":null,"m":"s","z":[3,{"a":2,"b":1}]}"#
        );
        assert_eq!(canonical_json(&json!([2, 1])), "[2,1]");
        assert_eq!(canonical_json(&json!("q\"uote")), r#""q\"uote""#);
    }

    #[test]
    fn test_canonical_json_is_order_independent_for_permutations() {
        let keys = ["delta", "alpha", "charlie", "bravo", "echo"];
        let build = |order: &[&str]| {
            let mut map = serde_json::Map::new();
            for (i, key) in order.iter().enumerate() {
                map.insert((*key).to_string(), json!({"i": i, "k": key}));
            }
            serde_json::Value::Object(map)
        };
        let forward = build(&keys);
        let mut reversed = keys;
        reversed.reverse();
        let backward = build(&reversed);
        assert_ne!(
            forward, backward,
            "fixture must differ in value, not just order"
        );
        let mut sorted = keys;
        sorted.sort();
        let ordered = build(&sorted);
        assert_ne!(ordered, forward);
        // Same keys, same per-key value: identical canonical form regardless
        // of insertion order.
        let same_a = {
            let mut map = serde_json::Map::new();
            for key in keys {
                map.insert(key.to_string(), json!(key.len()));
            }
            serde_json::Value::Object(map)
        };
        let same_b = {
            let mut map = serde_json::Map::new();
            for key in reversed {
                map.insert(key.to_string(), json!(key.len()));
            }
            serde_json::Value::Object(map)
        };
        assert_eq!(canonical_json(&same_a), canonical_json(&same_b));
    }
}
