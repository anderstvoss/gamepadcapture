//! Native evidence viewer for gamepad-capture.
//!
//! Run with `cargo run --features tester --bin gamepad-tester`. It starts in a
//! safe replay-only state. Live capture remains a future explicit opt-in.

use eframe::egui;
use gamepad_capture::tester::TesterState;

fn main() -> eframe::Result {
    eframe::run_native(
        "Gamepad Capture Tester",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::<TesterApp>::default())),
    )
}

#[derive(Default)]
struct TesterApp {
    access: AccessChoice,
    state: TesterState,
}

#[derive(Default, PartialEq)]
enum AccessChoice {
    #[default]
    Shared,
    PreferExclusive,
    Exclusive,
}

impl eframe::App for TesterApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.heading("Gamepad Capture Tester");
            ui.label("Native evidence viewer — no normalization, routing, or output writes.");
        });
        egui::SidePanel::left("sources").show(ctx, |ui| {
            ui.heading("Capture mode");
            ui.radio_value(&mut self.access, AccessChoice::Shared, "Shared");
            ui.radio_value(
                &mut self.access,
                AccessChoice::PreferExclusive,
                "Prefer exclusive",
            );
            ui.radio_value(&mut self.access, AccessChoice::Exclusive, "Exclusive");
            ui.separator();
            ui.heading("Sources");
            ui.label("Replay mode: no source loaded");
            ui.small("Live evdev capture is intentionally not started by this tool yet.");
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Native event frames");
            ui.label("Load a sanitized replay fixture to inspect source lifecycle, raw event codes, values, and profile candidates.");
            ui.separator();
            if self.state.log().is_empty() {
                ui.weak("No events. This tester never synthesizes controller input.");
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for event in self.state.log() {
                        ui.monospace(event);
                    }
                });
            }
        });
    }
}
