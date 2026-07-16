use crate::host_commands::HOST_INVOKE_COMMANDS;
use crate::runtime::DesktopEvent;
use crate::DesktopShutdownCoordinator;

pub const HOST_KIND: &str = "dioxus";
pub const HOST_BOOTSTRAP_STATUS: &str = crate::host_commands::PENDING_HOST_BOOTSTRAP_STATUS;
pub const HOST_EVENT_NAMES: &[&str] = DesktopEvent::HOST_EVENT_NAMES;
const DEFAULT_WINDOW_WIDTH: f64 = 1440.0;
const DEFAULT_WINDOW_HEIGHT: f64 = 900.0;
const MINIMUM_COCKPIT_WIDTH: f64 = 1180.0;
const MINIMUM_COCKPIT_HEIGHT: f64 = 720.0;

pub fn host_bootstrap_script() -> String {
    r#"
<script>
(() => {
  if (window.__IMPULSE_DESKTOP_HOST) {
    return;
  }
  const pending = (operation) => {
    return Promise.reject(
      new Error(`Dioxus Desktop host adapter pending: ${operation}`)
    );
  };
  window.__IMPULSE_DESKTOP_HOST = {
    invoke(command, payload) {
      return pending(`invoke:${command}`);
    },
    listen(event, handler) {
      return pending(`listen:${event}`);
    },
    hostKind: "__IMPULSE_HOST_KIND__",
    status: "__IMPULSE_HOST_STATUS__",
    supportedInvokes: __IMPULSE_HOST_INVOKES__,
    supportedEvents: __IMPULSE_HOST_EVENTS__,
  };
  // Mark the placeholder transport so the host-adapter resolver can tell these
  // always-rejecting stubs apart from a live eval bridge that later replaces
  // them. Without this flag the resolver would advertise the bridge as mounted
  // and then unhandled-reject on the first invoke/listen.
  window.__IMPULSE_DESKTOP_HOST.invoke.__impulseHostPending = true;
  window.__IMPULSE_DESKTOP_HOST.listen.__impulseHostPending = true;
  document.documentElement?.setAttribute("data-impulse-host-kind", "__IMPULSE_HOST_KIND__");
  document.documentElement?.setAttribute(
    "data-impulse-host-status",
    "__IMPULSE_HOST_STATUS__"
  );
})();
</script>
"#
    .replace("__IMPULSE_HOST_KIND__", HOST_KIND)
    .replace("__IMPULSE_HOST_STATUS__", HOST_BOOTSTRAP_STATUS)
    .replace(
        "__IMPULSE_HOST_INVOKES__",
        &javascript_string_array(HOST_INVOKE_COMMANDS),
    )
    .replace(
        "__IMPULSE_HOST_EVENTS__",
        &javascript_string_array(HOST_EVENT_NAMES),
    )
}

pub fn is_manifest_only_bootstrap() -> bool {
    HOST_BOOTSTRAP_STATUS == "manifest-only-pending-dioxus-eval-bridge"
}

pub fn desktop_config() -> dioxus_desktop::Config {
    let window = dioxus_desktop::WindowBuilder::new()
        .with_title("Impulse Desktop")
        .with_inner_size(dioxus_desktop::tao::dpi::LogicalSize::new(
            DEFAULT_WINDOW_WIDTH,
            DEFAULT_WINDOW_HEIGHT,
        ))
        .with_min_inner_size(dioxus_desktop::tao::dpi::LogicalSize::new(
            MINIMUM_COCKPIT_WIDTH,
            MINIMUM_COCKPIT_HEIGHT,
        ));
    dioxus_desktop::Config::new()
        .with_window(window)
        .with_custom_head(host_bootstrap_script())
}

/// Build the native desktop configuration with an explicit process lifecycle
/// boundary. Dioxus's event loop does not return, so final loop destruction
/// must drain managed workers and telemetry before reaping an owned companion.
pub fn desktop_config_with_shutdown(
    shutdown_coordinator: DesktopShutdownCoordinator,
) -> dioxus_desktop::Config {
    desktop_config().with_custom_event_handler(move |event, _| {
        if desktop_event_requests_shutdown(event) {
            shutdown_coordinator.shutdown();
        }
    })
}

fn desktop_event_requests_shutdown<T: 'static>(
    event: &dioxus_desktop::tao::event::Event<'_, T>,
) -> bool {
    matches!(event, dioxus_desktop::tao::event::Event::LoopDestroyed)
}

