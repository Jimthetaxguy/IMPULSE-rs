//! The egui panel keeps rendering after scrolling back as far as it can.
#![cfg(feature = "egui")]

use std::time::{Duration, Instant};

use eframe::egui;

fn run_frame(
    ctx: &egui::Context,
    panel: &mut impulse_term::TerminalPanel,
    events: Vec<egui::Event>,
) -> Result<(), String> {
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        events,
        ..Default::default()
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.show(ui));
        });
    }))
    .map_err(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_default()
    })
}

/// Verification finding: once `scrollback_len` reported the history, the
/// panel could scroll back a full screen; its scroll badge then shrank the
/// terminal by a row, and the stored offset, now past the height, made
/// vt100 0.15 underflow on every later frame.
#[test]
fn panel_scrolled_back_to_the_top_keeps_rendering() {
    let ctx = egui::Context::default();
    let mut panel = impulse_term::TerminalPanel::spawn(
        "sh",
        &["-c".to_string(), "seq 1 300; sleep 30".to_string()],
        None,
        "Shell",
        1,
    )
    .expect("spawn panel");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !panel.screen_text().contains("300") {
        std::thread::sleep(Duration::from_millis(20));
    }
    // The first frame sizes the PTY to the panel; let the shell redraw.
    run_frame(&ctx, &mut panel, vec![]).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    run_frame(&ctx, &mut panel, vec![]).unwrap();

    let wheel = egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -100_000.0),
        modifiers: egui::Modifiers::NONE,
    };
    let mut frames = vec![run_frame(&ctx, &mut panel, vec![wheel])];
    for _ in 0..4 {
        frames.push(run_frame(&ctx, &mut panel, vec![]));
    }
    panel.kill();
    assert!(frames.iter().all(Result::is_ok), "{frames:?}");
}
