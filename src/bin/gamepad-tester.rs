//! Native evidence viewer for gamepad-capture.
//!
//! Run with `cargo run --features tester --bin gamepad-tester`. It starts in a
//! safe replay-only state. Live capture remains a future explicit opt-in.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use eframe::egui;
use gamepad_capture::tester::{TesterSource, TesterState};
use gamepad_capture::{
    AbsoluteAxisInfo, AccessMode, CaptureAccess, CaptureEvent, CaptureSession, ControlDescriptor,
    ControllerClass, DeviceDescriptor, DeviceIdentity, DeviceProvenance, EventBatch,
    IdentityStability, NativeEvent, PhysicalDeviceId, ProfileId, SourceId, Transport,
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
        self.drain_events();
        Self::render_toolbar(ctx);
        self.render_sources(ctx);
        self.render_evidence(ctx);
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

fn render_control(ui: &mut egui::Ui, control: &ControlDescriptor) {
    match control {
        ControlDescriptor::Key { code, name } => ui.monospace(format!("key {code:#06x}: {name}")),
        ControlDescriptor::AbsoluteAxis { code, name, info } => ui.monospace(format!(
            "absolute {code:#06x}: {name}; {}..={}, current={}, fuzz={}, flat={}, resolution={}",
            info.minimum, info.maximum, info.current, info.fuzz, info.flat, info.resolution
        )),
        ControlDescriptor::RelativeAxis { code, name } => {
            ui.monospace(format!("relative {code:#06x}: {name}"))
        }
        ControlDescriptor::Switch { code, name } => {
            ui.monospace(format!("switch {code:#06x}: {name}"))
        }
        ControlDescriptor::Led { code, name } => {
            ui.monospace(format!("LED {code:#06x}: {name} (display only)"))
        }
        ControlDescriptor::ForceFeedback { code, name } => {
            ui.monospace(format!("force-feedback {code:#06x}: {name} (never sent)"))
        }
        _ => ui.monospace("unrecognized native control (display only)"),
    };
}

fn render_source(ui: &mut egui::Ui, id: &SourceId, source: &TesterSource, state: &mut TesterState) {
    ui.collapsing(format!("{id}: {}", source.device.reported_name), |ui| {
        ui.monospace(format!("access: {:?}", source.access));
        ui.monospace(format!("physical: {}", source.device.physical_id));
        ui.monospace(format!("transport: {:?}", source.device.transport));
        ui.monospace(format!("stability: {:?}", source.device.identity_stability));
        ui.monospace(format!(
            "identity: bus={:#06x}, vid={:#06x}, pid={:#06x}, version={:#06x}",
            source.device.identity.bus_type,
            source.device.identity.vendor_id,
            source.device.identity.product_id,
            source.device.identity.version,
        ));
        ui.label("The displayed identities are local diagnostic evidence and are not exported by fixture replay.");
        ui.separator();
        ui.label("Profile candidates and evidence");
        if let Some(forced) = source.profile.forced_profile() {
            ui.colored_label(egui::Color32::YELLOW, format!("forced preview: {forced:?}"));
            if ui.button("Return to automatic selection").clicked() {
                state.clear_forced_profile(id);
            }
        } else if ui.button("Preview forced capture profile").clicked() {
            state.force_profile(id, ProfileId::new("operator-preview"));
        }
        ui.monospace(format!(
            "automatic choice: {:?}",
            source.profile.automatic().selected.profile_id
        ));
        for candidate in &source.profile.automatic().candidates {
            ui.monospace(format!(
                "{:?} {:?}: {:?}",
                candidate.profile_id, candidate.confidence, candidate.evidence
            ));
        }
        ui.separator();
        ui.label("Native controls and advertised ranges");
        for control in &source.device.controls {
            render_control(ui, control);
        }
    });
}

fn render_axis_values(ui: &mut egui::Ui, state: &TesterState) {
    ui.separator();
    ui.heading("Raw axis values and advertised ranges");
    for ((source_id, event_type, code), value) in state.values() {
        let axis = state.sources().get(source_id).and_then(|source| {
            source
                .device
                .controls
                .iter()
                .find_map(|control| match control {
                    ControlDescriptor::AbsoluteAxis {
                        code: axis_code,
                        info,
                        ..
                    } if event_type == &3 && code == axis_code => Some(info),
                    _ => None,
                })
        });
        if let Some(info) = axis {
            ui.monospace(format!(
                "{source_id} ({event_type:#06x}, {code:#06x}): {value}  range {}..={}  fuzz={} flat={}",
                info.minimum, info.maximum, info.fuzz, info.flat
            ));
            ui.add(
                egui::ProgressBar::new(axis_marker(*value, info.minimum, info.maximum))
                    .text(format!("raw {value}; display marker only")),
            );
        }
    }
}

/// This lossy ratio is exclusively for drawing a progress bar; stored values stay native `i32`s.
#[allow(clippy::cast_precision_loss)]
fn axis_marker(value: i32, minimum: i32, maximum: i32) -> f32 {
    let span = (i64::from(maximum) - i64::from(minimum)).max(1) as f32;
    ((i64::from(value) - i64::from(minimum)) as f32 / span).clamp(0.0, 1.0)
}

fn render_frames(ui: &mut egui::Ui, state: &TesterState) {
    ui.separator();
    ui.heading("Complete raw event frames");
    egui::ScrollArea::vertical()
        .max_height(180.0)
        .show(ui, |ui| {
            for frame in state.frames().iter().rev() {
                ui.monospace(format!("{} frame {}", frame.source_id, frame.sequence));
                for event in &frame.events {
                    ui.monospace(format!(
                        "  ({:#06x}, {:#06x}) -> {} @ {:?}",
                        event.event_type, event.code, event.value, event.timestamp
                    ));
                }
            }
        });
}

fn render_lifecycle(ui: &mut egui::Ui, state: &TesterState) {
    ui.separator();
    ui.heading("Lifecycle and errors");
    if state.log().is_empty() {
        ui.weak("No events yet. Use a synthetic replay frame or explicitly start live capture.");
    } else {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for event in state.log() {
                ui.monospace(event);
            }
        });
    }
}

