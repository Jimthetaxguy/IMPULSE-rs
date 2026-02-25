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

/// Get impulse-specific environment variables
pub fn get_impulse_env_vars() -> Vec<EnvInfo> {
    get_env_vars("IMPULSE_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_info() {
        let info = SystemInfo::collect();
        println!("System info: {:?}", info);
    }

    #[test]
    fn test_get_impulse_env_vars() {
        let vars = get_impulse_env_vars();
        println!("Impulse env vars: {:?}", vars);
    }
}
