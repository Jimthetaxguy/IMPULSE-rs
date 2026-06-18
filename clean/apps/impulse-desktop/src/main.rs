//! Desktop / web entry point. The exact `main` is feature-gated.

#![cfg_attr(feature = "desktop", allow(dead_code))]

use dioxus::prelude::*;
use impulse_desktop::{components::ViewSwitcher, state::AppState, theme::STYLE};

fn app() -> Element {
    let state = use_signal(AppState::new);

    rsx! {
        style { "{STYLE}" }
        div { id: "app", ViewSwitcher { state } }
    }
}

/// Native desktop entry. Compiled only with the `desktop` feature.
#[cfg(feature = "desktop")]
fn main() {
    use dioxus_desktop::Config;
    let cfg = Config::new().with_window(
        dioxus_desktop::WindowBuilder::new()
            .with_title("Impulse-RS")
            .with_inner_size(dioxus_desktop::LogicalSize::new(1280.0, 800.0)),
    );
    dioxus_desktop::launch::launch(app, Vec::new(), vec![Box::new(cfg)]);
}

/// Web entry. Compiled only with the `web` feature.
#[cfg(feature = "web")]
fn main() {
    launch(app);
}

/// SSR entry. Compiled only with the `ssr` feature.
#[cfg(feature = "ssr")]
fn main() {
    use dioxus_ssr::prelude::*;
    let mut vdom = VirtualDom::new(app);
    let _ = vdom.rebuild();
    let _ = dioxus_ssr::render(&vdom);
}

/// Placeholder main for the default feature set (which is `web`).
/// Without an active feature we shouldn't try to launch anything.
#[cfg(not(any(feature = "desktop", feature = "web", feature = "ssr")))]
compile_error!("enable one of: desktop, web, ssr");
