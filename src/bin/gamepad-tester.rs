//! Native evidence viewer for gamepad-capture.
//!
//! Run with `cargo run --features tester --bin gamepad-tester`. It starts in a
//! safe replay-only state. Live capture remains a future explicit opt-in.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use eframe::egui;
use gamepad_capture::tester::TesterState;
use gamepad_capture::{
    AccessMode, CaptureEvent, CaptureSession, EventBatch, NativeEvent, SourceId,
};

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
    receiver: Option<mpsc::Receiver<CaptureEvent>>,
    stop: Option<Arc<AtomicBool>>,
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
        if let Some(receiver) = &self.receiver {
            while let Ok(event) = receiver.try_recv() {
                self.state.apply(event);
            }
        }
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
            if ui.button("Start live evdev capture").clicked() {
                self.start_live_capture();
            }
            if ui.button("Show synthetic replay frame").clicked() {
                self.synthetic_frame();
            }
            if ui.button("Stop capture").clicked() {
                self.stop_capture();
            }
            ui.separator();
            ui.heading("Sources");
            if self.state.sources().is_empty() {
                ui.label("No active source");
            }
            for (id, source) in self.state.sources() {
                ui.monospace(format!(
                    "{id}: {} ({:?})",
                    source.reported_name, source.transport
                ));
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Native event frames and values");
            ui.label("Values are raw `(event_type, code) -> value` evidence; no gamepad mapping is applied.");
            ui.separator();
            egui::Grid::new("raw-values").striped(true).show(ui, |ui| {
                ui.label("Source"); ui.label("Type"); ui.label("Code"); ui.label("Value"); ui.end_row();
                for ((source, event_type, code), value) in self.state.values() {
                    ui.monospace(source.as_str()); ui.monospace(format!("{event_type:#06x}")); ui.monospace(format!("{code:#06x}")); ui.monospace(value.to_string()); ui.end_row();
                }
            });
            ui.separator();
            ui.heading("Lifecycle and errors");
            if self.state.log().is_empty() {
                ui.weak("No events yet. Use a synthetic replay frame or explicitly start live capture.");
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for event in self.state.log() {
                        ui.monospace(event);
                    }
                });
            }
        });
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

impl TesterApp {
    fn access_mode(&self) -> AccessMode {
        match self.access {
            AccessChoice::Shared => AccessMode::Shared,
            AccessChoice::PreferExclusive => AccessMode::PreferExclusive,
            AccessChoice::Exclusive => AccessMode::Exclusive,
        }
    }

    fn start_live_capture(&mut self) {
        self.stop_capture();
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let mode = self.access_mode();
        thread::spawn(move || {
            let mut session =
                CaptureSession::new(gamepad_capture::linux::EvdevProvider::new(), mode);
            while !thread_stop.load(Ordering::Relaxed) {
                match session.poll() {
                    Ok(events) => {
                        for event in events {
                            if sender.send(event).is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(CaptureEvent::SourceError {
                            source_id: SourceId::new("discovery"),
                            error,
                        });
                    }
                }
                thread::sleep(Duration::from_millis(4));
            }
        });
        self.receiver = Some(receiver);
        self.stop = Some(stop);
    }

    fn stop_capture(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.receiver = None;
    }

    fn synthetic_frame(&mut self) {
        self.state.apply(CaptureEvent::Input(EventBatch {
            source_id: SourceId::new("synthetic-replay"),
            sequence: 1,
            events: vec![
                NativeEvent {
                    timestamp: std::time::SystemTime::UNIX_EPOCH,
                    event_type: 3,
                    code: 0,
                    value: -12,
                },
                NativeEvent {
                    timestamp: std::time::SystemTime::UNIX_EPOCH,
                    event_type: 1,
                    code: 304,
                    value: 1,
                },
            ],
        }));
    }
}

impl Drop for TesterApp {
    fn drop(&mut self) {
        self.stop_capture();
    }
}
