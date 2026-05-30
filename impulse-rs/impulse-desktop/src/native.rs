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
