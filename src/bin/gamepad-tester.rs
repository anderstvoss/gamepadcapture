//! Native evidence viewer for gamepad-capture.
//!
//! Run with `cargo run --features tester --bin gamepad-tester`. It starts in a
//! safe replay-only state. Live capture remains a future explicit opt-in.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use gamepad_capture::hid::HidFixture;
use gamepad_capture::tester::{
    NativeInputView, TesterSource, TesterState, XboxDisplayButton, XboxDisplayView,
};
use gamepad_capture::{
    AbsoluteAxisInfo, AccessMode, CaptureAccess, CaptureEvent, CaptureSession, ControlDescriptor,
    ControllerClass, DeviceDescriptor, DeviceIdentity, DeviceProvenance, EventBatch,
    IdentityStability, NativeEvent, PhysicalDeviceId, ProfileId, SourceId, Transport,
};

const CAPTURE_POLL_INTERVAL: Duration = Duration::from_millis(8);
const UI_REPAINT_INTERVAL: Duration = Duration::from_millis(16);
const MAX_EVENTS_PER_UPDATE: usize = 512;
const MAX_PENDING_CAPTURE_EVENTS: usize = 512;
const MAX_RENDERED_FRAMES: usize = 64;
const MAX_RENDERED_LOG_ENTRIES: usize = 128;

fn main() -> eframe::Result {
    eframe::run_native(
        "Gamepad Capture Tester",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1100.0, 760.0])
                .with_min_inner_size([760.0, 520.0]),
            // This passive diagnostics window has no animation. Keep the compositor
            // paced instead of continuously submitting frames while idle.
            vsync: true,
            // A standalone tester does not need eframe's run-on-demand event-loop
            // compatibility mode. The normal loop has a simpler shutdown path.
            run_and_return: false,
            ..Default::default()
        },
        Box::new(|_| Ok(Box::<TesterApp>::default())),
    )
}

#[derive(Default)]
struct TesterApp {
    access: AccessChoice,
    surface: VisualizerSurface,
    state: TesterState,
    receiver: Option<mpsc::Receiver<CaptureEvent>>,
    stop: Option<Arc<AtomicBool>>,
}

#[derive(Default, PartialEq)]
enum VisualizerSurface {
    #[default]
    Native,
    XboxDemo,
    Evidence,
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
        let more_events_ready = self.drain_events();
        Self::render_toolbar(ctx);
        self.render_sources(ctx);
        self.render_evidence(ctx);
        if more_events_ready {
            ctx.request_repaint();
        }
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

fn render_dpad(ui: &mut egui::Ui, dpad: Option<(i32, i32)>) {
    let (x, y) = dpad.unwrap_or_default();
    ui.label("Native D-pad / hat values");
    egui::Grid::new("native-dpad").show(ui, |ui| {
        ui.label("");
        ui.colored_label(
            if y < 0 {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::GRAY
            },
            "↑",
        );
        ui.end_row();
        ui.colored_label(
            if x < 0 {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::GRAY
            },
            "←",
        );
        ui.monospace(format!("{x}, {y}"));
        ui.colored_label(
            if x > 0 {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::GRAY
            },
            "→",
        );
        ui.end_row();
        ui.label("");
        ui.colored_label(
            if y > 0 {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::GRAY
            },
            "↓",
        );
        ui.end_row();
    });
}

fn render_native_dashboard(ui: &mut egui::Ui, view: Option<NativeInputView>) {
    ui.heading("Native input dashboard");
    ui.label("Raw evdev codes and values; this view does not apply a gamepad mapping.");
    let Some(view) = view else {
        ui.weak("Select a live or replayed source to inspect native controls.");
        return;
    };
    ui.monospace(format!(
        "{}; {} complete frame(s)",
        view.source_id, view.recent_frame_count
    ));
    ui.separator();
    ui.label("Keys");
    ui.horizontal_wrapped(|ui| {
        for key in &view.keys {
            let color = if key.value != 0 {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::DARK_GRAY
            };
            ui.colored_label(
                color,
                format!("{} ({:#06x}) = {}", key.name, key.code, key.value),
            );
        }
    });
    ui.separator();
    render_dpad(ui, view.dpad);
    ui.separator();
    ui.label("Absolute axes");
    for axis in &view.axes {
        ui.monospace(format!(
            "{} ({:#06x}): {}  range {}..={}",
            axis.name, axis.code, axis.value, axis.info.minimum, axis.info.maximum
        ));
        ui.add(
            egui::ProgressBar::new(axis_marker(
                axis.value,
                axis.info.minimum,
                axis.info.maximum,
            ))
            .text("raw display marker"),
        );
    }
}

fn xbox_button_label(button: XboxDisplayButton) -> &'static str {
    match button {
        XboxDisplayButton::South => "A",
        XboxDisplayButton::East => "B",
        XboxDisplayButton::North => "Y",
        XboxDisplayButton::West => "X",
        XboxDisplayButton::LeftBumper => "LB",
        XboxDisplayButton::RightBumper => "RB",
        XboxDisplayButton::View => "View",
        XboxDisplayButton::Menu => "Menu",
        XboxDisplayButton::Guide => "Guide",
        XboxDisplayButton::LeftStick => "LS",
        XboxDisplayButton::RightStick => "RS",
    }
}

