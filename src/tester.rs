//! Pure state model used by the optional gamepad tester window.

use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

use crate::hid::{HidDescriptor, HidFixture, HidReportFrame, HidReportLayout, frame_report};
use crate::{
    AutoProfileDetector, CaptureAccess, CaptureEvent, DeviceDescriptor, EventBatch, ProfileId,
    ProfileSelectionMode, SourceId,
};

const MAX_RETAINED_FRAMES: usize = 256;
const MAX_RETAINED_LOG_ENTRIES: usize = 512;
const MAX_TIMING_SAMPLES: usize = 256;

/// A deterministic summary of recent diagnostic timing samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencySummary {
    /// Number of samples currently represented, up to 256.
    pub samples: usize,
    /// Median latency in microseconds.
    pub p50_micros: u128,
    /// 95th-percentile latency in microseconds.
    pub p95_micros: u128,
    /// Largest retained latency in microseconds.
    pub max_micros: u128,
}

/// Bounded timing evidence for the optional tester pipeline.
///
/// Timings diagnose this process only. In particular, `kernel_to_gui` uses the
/// kernel event timestamp when it has the same clock basis as `SystemTime`; it
/// is omitted rather than guessed for replay fixtures or incompatible clocks.
#[derive(Debug, Default)]
pub struct TesterTiming {
    capture_core: RecentLatencies,
    queue_to_gui: RecentLatencies,
    state_apply: RecentLatencies,
    kernel_to_gui: RecentLatencies,
    gui_update: RecentLatencies,
}

#[derive(Debug, Default)]
struct RecentLatencies {
    samples: VecDeque<Duration>,
}

impl RecentLatencies {
    fn observe(&mut self, duration: Duration) {
        if self.samples.len() == MAX_TIMING_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back(duration);
    }

    fn summary(&self) -> Option<LatencySummary> {
        let mut micros = self
            .samples
            .iter()
            .map(Duration::as_micros)
            .collect::<Vec<_>>();
        if micros.is_empty() {
            return None;
        }
        micros.sort_unstable();
        let percentile = |numerator: usize, denominator: usize| {
            let index = (micros.len() - 1) * numerator / denominator;
            micros[index]
        };
        Some(LatencySummary {
            samples: micros.len(),
            p50_micros: percentile(50, 100),
            p95_micros: percentile(95, 100),
            max_micros: *micros.last().expect("checked non-empty timing samples"),
        })
    }
}

impl TesterTiming {
    /// Record the duration spent in `CaptureSession::poll` or `poll_active`.
    pub fn record_capture_core(&mut self, duration: Duration) {
        self.capture_core.observe(duration);
    }

    /// Record the elapsed time from capture-worker enqueue to GUI receipt.
    pub fn record_queue_to_gui(&mut self, duration: Duration) {
        self.queue_to_gui.observe(duration);
    }

    /// Record state-model application time for one capture event.
    pub fn record_state_apply(&mut self, duration: Duration) {
        self.state_apply.observe(duration);
    }

    /// Record kernel-event-time to GUI-state latency when both clocks agree.
    pub fn record_kernel_to_gui(&mut self, duration: Duration) {
        self.kernel_to_gui.observe(duration);
    }

    /// Record one complete egui update, including view construction and paint submission.
    pub fn record_gui_update(&mut self, duration: Duration) {
        self.gui_update.observe(duration);
    }

    #[must_use]
    pub fn capture_core(&self) -> Option<LatencySummary> {
        self.capture_core.summary()
    }

    #[must_use]
    pub fn queue_to_gui(&self) -> Option<LatencySummary> {
        self.queue_to_gui.summary()
    }

    #[must_use]
    pub fn state_apply(&self) -> Option<LatencySummary> {
        self.state_apply.summary()
    }

    #[must_use]
    pub fn kernel_to_gui(&self) -> Option<LatencySummary> {
        self.kernel_to_gui.summary()
    }

    #[must_use]
    pub fn gui_update(&self) -> Option<LatencySummary> {
        self.gui_update.summary()
    }
}

