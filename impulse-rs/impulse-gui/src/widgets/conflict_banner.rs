use eframe::egui;

use crate::theme::colors;

/// Renders a persistent warning banner indicating file edge/overwrite conflicts
/// between the current pane and other active panes.
pub fn show(ui: &mut egui::Ui, conflicts: &[(String, String)]) -> Option<egui::Response> {
    if conflicts.is_empty() {
        return None;
    }

    let mut response = None;

    // Use a distinguishable warning frame
    let frame = egui::Frame::NONE
        .fill(egui::Color32::from_rgba_unmultiplied(
            colors::RED.r(),
            colors::RED.g(),
            colors::RED.b(),
            20,
        ))
        .stroke(egui::Stroke::new(1.0, colors::RED))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .corner_radius(egui::CornerRadius::same(6));

    frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            // High-contrast warning icon
            ui.label(
                egui::RichText::new("\u{26A0}") // Warning icon
                    .size(16.0)
                    .color(colors::RED),
            );

            ui.add_space(8.0);

            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("File Conflict Detected")
                        .strong()
                        .color(colors::TEXT),
                );

                for (file, other_tab) in conflicts {
                    ui.label(
                        egui::RichText::new(format!(
                            "`{}` is also being edited by {}",
                            file, other_tab
                        ))
                        .small()
                        .color(colors::TEXT_MUTED),
                    );
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Button to acknowledge or act upon the conflict
                let btn = ui.button(egui::RichText::new("Review").color(colors::TEXT));
                response = Some(btn);
            });
        });
    });

    response
}
