use impulse_desktop::{desktop_host::desktop_config, DesktopShell};

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(desktop_config())
        .launch(DesktopShell);
}