/// Pure HID fixture evidence rendered beside native evdev evidence.
#[derive(Debug, Clone)]
pub struct TesterHidEvidence {
    pub item_count: usize,
    pub layouts: Vec<HidReportLayout>,
    pub reports: Vec<HidReportFrame>,
    pub diagnostics: Vec<String>,
}

/// The current state of one connected source, including the access result and
/// inspectable automatic profile evidence.
#[derive(Debug, Clone)]
pub struct TesterSource {
    pub device: DeviceDescriptor,
    pub access: CaptureAccess,
    pub profile: ProfileSelectionMode,
}

/// Native key state suitable for a diagnostic view. `code` remains authoritative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualKey {
    pub code: u16,
    pub name: String,
    pub value: i32,
}

/// Native absolute-axis evidence suitable for a diagnostic view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualAxis {
    pub code: u16,
    pub name: String,
    pub info: crate::AbsoluteAxisInfo,
    pub value: i32,
}

/// Generic, source-selected native evidence for the tester dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeInputView {
    pub source_id: SourceId,
    pub keys: Vec<VisualKey>,
    pub axes: Vec<VisualAxis>,
    /// Native `ABS_HAT0X`/`ABS_HAT0Y` values when both axes are advertised.
    pub dpad: Option<(i32, i32)>,
    pub recent_frame_count: usize,
}

/// Display-only Xbox-compatible presentation assembled from observed Linux codes.
///
/// This is never a capture profile, decoder, or controller-support claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XboxDisplayView {
    pub source_id: SourceId,
    pub buttons: Vec<(XboxDisplayButton, i32)>,
    pub left_stick: (i32, i32),
    pub right_stick: (i32, i32),
    pub dpad: (i32, i32),
    pub left_trigger: Option<i32>,
    pub right_trigger: Option<i32>,
}

/// Labels used only by the Xbox display demo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum XboxDisplayButton {
    South,
    East,
    North,
    West,
    LeftBumper,
    RightBumper,
    View,
    Menu,
    Guide,
    LeftStick,
    RightStick,
}

/// Native values and lifecycle state rendered by the tester.
#[derive(Debug, Default)]
pub struct TesterState {
    sources: BTreeMap<SourceId, TesterSource>,
    values: BTreeMap<(SourceId, u16, u16), i32>,
    frames: Vec<EventBatch>,
    hid: Option<TesterHidEvidence>,
    log: Vec<String>,
    selected_source: Option<SourceId>,
    timing: TesterTiming,
}

