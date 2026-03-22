//! Delegation tracking for coordinator/worker agent patterns.
//!
//! IMPULSE detects delegation patterns in agent output and tracks their lifecycle.
//! It does NOT spawn or manage sub-agents — it observes and records.
//!
//! ## Design Sources
//! - **OpenSquirrel**: JSON code-fence delegation format (```delegate blocks)
//! - **Hermes Agent**: Depth-limited delegation with restricted child toolsets

pub mod detector;
pub mod tracker;
pub mod types;

pub use detector::{detect_delegation, detect_delegation_natural};
pub use tracker::DelegationTracker;
pub use types::{DelegationSpec, DelegationState, TrackedDelegation, MAX_DELEGATION_DEPTH};