fn render_xbox_button(ui: &mut egui::Ui, button: XboxDisplayButton, value: i32) {
    let color = if value != 0 {
        egui::Color32::LIGHT_GREEN
    } else {
        egui::Color32::DARK_GRAY
    };
    ui.colored_label(color, format!("[{}]", xbox_button_label(button)));
}

fn render_xbox_demo(ui: &mut egui::Ui, view: Option<XboxDisplayView>) {
    ui.heading("Xbox-layout display demo");
    ui.label(
        "Presentation-only view from a compatible Linux code shape — not a profile or decoder.",
    );
    let Some(view) = view else {
        ui.weak("The selected source does not expose the required Xbox-compatible code shape.");
        return;
    };
    let value = |button| {
        view.buttons
            .iter()
            .find(|(known, _)| *known == button)
            .map_or(0, |(_, value)| *value)
    };
    ui.horizontal(|ui| {
        render_xbox_button(
            ui,
            XboxDisplayButton::LeftBumper,
            value(XboxDisplayButton::LeftBumper),
        );
        ui.add_space(120.0);
        render_xbox_button(
            ui,
            XboxDisplayButton::RightBumper,
            value(XboxDisplayButton::RightBumper),
        );
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            render_dpad(ui, Some(view.dpad));
            ui.monospace(format!(
                "LS raw: {}, {}",
                view.left_stick.0, view.left_stick.1
            ));
            render_xbox_button(
                ui,
                XboxDisplayButton::LeftStick,
                value(XboxDisplayButton::LeftStick),
            );
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            render_xbox_button(ui, XboxDisplayButton::View, value(XboxDisplayButton::View));
            render_xbox_button(
                ui,
                XboxDisplayButton::Guide,
                value(XboxDisplayButton::Guide),
            );
            render_xbox_button(ui, XboxDisplayButton::Menu, value(XboxDisplayButton::Menu));
        });
        ui.add_space(24.0);
        ui.vertical(|ui| {
            render_xbox_button(
                ui,
                XboxDisplayButton::North,
                value(XboxDisplayButton::North),
            );
            ui.horizontal(|ui| {
                render_xbox_button(ui, XboxDisplayButton::West, value(XboxDisplayButton::West));
                render_xbox_button(ui, XboxDisplayButton::East, value(XboxDisplayButton::East));
            });
            render_xbox_button(
                ui,
                XboxDisplayButton::South,
                value(XboxDisplayButton::South),
            );
            ui.monospace(format!(
                "RS raw: {}, {}",
                view.right_stick.0, view.right_stick.1
            ));
            render_xbox_button(
                ui,
                XboxDisplayButton::RightStick,
                value(XboxDisplayButton::RightStick),
            );
        });
    });
    ui.separator();
    ui.monospace(format!(
        "LT raw: {:?}; RT raw: {:?}",
        view.left_trigger, view.right_trigger
    ));
}

/// This lossy ratio is exclusively for drawing a progress bar; stored values stay native `i32`s.
#[allow(clippy::cast_precision_loss)]
fn axis_marker(value: i32, minimum: i32, maximum: i32) -> f32 {
    let span = (i64::from(maximum) - i64::from(minimum)).max(1) as f32;
    ((i64::from(value) - i64::from(minimum)) as f32 / span).clamp(0.0, 1.0)
}