impl TesterState {
    /// Incorporate one capture event without changing its native interpretation.
    pub fn apply(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::Connected { device, access } => {
                let source_id = device.source_id.clone();
                self.push_log(format!("connected {} ({access:?})", device.source_id));
                let profile = AutoProfileDetector::new(Vec::new(), ProfileId::new("sdl-joystick"))
                    .detect(&device);
                self.sources.insert(
                    device.source_id.clone(),
                    TesterSource {
                        device,
                        access,
                        profile: ProfileSelectionMode::Auto(profile),
                    },
                );
                if self.selected_source.is_none() {
                    self.selected_source = Some(source_id);
                }
            }
            CaptureEvent::Input(batch) => {
                for event in &batch.events {
                    self.values.insert(
                        (batch.source_id.clone(), event.event_type, event.code),
                        event.value,
                    );
                }
                self.push_log(format!("frame {} from {}", batch.sequence, batch.source_id));
                self.push_frame(batch);
            }
            CaptureEvent::Disconnected { source_id, .. } => {
                self.sources.remove(&source_id);
                self.values.retain(|(known, _, _), _| known != &source_id);
                if self.selected_source.as_ref() == Some(&source_id) {
                    self.selected_source = self.sources.keys().next().cloned();
                }
                self.push_log(format!("disconnected {source_id}"));
            }
            CaptureEvent::SourceError { source_id, error } => {
                self.push_log(format!("source error {source_id}: {error}"));
            }
            CaptureEvent::DiscoveryError(issue) => self.push_log(format!(
                "discovery error {}: {}",
                issue.device_path.display(),
                issue.error
            )),
        }
    }

    #[must_use]
    pub fn sources(&self) -> &BTreeMap<SourceId, TesterSource> {
        &self.sources
    }
    #[must_use]
    pub fn values(&self) -> &BTreeMap<(SourceId, u16, u16), i32> {
        &self.values
    }
    /// Complete native frames in the order the tester observed them.
    #[must_use]
    pub fn frames(&self) -> &[EventBatch] {
        &self.frames
    }
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// Display-only timing evidence for the native tester pipeline.
    #[must_use]
    pub fn timing(&self) -> &TesterTiming {
        &self.timing
    }

    /// Mutable display-only timing evidence for the native tester pipeline.
    pub fn timing_mut(&mut self) -> &mut TesterTiming {
        &mut self.timing
    }

    fn push_frame(&mut self, frame: EventBatch) {
        if self.frames.len() == MAX_RETAINED_FRAMES {
            self.frames.remove(0);
        }
        self.frames.push(frame);
    }

    fn push_log(&mut self, entry: String) {
        if self.log.len() == MAX_RETAINED_LOG_ENTRIES {
            self.log.remove(0);
        }
        self.log.push(entry);
    }

    /// Select an active source for visual inspection.
    pub fn select_source(&mut self, source_id: Option<SourceId>) {
        self.selected_source = source_id.filter(|id| self.sources.contains_key(id));
    }

    #[must_use]
    pub fn selected_source(&self) -> Option<&SourceId> {
        self.selected_source.as_ref()
    }

    /// Build a generic native-input view without changing the captured evidence.
    #[must_use]
    pub fn native_input_view(&self) -> Option<NativeInputView> {
        let source_id = self.selected_source.as_ref()?;
        let source = self.sources.get(source_id)?;
        let value = |event_type, code, fallback| {
            self.values
                .get(&(source_id.clone(), event_type, code))
                .copied()
                .unwrap_or(fallback)
        };
        let keys = source
            .device
            .controls
            .iter()
            .filter_map(|control| match control {
                crate::ControlDescriptor::Key { code, name } => Some(VisualKey {
                    code: *code,
                    name: name.clone(),
                    value: value(1, *code, 0),
                }),
                _ => None,
            })
            .collect();
        let axes = source
            .device
            .controls
            .iter()
            .filter_map(|control| match control {
                crate::ControlDescriptor::AbsoluteAxis { code, name, info } => Some(VisualAxis {
                    code: *code,
                    name: name.clone(),
                    info: info.clone(),
                    value: value(3, *code, info.current),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        let axis_value = |code| {
            axes.iter()
                .find(|axis| axis.code == code)
                .map(|axis| axis.value)
        };
        Some(NativeInputView {
            source_id: source_id.clone(),
            keys,
            dpad: axis_value(0x10).zip(axis_value(0x11)),
            axes,
            recent_frame_count: self
                .frames
                .iter()
                .filter(|frame| frame.source_id == *source_id)
                .count(),
        })
    }

    /// Build an Xbox-shaped display demo only for a matching Linux control shape.
    #[must_use]
    pub fn xbox_display_view(&self) -> Option<XboxDisplayView> {
        let view = self.native_input_view()?;
        let has_key = |code| view.keys.iter().any(|key| key.code == code);
        let has_axis = |code| view.axes.iter().any(|axis| axis.code == code);
        if ![304, 305, 307, 308].into_iter().all(has_key)
            || ![0, 1, 3, 4, 0x10, 0x11].into_iter().all(has_axis)
        {
            return None;
        }
        let key_value = |code| {
            view.keys
                .iter()
                .find(|key| key.code == code)
                .map_or(0, |key| key.value)
        };
        let axis_value = |code| {
            view.axes
                .iter()
                .find(|axis| axis.code == code)
                .map_or(0, |axis| axis.value)
        };
        let optional_axis = |code| {
            view.axes
                .iter()
                .find(|axis| axis.code == code)
                .map(|axis| axis.value)
        };
        Some(XboxDisplayView {
            source_id: view.source_id,
            buttons: vec![
                (XboxDisplayButton::South, key_value(304)),
                (XboxDisplayButton::East, key_value(305)),
                (XboxDisplayButton::North, key_value(307)),
                (XboxDisplayButton::West, key_value(308)),
                (XboxDisplayButton::LeftBumper, key_value(310)),
                (XboxDisplayButton::RightBumper, key_value(311)),
                (XboxDisplayButton::View, key_value(314)),
                (XboxDisplayButton::Menu, key_value(315)),
                (XboxDisplayButton::Guide, key_value(316)),
                (XboxDisplayButton::LeftStick, key_value(317)),
                (XboxDisplayButton::RightStick, key_value(318)),
            ],
            left_stick: (axis_value(0), axis_value(1)),
            right_stick: (axis_value(3), axis_value(4)),
            dpad: (axis_value(0x10), axis_value(0x11)),
            left_trigger: optional_axis(2),
            right_trigger: optional_axis(5),
        })
    }

    /// Apply a sanitized HID fixture without opening a hidraw device.
    ///
    /// # Errors
    ///
    /// Returns descriptor parsing errors after recording them for inspection.
    pub fn apply_hid_fixture(
        &mut self,
        fixture: &HidFixture,
    ) -> Result<(), crate::hid::HidDescriptorError> {
        let descriptor = match HidDescriptor::parse(&fixture.descriptor) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                self.push_log(format!("HID descriptor error: {error}"));
                return Err(error);
            }
        };
        let mut diagnostics = Vec::new();
        for report in &fixture.reports {
            if let Err(error) = frame_report(&descriptor, report) {
                diagnostics.push(error.to_string());
            }
        }
        self.hid = Some(TesterHidEvidence {
            item_count: descriptor.items.len(),
            layouts: descriptor.layouts,
            reports: fixture.reports.clone(),
            diagnostics,
        });
        self.push_log("applied synthetic HID fixture".to_owned());
        Ok(())
    }

    #[must_use]
    pub fn hid(&self) -> Option<&TesterHidEvidence> {
        self.hid.as_ref()
    }

    /// Preview an explicit reader without discarding automatic candidates.
    pub fn force_profile(&mut self, source_id: &SourceId, profile_id: ProfileId) {
        if let Some(source) = self.sources.get_mut(source_id) {
            let automatic = source.profile.automatic().clone();
            source.profile = automatic.force(profile_id);
            self.push_log(format!("forced profile preview for {source_id}"));
        }
    }

    /// Return a source to automatic profile selection.
    pub fn clear_forced_profile(&mut self, source_id: &SourceId) {
        if let Some(source) = self.sources.get_mut(source_id) {
            source.profile = ProfileSelectionMode::Auto(source.profile.automatic().clone());
            self.push_log(format!("cleared forced profile preview for {source_id}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::{
        EndpointEvidence, EndpointFixture, FixtureDeviceIdentity, HidFixture, HidReportType,
        TransportFixture,
    };
    use crate::{
        CaptureAccess, ControllerClass, DeviceIdentity, DeviceProvenance, EventBatch,
        IdentityStability, NativeEvent, PhysicalDeviceId, Transport,
    };
    use std::{path::PathBuf, time::SystemTime};

    fn device() -> DeviceDescriptor {
        DeviceDescriptor {
            physical_id: PhysicalDeviceId::new("pad"),
            source_id: SourceId::new("source"),
            reported_name: "Pad".into(),
            identity: DeviceIdentity {
                bus_type: 3,
                vendor_id: 1,
                product_id: 2,
                version: 1,
            },
            transport: Transport::Usb,
            provenance: DeviceProvenance::Physical,
            identity_stability: IdentityStability::ConnectionOnly,
            class: ControllerClass::Gamepad,
            device_path: PathBuf::new(),
            physical_path: None,
            unique_id: None,
            controls: Vec::new(),
        }
    }

    fn xbox_device() -> DeviceDescriptor {
        let mut device = device();
        device.controls = [304, 305, 307, 308]
            .into_iter()
            .map(|code| crate::ControlDescriptor::Key {
                code,
                name: format!("key-{code}"),
            })
            .chain([0, 1, 3, 4, 0x10, 0x11, 2, 5].into_iter().map(|code| {
                crate::ControlDescriptor::AbsoluteAxis {
                    code,
                    name: format!("axis-{code}"),
                    info: crate::AbsoluteAxisInfo {
                        minimum: if code >= 0x10 { -1 } else { -32_768 },
                        maximum: if code >= 0x10 { 1 } else { 32_767 },
                        current: 0,
                        fuzz: 0,
                        flat: 0,
                        resolution: 0,
                    },
                }
            }))
            .collect();
        device
    }

    #[test]
    fn state_keeps_raw_values_and_clears_them_on_disconnect() {
        let device = device();
        let source = device.source_id.clone();
        let mut state = TesterState::default();
        state.apply(CaptureEvent::Connected {
            device: device.clone(),
            access: CaptureAccess::Shared,
        });
        state.apply(CaptureEvent::Input(EventBatch {
            source_id: source.clone(),
            sequence: 7,
            events: vec![NativeEvent {
                timestamp: SystemTime::UNIX_EPOCH,
                event_type: 3,
                code: 0,
                value: -123,
            }],
        }));
        assert_eq!(state.values().get(&(source.clone(), 3, 0)), Some(&-123));
        assert_eq!(state.frames().len(), 1);
        state.apply(CaptureEvent::Disconnected {
            source_id: source,
            physical_id: device.physical_id,
        });
        assert!(state.sources().is_empty());
        assert!(state.values().is_empty());
    }

    #[test]
    fn native_view_tracks_raw_values_and_selected_source() {
        let device = xbox_device();
        let source = device.source_id.clone();
        let mut state = TesterState::default();
        state.apply(CaptureEvent::Connected {
            device,
            access: CaptureAccess::Shared,
        });
        state.apply(CaptureEvent::Input(EventBatch {
            source_id: source.clone(),
            sequence: 1,
            events: vec![
                NativeEvent {
                    timestamp: SystemTime::UNIX_EPOCH,
                    event_type: 1,
                    code: 304,
                    value: 1,
                },
                NativeEvent {
                    timestamp: SystemTime::UNIX_EPOCH,
                    event_type: 3,
                    code: 0x10,
                    value: -1,
                },
                NativeEvent {
                    timestamp: SystemTime::UNIX_EPOCH,
                    event_type: 3,
                    code: 0x11,
                    value: 1,
                },
            ],
        }));
        let view = state.native_input_view().unwrap();
        assert_eq!(view.source_id, source);
        assert_eq!(
            view.keys.iter().find(|key| key.code == 304).unwrap().value,
            1
        );
        assert_eq!(view.dpad, Some((-1, 1)));
        assert_eq!(view.recent_frame_count, 1);
    }

    #[test]
    fn xbox_display_is_shape_gated_and_does_not_change_raw_evidence() {
        let mut state = TesterState::default();
        state.apply(CaptureEvent::Connected {
            device: device(),
            access: CaptureAccess::Shared,
        });
        assert!(state.xbox_display_view().is_none());
        let device = xbox_device();
        let source = device.source_id.clone();
        state.apply(CaptureEvent::Connected {
            device,
            access: CaptureAccess::Shared,
        });
        state.select_source(Some(source.clone()));
        state.apply(CaptureEvent::Input(EventBatch {
            source_id: source.clone(),
            sequence: 2,
            events: vec![NativeEvent {
                timestamp: SystemTime::UNIX_EPOCH,
                event_type: 1,
                code: 304,
                value: 1,
            }],
        }));
        let before = state.values().clone();
        let display = state.xbox_display_view().unwrap();
        assert_eq!(display.source_id, source);
        assert_eq!(display.buttons[0], (XboxDisplayButton::South, 1));
        assert_eq!(state.values(), &before);
    }

    #[test]
    fn state_keeps_source_errors_without_discarding_other_evidence() {
        let device = device();
        let source = device.source_id.clone();
        let mut state = TesterState::default();
        state.apply(CaptureEvent::Connected {
            device,
            access: CaptureAccess::SharedFallback,
        });
        state.apply(CaptureEvent::SourceError {
            source_id: source.clone(),
            error: crate::CaptureError::new(crate::CaptureErrorKind::Read, "fixture failure"),
        });
        assert!(state.sources().contains_key(&source));
        assert!(
            state
                .log()
                .iter()
                .any(|entry| entry.contains("fixture failure"))
        );
    }

    #[test]
    fn state_retains_bounded_recent_frames_and_log_entries() {
        let device = device();
        let source = device.source_id.clone();
        let mut state = TesterState::default();
        state.apply(CaptureEvent::Connected {
            device,
            access: CaptureAccess::Shared,
        });
        let frame_limit = u64::try_from(MAX_RETAINED_FRAMES).expect("limit fits in u64");
        for sequence in 0..=frame_limit {
            state.apply(CaptureEvent::Input(EventBatch {
                source_id: source.clone(),
                sequence,
                events: Vec::new(),
            }));
        }
        assert_eq!(state.frames().len(), MAX_RETAINED_FRAMES);
        assert_eq!(state.frames().first().map(|frame| frame.sequence), Some(1));
        assert_eq!(
            state.frames().last().map(|frame| frame.sequence),
            Some(frame_limit)
        );

        for index in 0..=MAX_RETAINED_LOG_ENTRIES {
            state.apply(CaptureEvent::SourceError {
                source_id: source.clone(),
                error: crate::CaptureError::new(crate::CaptureErrorKind::Read, index.to_string()),
            });
        }
        assert_eq!(state.log().len(), MAX_RETAINED_LOG_ENTRIES);
        assert!(
            state
                .log()
                .first()
                .is_some_and(|entry| entry.ends_with('1'))
        );
        assert!(state
            .log()
            .last()
            .is_some_and(|entry| entry.ends_with(MAX_RETAINED_LOG_ENTRIES.to_string().as_str())));
    }

    #[test]
    fn timing_summaries_are_bounded_and_percentile_ordered() {
        let mut timing = TesterTiming::default();
        assert_eq!(timing.capture_core(), None);
        assert_eq!(timing.queue_to_gui(), None);
        for micros in 0..=MAX_TIMING_SAMPLES {
            timing.record_capture_core(Duration::from_micros(
                u64::try_from(micros).expect("sample count fits u64"),
            ));
        }
        let summary = timing.capture_core().expect("samples were recorded");
        assert_eq!(summary.samples, MAX_TIMING_SAMPLES);
        assert_eq!(summary.p50_micros, 128);
        assert_eq!(summary.p95_micros, 243);
        assert_eq!(summary.max_micros, 256);
        assert!(summary.p50_micros <= summary.p95_micros);
        assert!(summary.p95_micros <= summary.max_micros);

        timing.record_queue_to_gui(Duration::from_micros(9));
        assert_eq!(
            timing.queue_to_gui(),
            Some(LatencySummary {
                samples: 1,
                p50_micros: 9,
                p95_micros: 9,
                max_micros: 9,
            })
        );
    }

    #[test]
    fn forced_preview_retains_automatic_candidates() {
        let device = device();
        let source = device.source_id.clone();
        let mut state = TesterState::default();
        state.apply(CaptureEvent::Connected {
            device,
            access: CaptureAccess::Shared,
        });
        state.force_profile(&source, ProfileId::new("operator-reader"));
        let profile = &state.sources()[&source].profile;
        assert_eq!(
            profile.forced_profile(),
            Some(&ProfileId::new("operator-reader"))
        );
        assert_eq!(profile.automatic().candidates.len(), 1);
        state.clear_forced_profile(&source);
        assert_eq!(state.sources()[&source].profile.forced_profile(), None);
    }

    #[test]
    fn state_keeps_hid_fixture_parse_and_report_errors() {
        let fixture = HidFixture::new(
            true,
            vec![0x75, 8, 0x95, 1, 0x81, 2],
            vec![HidReportFrame {
                report_type: HidReportType::Input,
                bytes: Vec::new(),
            }],
            EndpointFixture {
                fixture_index: 0,
                evidence: EndpointEvidence {
                    identity: FixtureDeviceIdentity {
                        bus_type: 3,
                        vendor_id: 1,
                        product_id: 2,
                        version: 1,
                    },
                    transport: TransportFixture::Usb,
                    topology_token: None,
                    connection_token: None,
                },
            },
            Vec::new(),
        );
        let mut state = TesterState::default();
        state.apply_hid_fixture(&fixture).unwrap();
        assert_eq!(state.hid().unwrap().layouts.len(), 1);
        assert_eq!(state.hid().unwrap().diagnostics.len(), 1);
    }
}
