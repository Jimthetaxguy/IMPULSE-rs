use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::bridge::{empty_payload, DesktopBridgeError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeIslandKind {
    AppKitProbe,
    MenuBar,
    GlobalShortcut,
    FileOpenPanel,
    Notification,
    FloatingPanel,
    AccessibilityHook,
}

impl NativeIslandKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AppKitProbe => "appkit_probe",
            Self::MenuBar => "menu_bar",
            Self::GlobalShortcut => "global_shortcut",
            Self::FileOpenPanel => "file_open_panel",
            Self::Notification => "notification",
            Self::FloatingPanel => "floating_panel",
            Self::AccessibilityHook => "accessibility_hook",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeIslandRequest {
    pub request_id: String,
    pub kind: NativeIslandKind,
    #[serde(default = "empty_payload")]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NativeIslandResult {
    pub request_id: String,
    pub kind: NativeIslandKind,
    pub handled: bool,
    #[serde(default = "empty_payload")]
    pub payload: Value,
}

pub trait NativeIslandHost {
    fn dispatch(
        &self,
        request: NativeIslandRequest,
    ) -> Result<NativeIslandResult, DesktopBridgeError>;
}

#[derive(Debug, Default)]
pub struct DefaultNativeIslandHost;

impl NativeIslandHost for DefaultNativeIslandHost {
    fn dispatch(
        &self,
        request: NativeIslandRequest,
    ) -> Result<NativeIslandResult, DesktopBridgeError> {
        match request.kind {
            NativeIslandKind::AppKitProbe => probe_appkit(request),
            NativeIslandKind::FileOpenPanel => dispatch_file_open_panel(request),
            kind => Err(DesktopBridgeError::UnsupportedNativeIsland {
                kind: kind.as_str().to_string(),
            }),
        }
    }
}

#[cfg(all(target_os = "macos", feature = "native-macos"))]
fn probe_appkit(request: NativeIslandRequest) -> Result<NativeIslandResult, DesktopBridgeError> {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{MainThreadMarker, NSString};

    let label = NSString::from_str("Impulse Native Island").to_string();
    let main_thread_available = MainThreadMarker::new()
        .map(|mtm| {
            // The AppKit touch stays intentionally tiny: the shell only proves that
            // an AppKit-backed island can be reached. It does not start an app
            // lifecycle or duplicate Dioxus-owned UI state.
            let _app = NSApplication::sharedApplication(mtm);
            true
        })
        .unwrap_or(false);

    Ok(NativeIslandResult {
        request_id: request.request_id,
        kind: NativeIslandKind::AppKitProbe,
        handled: true,
        payload: json!({
            "bridge": "objc2",
            "framework": "AppKit",
            "label": label,
            "main_thread_available": main_thread_available,
            "state_owner": "dioxus"
        }),
    })
}

#[cfg(not(all(target_os = "macos", feature = "native-macos")))]
fn probe_appkit(request: NativeIslandRequest) -> Result<NativeIslandResult, DesktopBridgeError> {
    Ok(NativeIslandResult {
        request_id: request.request_id,
        kind: NativeIslandKind::AppKitProbe,
        handled: false,
        payload: json!({
            "bridge": "objc2",
            "framework": "AppKit",
            "reason": "native-macos feature or macOS target not enabled",
            "state_owner": "dioxus"
        }),
    })
}

/// Abstraction over "ask the OS for a folder path." Exists so
/// [`file_open_panel_with`] can be exercised deterministically in tests
/// without a real display — [`RfdFolderPicker`] is the only implementation
/// that ever touches the OS.
pub trait FolderPicker {
    /// `Ok(Some(path))` — the user picked a folder.
    /// `Ok(None)` — the user cancelled the dialog. This is a normal outcome,
    /// never an error.
    /// `Err(_)` — the picker itself failed (e.g. no display available).
    fn pick_folder(
        &self,
        title: &str,
        starting_directory: Option<&str>,
    ) -> Result<Option<PathBuf>, DesktopBridgeError>;
}

/// Native folder picker backed by `rfd`. The synchronous `FileDialog`
/// (not `AsyncFileDialog`) is used deliberately: rfd's own docs confirm
/// dialogs may run "from any thread ... in a windowed app" (Dioxus Desktop
/// is one), so the blocking call is safe as long as the *caller* keeps it
/// off the single-consumer host-invoke FIFO thread — see
/// `host_commands::native_island_request`, which wraps this dispatch in
/// `tokio::task::spawn_blocking` for exactly that reason.
#[cfg(feature = "desktop-app")]
pub struct RfdFolderPicker;

#[cfg(feature = "desktop-app")]
impl FolderPicker for RfdFolderPicker {
    fn pick_folder(
        &self,
        title: &str,
        starting_directory: Option<&str>,
    ) -> Result<Option<PathBuf>, DesktopBridgeError> {
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(directory) = starting_directory {
            dialog = dialog.set_directory(directory);
        }
        // `pick_folder` is infallible in rfd's own API (`Option<PathBuf>`,
        // no `Result`); `None` means "user cancelled," not an error.
        Ok(dialog.pick_folder())
    }
}

/// Pure dispatch logic for [`NativeIslandKind::FileOpenPanel`], independent
/// of which [`FolderPicker`] answers it. Never panics: a cancelled dialog is
/// a normal `Ok` result (`handled: true`, `payload.cancelled: true`,
/// `payload.path: null`), not an error — only a genuine picker failure
/// propagates as `Err`.
///
/// Called from the `desktop-app` dispatch path and from unit tests with a
/// fake picker. Integration-test builds of the lib (no `cfg(test)`, no
/// `desktop-app`) legitimately leave this unused, so silence that lint.
#[allow(dead_code)]
fn file_open_panel_with(
    request: NativeIslandRequest,
    picker: &dyn FolderPicker,
) -> Result<NativeIslandResult, DesktopBridgeError> {
    let title = request
        .payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Select a folder");
    let starting_directory = request
        .payload
        .get("starting_directory")
        .and_then(Value::as_str);

    let selection = picker.pick_folder(title, starting_directory)?;

    let payload = match &selection {
        Some(path) => json!({
            "path": path.to_string_lossy(),
            "cancelled": false,
        }),
        None => json!({
            "path": Value::Null,
            "cancelled": true,
        }),
    };

    Ok(NativeIslandResult {
        request_id: request.request_id,
        kind: NativeIslandKind::FileOpenPanel,
        handled: true,
        payload,
    })
}

#[cfg(feature = "desktop-app")]
fn dispatch_file_open_panel(
    request: NativeIslandRequest,
) -> Result<NativeIslandResult, DesktopBridgeError> {
    file_open_panel_with(request, &RfdFolderPicker)
}

#[cfg(not(feature = "desktop-app"))]
fn dispatch_file_open_panel(
    request: NativeIslandRequest,
) -> Result<NativeIslandResult, DesktopBridgeError> {
    Ok(NativeIslandResult {
        request_id: request.request_id,
        kind: NativeIslandKind::FileOpenPanel,
        handled: false,
        payload: json!({
            "reason": "desktop-app feature not enabled",
            "path": Value::Null,
            "cancelled": false,
        }),
    })
}

#[cfg(test)]
mod file_open_panel_tests {
    use super::*;

    enum FakeOutcome {
        Picked(PathBuf),
        Cancelled,
        Failed(String),
    }

    struct FakePicker {
        outcome: FakeOutcome,
        seen: std::cell::RefCell<Option<(String, Option<String>)>>,
    }

    impl FakePicker {
        fn new(outcome: FakeOutcome) -> Self {
            Self {
                outcome,
                seen: std::cell::RefCell::new(None),
            }
        }
    }

    impl FolderPicker for FakePicker {
        fn pick_folder(
            &self,
            title: &str,
            starting_directory: Option<&str>,
        ) -> Result<Option<PathBuf>, DesktopBridgeError> {
            *self.seen.borrow_mut() =
                Some((title.to_string(), starting_directory.map(str::to_string)));
            match &self.outcome {
                FakeOutcome::Picked(path) => Ok(Some(path.clone())),
                FakeOutcome::Cancelled => Ok(None),
                FakeOutcome::Failed(message) => Err(DesktopBridgeError::NativeIslandFailed {
                    message: message.clone(),
                }),
            }
        }
    }

    fn request(payload: Value) -> NativeIslandRequest {
        NativeIslandRequest {
            request_id: "req-1".to_string(),
            kind: NativeIslandKind::FileOpenPanel,
            payload,
        }
    }

    #[test]
    fn test_file_open_panel_with_returns_selected_path_on_pick() {
        let picker = FakePicker::new(FakeOutcome::Picked(PathBuf::from("/tmp/project")));
        let result = file_open_panel_with(request(empty_payload()), &picker).unwrap();

        assert!(result.handled);
        assert_eq!(result.kind, NativeIslandKind::FileOpenPanel);
        assert_eq!(result.payload["path"], "/tmp/project");
        assert_eq!(result.payload["cancelled"], false);
    }

    #[test]
    fn test_file_open_panel_with_returns_cancelled_when_user_cancels() {
        let picker = FakePicker::new(FakeOutcome::Cancelled);
        let result = file_open_panel_with(request(empty_payload()), &picker).unwrap();

        assert!(result.handled);
        assert!(result.payload["path"].is_null());
        assert_eq!(result.payload["cancelled"], true);
    }

    #[test]
    fn test_file_open_panel_with_propagates_picker_error() {
        let picker = FakePicker::new(FakeOutcome::Failed("no display".to_string()));
        let error = file_open_panel_with(request(empty_payload()), &picker).unwrap_err();

        assert_eq!(
            error,
            DesktopBridgeError::NativeIslandFailed {
                message: "no display".to_string()
            }
        );
    }

    #[test]
    fn test_file_open_panel_with_uses_default_title_when_payload_empty() {
        let picker = FakePicker::new(FakeOutcome::Cancelled);
        file_open_panel_with(request(empty_payload()), &picker).unwrap();

        let (title, starting_directory) = picker.seen.borrow().clone().unwrap();
        assert_eq!(title, "Select a folder");
        assert_eq!(starting_directory, None);
    }

    #[test]
    fn test_file_open_panel_with_forwards_custom_title_and_starting_directory() {
        let picker = FakePicker::new(FakeOutcome::Cancelled);
        let payload = json!({ "title": "Select a project folder", "starting_directory": "/tmp" });
        file_open_panel_with(request(payload), &picker).unwrap();

        let (title, starting_directory) = picker.seen.borrow().clone().unwrap();
        assert_eq!(title, "Select a project folder");
        assert_eq!(starting_directory, Some("/tmp".to_string()));
    }

    #[test]
    fn test_dispatch_reports_unhandled_without_desktop_app_feature() {
        // Exercises the cfg(not(feature = "desktop-app")) branch, which is
        // what a bare `cargo test --workspace` compiles by default.
        let result = DefaultNativeIslandHost
            .dispatch(request(empty_payload()))
            .unwrap();
        #[cfg(not(feature = "desktop-app"))]
        {
            assert!(!result.handled);
            assert_eq!(result.kind, NativeIslandKind::FileOpenPanel);
        }
        #[cfg(feature = "desktop-app")]
        {
            let _ = result; // real picker path — see manual verification in Step 10
        }
    }

    #[test]
    #[ignore = "opens a real native folder-picker dialog; run manually with a live display: `cargo test --features desktop-app -- --ignored`"]
    #[cfg(feature = "desktop-app")]
    fn manual_pick_folder_opens_native_dialog() {
        let result = RfdFolderPicker.pick_folder("Select a project folder", None);
        assert!(result.is_ok());
    }
}
