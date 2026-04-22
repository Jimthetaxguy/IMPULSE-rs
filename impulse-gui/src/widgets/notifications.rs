//! Toast-style notification system.
//!
//! `NotificationManager` collects notifications and renders them as stacked
//! toasts anchored to the top-right of the screen. Each toast auto-dismisses
//! after a configurable duration with a fade-out animation.

use std::time::Instant;

use eframe::egui;

use crate::theme::colors;

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

/// Notification severity — determines color and default duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    fn color(&self) -> egui::Color32 {
        match self {
            Severity::Info => colors::BLUE,
            Severity::Success => colors::GREEN,
            Severity::Warning => colors::YELLOW,
            Severity::Error => colors::RED,
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Severity::Info => "\u{2139}",    // ℹ
            Severity::Success => "\u{2713}", // ✓
            Severity::Warning => "\u{26A0}", // ⚠
            Severity::Error => "\u{2718}",   // ✘
        }
    }

    /// Default auto-dismiss duration in seconds.
    fn default_duration_secs(&self) -> f32 {
        match self {
            Severity::Info => 4.0,
            Severity::Success => 3.0,
            Severity::Warning => 6.0,
            Severity::Error => 8.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Notification
// ---------------------------------------------------------------------------

/// A single notification toast.
struct Notification {
    message: String,
    severity: Severity,
    created: Instant,
    /// Total display duration in seconds (including fade-out).
    duration_secs: f32,
    /// Whether the user has manually dismissed this notification.
    dismissed: bool,
}

/// Duration of the fade-out animation in seconds.
const FADE_DURATION: f32 = 0.5;

/// Maximum number of visible toasts at once.
const MAX_VISIBLE: usize = 5;

/// Toast width in logical pixels.
const TOAST_WIDTH: f32 = 320.0;

/// Vertical spacing between toasts.
const TOAST_SPACING: f32 = 6.0;

/// Top offset from the top of the screen.
const TOP_OFFSET: f32 = 40.0;

/// Right margin from the edge of the screen.
const RIGHT_MARGIN: f32 = 12.0;

// ---------------------------------------------------------------------------
// NotificationManager
// ---------------------------------------------------------------------------

/// Manages a queue of toast notifications rendered as overlays.
pub struct NotificationManager {
    notifications: Vec<Notification>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
        }
    }

    /// Push a notification with default duration for its severity.
    pub fn notify(&mut self, severity: Severity, message: impl Into<String>) {
        let severity_copy = severity;
        self.notifications.push(Notification {
            message: message.into(),
            severity,
            created: Instant::now(),
            duration_secs: severity_copy.default_duration_secs(),
            dismissed: false,
        });
    }

    /// Push a notification with a custom duration.
    // dead_code: retained for tests and future severity-specific UX overrides.
    #[allow(dead_code)]
    pub fn notify_with_duration(
        &mut self,
        severity: Severity,
        message: impl Into<String>,
        duration_secs: f32,
    ) {
        self.notifications.push(Notification {
            message: message.into(),
            severity,
            created: Instant::now(),
            duration_secs,
            dismissed: false,
        });
    }

    /// Number of currently active (not yet fully faded) notifications.
    // dead_code: retained for tests and future diagnostics/status surfaces.
    #[allow(dead_code)]
    pub fn count(&self) -> usize {
        self.notifications.len()
    }

    /// Render notifications as overlay toasts. Call this in `update()` after
    /// all panels are laid out, so toasts float above content.
    pub fn show(&mut self, ctx: &egui::Context) {
        let now = Instant::now();

        // Remove fully expired or dismissed notifications.
        self.notifications.retain(|n| {
            if n.dismissed {
                return false;
            }
            let elapsed = now.duration_since(n.created).as_secs_f32();
            elapsed < n.duration_secs + FADE_DURATION
        });

        if self.notifications.is_empty() {
            return;
        }

        // Request repaint while animating.
        ctx.request_repaint();

        let screen = ctx.screen_rect();
        let anchor_x = screen.max.x - RIGHT_MARGIN - TOAST_WIDTH;
        let mut y = screen.min.y + TOP_OFFSET;

        // Show at most MAX_VISIBLE, starting from the newest.
        let start = self.notifications.len().saturating_sub(MAX_VISIBLE);
        let mut dismiss_idx: Option<usize> = None;

        for (vis_idx, notification) in self.notifications[start..].iter().enumerate() {
            let elapsed = now.duration_since(notification.created).as_secs_f32();

            // Calculate opacity: full during display, fade during last FADE_DURATION.
            let opacity = if elapsed < notification.duration_secs {
                1.0_f32
            } else {
                let fade_elapsed = elapsed - notification.duration_secs;
                (1.0 - fade_elapsed / FADE_DURATION).clamp(0.0, 1.0)
            };

            // Slide-in from right during first 0.15s.
            let slide_progress = (elapsed / 0.15).min(1.0);
            let offset_x = (1.0 - slide_progress) * 60.0;

            let area_id = egui::Id::new("toast").with(start + vis_idx);
            let resp = egui::Area::new(area_id)
                .fixed_pos(egui::pos2(anchor_x + offset_x, y))
                .order(egui::Order::Foreground)
                .interactable(true)
                .show(ctx, |ui| {
                    let accent = notification.severity.color();
                    let alpha = (opacity * 255.0) as u8;

                    let bg = egui::Color32::from_rgba_unmultiplied(
                        colors::SURFACE.r(),
                        colors::SURFACE.g(),
                        colors::SURFACE.b(),
                        alpha,
                    );
                    let border = egui::Color32::from_rgba_unmultiplied(
                        accent.r(),
                        accent.g(),
                        accent.b(),
                        (opacity * 180.0) as u8,
                    );
                    let text_color = egui::Color32::from_rgba_unmultiplied(
                        colors::TEXT.r(),
                        colors::TEXT.g(),
                        colors::TEXT.b(),
                        alpha,
                    );
                    let icon_color = egui::Color32::from_rgba_unmultiplied(
                        accent.r(),
                        accent.g(),
                        accent.b(),
                        alpha,
                    );

                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .stroke(egui::Stroke::new(1.0, border))
                        .show(ui, |ui| {
                            ui.set_width(TOAST_WIDTH - 20.0);
                            ui.horizontal(|ui| {
                                // Icon.
                                ui.label(
                                    egui::RichText::new(notification.severity.icon())
                                        .color(icon_color),
                                );
                                // Message (wrapping).
                                ui.label(
                                    egui::RichText::new(&notification.message)
                                        .small()
                                        .color(text_color),
                                );
                            });
                        });
                });

            // Click to dismiss.
            if resp.response.clicked() {
                dismiss_idx = Some(start + vis_idx);
            }

            y += resp.response.rect.height() + TOAST_SPACING;
        }

        if let Some(idx) = dismiss_idx {
            if idx < self.notifications.len() {
                self.notifications[idx].dismissed = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_is_empty() {
        let mgr = NotificationManager::new();
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_notify_adds_notification() {
        let mut mgr = NotificationManager::new();
        mgr.notify(Severity::Info, "Hello");
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_notify_multiple_severities() {
        let mut mgr = NotificationManager::new();
        mgr.notify(Severity::Info, "info");
        mgr.notify(Severity::Success, "success");
        mgr.notify(Severity::Warning, "warning");
        mgr.notify(Severity::Error, "error");
        assert_eq!(mgr.count(), 4);
    }

    #[test]
    fn test_notify_with_custom_duration() {
        let mut mgr = NotificationManager::new();
        mgr.notify_with_duration(Severity::Info, "custom", 10.0);
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_severity_colors_are_distinct() {
        let severities = [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ];
        for i in 0..severities.len() {
            for j in (i + 1)..severities.len() {
                assert_ne!(
                    severities[i].color(),
                    severities[j].color(),
                    "{:?} and {:?} should have different colors",
                    severities[i],
                    severities[j]
                );
            }
        }
    }

    #[test]
    fn test_severity_icons_are_nonempty() {
        for sev in [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ] {
            assert!(!sev.icon().is_empty(), "{:?} has empty icon", sev);
        }
    }

    #[test]
    fn test_default_durations_are_positive() {
        for sev in [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ] {
            assert!(
                sev.default_duration_secs() > 0.0,
                "{:?} has non-positive duration",
                sev
            );
        }
    }

    #[test]
    fn test_error_duration_longer_than_info() {
        assert!(Severity::Error.default_duration_secs() > Severity::Info.default_duration_secs());
    }
}
