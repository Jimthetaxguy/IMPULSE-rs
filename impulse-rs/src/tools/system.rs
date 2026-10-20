// System information module - provides system and environment information

use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub home_dir: Option<String>,
    pub current_dir: String,
    pub python_available: bool,
    pub python_version: Option<String>,
}

impl SystemInfo {
    pub fn collect() -> Self {
        use crate::tools::python;

        let os = env::consts::OS.to_string();
        let arch = env::consts::ARCH.to_string();
        let home_dir = env::var("HOME").ok();
        let current_dir = env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let python_available = python::is_python_available();
        let python_version = python::get_python_version();

        Self {
            os,
            arch,
            home_dir,
            current_dir,
            python_available,
            python_version,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvInfo {
    pub key: String,
    pub value: String,
}

/// Get all environment variables starting with a prefix
pub fn get_env_vars(prefix: &str) -> Vec<EnvInfo> {
    env::vars()
        .filter(|(key, _)| key.starts_with(prefix))
        .map(|(key, value)| EnvInfo { key, value })
        .collect()
}

/// `IMPULSE_*` variables whose values are shown. They say where and how
/// Impulse is running; none grants anything.
const SHOWN_IMPULSE_VARS: &[&str] = &[
    "IMPULSE_HOME",
    "IMPULSE_SESSION_ID",
    "IMPULSE_SOCKET_PATH",
    "IMPULSE_MODEL",
    "IMPULSE_PROVIDER",
    "IMPULSE_PROJECT_ID",
    "IMPULSE_GOVERNED_TASK_ID",
    "IMPULSE_PANE_ID",
    "IMPULSE_CAPABILITIES_PATH",
    "IMPULSE_CONTROL_CLI",
];

/// What stands in for a value that is not shown.
const WITHHELD: &str = "(withheld)";

/// Impulse-specific environment variables, for `system_info`.
///
/// Only the variables in [`SHOWN_IMPULSE_VARS`] keep their values; any other
/// `IMPULSE_*` name is listed with its value withheld. `system_info` reaches
/// voice and MCP callers, and an `IMPULSE_*` variable can hold a secret: the
/// operator capability token (`IMPULSE_OPERATOR_CAPABILITY`) was handed to
/// any of them that asked.
pub fn get_impulse_env_vars() -> Vec<EnvInfo> {
    shown_impulse_vars(get_env_vars("IMPULSE_"))
}

/// [`get_impulse_env_vars`]'s filter, over any list of variables.
fn shown_impulse_vars(vars: Vec<EnvInfo>) -> Vec<EnvInfo> {
    vars.into_iter()
        .map(|EnvInfo { key, value }| {
            let value = if SHOWN_IMPULSE_VARS.contains(&key.as_str()) {
                value
            } else {
                WITHHELD.to_string()
            };
            EnvInfo { key, value }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_info_has_valid_fields() {
        let info = SystemInfo::collect();
        assert!(!info.os.is_empty(), "OS should not be empty");
        assert!(!info.arch.is_empty(), "arch should not be empty");
        assert!(
            !info.current_dir.is_empty(),
            "current_dir should not be empty"
        );
        // os must be a known target OS
        assert!(
            ["linux", "macos", "windows", "freebsd"]
                .iter()
                .any(|&os| info.os == os),
            "unexpected OS: {}",
            info.os
        );
    }

    #[test]
    fn test_get_impulse_env_vars_returns_only_impulse_prefix() {
        // Set a known env var for deterministic testing
        std::env::set_var("IMPULSE_TEST_ROUND_TRIP", "1");
        let vars = get_impulse_env_vars();
        for var in &vars {
            assert!(
                var.key.starts_with("IMPULSE_"),
                "expected IMPULSE_ prefix, got: {}",
                var.key
            );
        }
        assert!(
            vars.iter().any(|v| v.key == "IMPULSE_TEST_ROUND_TRIP"),
            "expected to find IMPULSE_TEST_ROUND_TRIP"
        );
        std::env::remove_var("IMPULSE_TEST_ROUND_TRIP");
    }

    #[test]
    fn test_system_info_round_trip() {
        let info = SystemInfo::collect();
        let json = serde_json::to_string(&info).unwrap();
        let recovered: SystemInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.os, recovered.os);
        assert_eq!(info.arch, recovered.arch);
        assert_eq!(info.current_dir, recovered.current_dir);
        assert_eq!(info.python_available, recovered.python_available);
    }

    /// Review finding: `system_info` (reachable from voice and MCP) returned
    /// every `IMPULSE_*` value, the operator capability token included.
    #[test]
    fn test_impulse_vars_show_only_informational_values() {
        let vars = vec![
            EnvInfo {
                key: "IMPULSE_HOME".to_string(),
                value: "/tmp/impulse".to_string(),
            },
            EnvInfo {
                key: "IMPULSE_OPERATOR_CAPABILITY".to_string(),
                value: "secret-token".to_string(),
            },
            EnvInfo {
                key: "IMPULSE_SOMETHING_NEW".to_string(),
                value: "unknown".to_string(),
            },
        ];
        let shown = shown_impulse_vars(vars);
        let value_of = |key: &str| {
            shown
                .iter()
                .find(|var| var.key == key)
                .map(|var| var.value.as_str())
        };
        assert_eq!(value_of("IMPULSE_HOME"), Some("/tmp/impulse"));
        assert_eq!(value_of("IMPULSE_OPERATOR_CAPABILITY"), Some(WITHHELD));
        assert_eq!(value_of("IMPULSE_SOMETHING_NEW"), Some(WITHHELD));
        assert!(!format!("{shown:?}").contains("secret-token"));
    }
}