impl TesterApp {
    fn drain_events(&mut self) {
        if let Some(receiver) = &self.receiver {
            while let Ok(event) = receiver.try_recv() {
                self.state.apply(event);
            }
        }
    }

    fn render_toolbar(ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.heading("Gamepad Capture Tester");
            ui.label("Native evidence viewer — no normalization, routing, or output writes.");
        });
    }

    fn render_sources(&mut self, ctx: &egui::Context) {
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
            let sources = self
                .state
                .sources()
                .iter()
                .map(|(id, source)| (id.clone(), source.clone()))
                .collect::<Vec<_>>();
            for (id, source) in sources {
                render_source(ui, &id, &source, &mut self.state);
            }
        });
    }

    fn render_evidence(&self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Native event frames and values");
            ui.label("Values are raw `(event_type, code) -> value` evidence; no gamepad mapping is applied.");
            ui.separator();
            egui::Grid::new("raw-values").striped(true).show(ui, |ui| {
                ui.label("Source");
                ui.label("Type");
                ui.label("Code");
                ui.label("Value");
                ui.end_row();
                for ((source, event_type, code), value) in self.state.values() {
                    ui.monospace(source.as_str());
                    ui.monospace(format!("{event_type:#06x}"));
                    ui.monospace(format!("{code:#06x}"));
                    ui.monospace(value.to_string());
                    ui.end_row();
                }
            });
            render_axis_values(ui, &self.state);
            render_frames(ui, &self.state);
            render_lifecycle(ui, &self.state);
        });
    }

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
        let device = synthetic_device();
        self.state.apply(CaptureEvent::Connected {
            device: device.clone(),
            access: CaptureAccess::Shared,
        });
        self.state.apply(CaptureEvent::Input(EventBatch {
            source_id: device.source_id,
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

fn synthetic_device() -> DeviceDescriptor {
    DeviceDescriptor {
        physical_id: PhysicalDeviceId::new("synthetic-physical"),
        source_id: SourceId::new("synthetic-replay"),
        reported_name: "Synthetic replay controller".to_owned(),
        identity: DeviceIdentity {
            bus_type: 0x0003,
            vendor_id: 0,
            product_id: 0,
            version: 1,
        },
        transport: Transport::Virtual,
        provenance: DeviceProvenance::Unknown,
        identity_stability: IdentityStability::ConnectionOnly,
        class: ControllerClass::Gamepad,
        device_path: PathBuf::default(),
        physical_path: None,
        unique_id: None,
        controls: vec![
            ControlDescriptor::AbsoluteAxis {
                code: 0,
                name: "synthetic-x".to_owned(),
                info: AbsoluteAxisInfo {
                    minimum: -32_768,
                    maximum: 32_767,
                    current: 0,
                    fuzz: 0,
                    flat: 0,
                    resolution: 1,
                },
            },
            ControlDescriptor::Key {
                code: 304,
                name: "synthetic-button".to_owned(),
            },
        ],
    }
}

impl Drop for TesterApp {
    fn drop(&mut self) {
        self.stop_capture();
    }
}
