mod daemon_bridge;
mod runtime;

fn main() {
    runtime::run();
}

#[cfg(test)]
mod tests {
    use super::runtime;

    #[test]
    fn test_runtime_disabled_message_mentions_feature_flag() {
        let message = runtime::runtime_disabled_message();
        assert!(message.contains("experimental-runtime"));
    }

    #[test]
    fn test_runtime_disabled_message_mentions_impulse_supervisor() {
        let message = runtime::runtime_disabled_message();
        assert!(message.contains("impulse-supervisor"));
    }

    #[test]
    fn test_runtime_title_matches_package_name() {
        assert_eq!(runtime::runtime_bootstrap().title, "Impulse Supervisor");
    }

    #[test]
    fn test_run_is_callable_in_default_build() {
        runtime::run();
    }
}