fn javascript_string_array(items: &[&str]) -> String {
    serde_json::json!(items).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_commands::{
        AGENT_FOCUS_COMMAND, AGENT_RESIZE_COMMAND, AGENT_SNAPSHOT_COMMAND, AGENT_WRITE_COMMAND,
        LIST_WORKSPACES_COMMAND, MCP_DESCRIPTORS_COMMAND, MCP_INVOKE_COMMAND,
        REGISTER_WORKSPACE_COMMAND, REVIEW_DECISION_COMMAND, REVIEW_QUEUE_COMMAND,
    };
    use std::collections::HashSet;

    #[test]
    fn host_bootstrap_installs_dioxus_host_adapter() {
        let script = host_bootstrap_script();

        assert!(script.contains("window.__IMPULSE_DESKTOP_HOST"));
        assert!(script.contains(&format!("hostKind: \"{HOST_KIND}\"")));
        assert!(script.contains(HOST_BOOTSTRAP_STATUS));
        assert!(script.contains("data-impulse-host-status"));
        assert!(script.contains("Dioxus Desktop host adapter pending"));
        assert!(script.contains("invoke(command, payload)"));
        assert!(script.contains("listen(event, handler)"));
        assert!(script.contains("supportedInvokes"));
        assert!(script.contains("supportedEvents"));
        assert!(is_manifest_only_bootstrap());
    }

    #[test]
    fn native_window_defaults_preserve_the_three_lane_cockpit() {
        assert!(DEFAULT_WINDOW_WIDTH > MINIMUM_COCKPIT_WIDTH);
        assert!(MINIMUM_COCKPIT_WIDTH > 1120.0);
        assert!(DEFAULT_WINDOW_HEIGHT >= MINIMUM_COCKPIT_HEIGHT);
    }

    #[test]
    fn host_bootstrap_flags_pending_stub_transport() {
        let script = host_bootstrap_script();

        // The resolver in ui.rs keys off `__impulseHostPending` to fail closed
        // when only the manifest-only stubs are installed; the bootstrap must
        // tag both transports or that gate silently stops working.
        assert!(
            script.contains("window.__IMPULSE_DESKTOP_HOST.invoke.__impulseHostPending = true;")
        );
        assert!(
            script.contains("window.__IMPULSE_DESKTOP_HOST.listen.__impulseHostPending = true;")
        );
        assert_eq!(
            HOST_BOOTSTRAP_STATUS,
            crate::host_commands::PENDING_HOST_BOOTSTRAP_STATUS
        );
    }

    #[test]
    fn host_manifest_declares_required_bridge_surface() {
        for command in [
            AGENT_FOCUS_COMMAND,
            AGENT_RESIZE_COMMAND,
            AGENT_SNAPSHOT_COMMAND,
            AGENT_WRITE_COMMAND,
            LIST_WORKSPACES_COMMAND,
            MCP_DESCRIPTORS_COMMAND,
            MCP_INVOKE_COMMAND,
            REGISTER_WORKSPACE_COMMAND,
            REVIEW_DECISION_COMMAND,
            REVIEW_QUEUE_COMMAND,
        ] {
            assert!(
                HOST_INVOKE_COMMANDS.contains(&command),
                "missing invoke command: {command}"
            );
        }

        for event in [
            "agent_runtime_update",
            "ops_connection_update",
            "ops_update",
            "supervisor_local_action",
            "terminal_exit",
            "terminal_output",
        ] {
            assert!(
                HOST_EVENT_NAMES.contains(&event),
                "missing event name: {event}"
            );
        }
    }

    #[test]
    fn only_event_loop_destruction_requests_process_shutdown() {
        let destroyed = dioxus_desktop::tao::event::Event::<()>::LoopDestroyed;
        let idle = dioxus_desktop::tao::event::Event::<()>::MainEventsCleared;

        assert!(desktop_event_requests_shutdown(&destroyed));
        assert!(!desktop_event_requests_shutdown(&idle));
    }

    #[test]
    fn host_manifest_has_unique_names() {
        assert_unique(HOST_INVOKE_COMMANDS);
        assert_unique(HOST_EVENT_NAMES);
    }

    #[test]
    fn host_bootstrap_includes_manifest_entries() {
        let script = host_bootstrap_script();

        for command in HOST_INVOKE_COMMANDS {
            assert!(
                script.contains(&format!(r#""{command}""#)),
                "bootstrap missing invoke command: {command}"
            );
        }

        for event in HOST_EVENT_NAMES {
            assert!(
                script.contains(&format!(r#""{event}""#)),
                "bootstrap missing event name: {event}"
            );
        }
    }

    #[test]
    fn host_manifest_serializes_names_as_json() {
        let serialized = javascript_string_array(&["plain", "quote\"inside"]);

        assert_eq!(serialized, r#"["plain","quote\"inside"]"#);
    }

    fn assert_unique(items: &[&str]) {
        let mut seen = HashSet::new();
        for item in items {
            assert!(seen.insert(item), "duplicate host manifest entry: {item}");
        }
    }
}