fn render_frames(ui: &mut egui::Ui, state: &TesterState) {
    ui.separator();
    ui.heading("Newest complete raw event frames (up to 64 shown)");
    egui::ScrollArea::vertical()
        .max_height(180.0)
        .show(ui, |ui| {
            for frame in state.frames().iter().rev().take(MAX_RENDERED_FRAMES) {
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
    ui.heading("Newest lifecycle and errors (up to 128 shown)");
    if state.log().is_empty() {
        ui.weak("No events yet. Use a synthetic replay frame or explicitly start live capture.");
    } else {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for event in state.log().iter().rev().take(MAX_RENDERED_LOG_ENTRIES) {
                ui.monospace(event);
            }
        });
    }
}

fn render_hid(ui: &mut egui::Ui, state: &TesterState) {
    ui.separator();
    ui.heading("Experimental HID fixture evidence");
    let Some(hid) = state.hid() else {
        ui.weak("No HID fixture replayed. This tester never opens hidraw devices.");
        return;
    };
    ui.monospace(format!("descriptor items: {}", hid.item_count));
    for layout in &hid.layouts {
        ui.monospace(format!(
            "{:?} report ID {}: {} bits, {} fields",
            layout.report_type,
            layout.report_id,
            layout.payload_bits,
            layout.fields.len()
        ));
    }
    for report in &hid.reports {
        ui.monospace(format!(
            "{:?} raw bytes: {:02x?}",
            report.report_type, report.bytes
        ));
    }
    for diagnostic in &hid.diagnostics {
        ui.colored_label(egui::Color32::YELLOW, diagnostic);
    }
}

impl TesterApp {
    fn drain_events(&mut self) -> bool {
        if let Some(receiver) = &self.receiver {
            for _ in 0..MAX_EVENTS_PER_UPDATE {
                let Ok(event) = receiver.try_recv() else {
                    return false;
                };
                self.state.apply(event);
            }
        }
        self.receiver.is_some()
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
                self.start_live_capture(ctx.clone());
            }
            if ui.button("Show synthetic replay frame").clicked() {
                self.synthetic_frame();
            }
            if ui.button("Show synthetic HID fixture").clicked() {
                self.synthetic_hid_fixture();
            }
            if ui.button("Stop capture").clicked() {
                self.stop_capture();
            }
            ui.separator();
            ui.heading("Visualizer");
            ui.radio_value(
                &mut self.surface,
                VisualizerSurface::Native,
                "Native dashboard",
            );
            ui.radio_value(
                &mut self.surface,
                VisualizerSurface::XboxDemo,
                "Xbox display demo",
            );
            ui.radio_value(&mut self.surface, VisualizerSurface::Evidence, "Evidence");
            let selected = self.state.selected_source().cloned();
            let mut choice = selected.clone();
            egui::ComboBox::from_label("Visualized source")
                .selected_text(selected.as_ref().map_or("None", SourceId::as_str))
                .show_ui(ui, |ui| {
                    for source_id in self.state.sources().keys() {
                        ui.selectable_value(
                            &mut choice,
                            Some(source_id.clone()),
                            source_id.as_str(),
                        );
                    }
                });
            if choice != selected {
                self.state.select_source(choice);
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
        egui::CentralPanel::default().show(ctx, |ui| match self.surface {
            VisualizerSurface::Native => {
                render_native_dashboard(ui, self.state.native_input_view());
            }
            VisualizerSurface::XboxDemo => {
                render_xbox_demo(ui, self.state.xbox_display_view());
            }
            VisualizerSurface::Evidence => {
                self.render_raw_evidence(ui);
            }
        });
    }

    fn render_raw_evidence(&self, ui: &mut egui::Ui) {
        ui.heading("Native event frames and values");
        ui.label(
            "Values are raw `(event_type, code) -> value` evidence; no gamepad mapping is applied.",
        );
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
        render_hid(ui, &self.state);
        render_lifecycle(ui, &self.state);
    }

    fn access_mode(&self) -> AccessMode {
        match self.access {
            AccessChoice::Shared => AccessMode::Shared,
            AccessChoice::PreferExclusive => AccessMode::PreferExclusive,
            AccessChoice::Exclusive => AccessMode::Exclusive,
        }
    }

    fn start_live_capture(&mut self, repaint: egui::Context) {
        self.stop_capture();
        let (sender, receiver) = mpsc::sync_channel(MAX_PENDING_CAPTURE_EVENTS);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let mode = self.access_mode();
        thread::spawn(move || {
            let mut session =
                CaptureSession::new(gamepad_capture::linux::EvdevProvider::new(), mode);
            let mut next_discovery = Instant::now();
            let mut next_repaint = Instant::now();
            let mut dropped_display_events = 0_usize;
            while !thread_stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                let mut sent_event = false;
                if dropped_display_events > 0 {
                    match sender.try_send(CaptureEvent::SourceError {
                        source_id: SourceId::new("tester-display-queue"),
                        error: gamepad_capture::CaptureError::new(
                            gamepad_capture::CaptureErrorKind::Read,
                            format!(
                                "tester display queue omitted {dropped_display_events} complete event(s)"
                            ),
                        ),
                    }) {
                        Ok(()) => {
                            dropped_display_events = 0;
                            sent_event = true;
                        }
                        Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
                let events = if now >= next_discovery {
                    next_discovery = now + Duration::from_millis(500);
                    session.poll()
                } else {
                    Ok(session.poll_active())
                };
                match events {
                    Ok(events) => {
                        for event in events {
                            match sender.try_send(event) {
                                Ok(()) => sent_event = true,
                                Err(TrySendError::Full(_)) => {
                                    dropped_display_events =
                                        dropped_display_events.saturating_add(1);
                                }
                                Err(TrySendError::Disconnected(_)) => return,
                            }
                        }
                        if sent_event && now >= next_repaint {
                            repaint.request_repaint();
                            next_repaint = now + UI_REPAINT_INTERVAL;
                        }
                    }
                    Err(error) => {
                        let _ = sender.try_send(CaptureEvent::SourceError {
                            source_id: SourceId::new("discovery"),
                            error,
                        });
                    }
                }
                thread::sleep(CAPTURE_POLL_INTERVAL);
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

    fn synthetic_hid_fixture(&mut self) {
        match HidFixture::from_json(include_str!("../../tests/fixtures/synthetic-hid.json")) {
            Ok(fixture) => {
                let _ = self.state.apply_hid_fixture(&fixture);
            }
            Err(error) => self.state.apply(CaptureEvent::SourceError {
                source_id: SourceId::new("synthetic-hid-fixture"),
                error: gamepad_capture::CaptureError::new(
                    gamepad_capture::CaptureErrorKind::InvalidDevice,
                    error.to_string(),
                ),
            }),
        }
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
