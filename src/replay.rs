//! Deterministic capture-provider fixtures for tests and diagnostics.
//!
//! Replay inputs are deliberately native [`EventBatch`] values. They do not
//! normalize controller controls or depend on `/dev/input`.

use std::{
    collections::{BTreeMap, VecDeque},
    time::UNIX_EPOCH,
};

use serde::{Deserialize, Serialize};

use crate::{
    CaptureAccess, CaptureError, CaptureErrorKind, CaptureProvider, ControlDescriptor,
    DeviceDescriptor, DiscoverySnapshot, EventBatch, EventSource, SourceId,
};

/// A sanitized, versioned description of fixture sources.
///
/// This deliberately omits device paths, physical paths, unique IDs, serials,
/// and Bluetooth addresses. Hardware recordings may retain those values in a
/// private bundle, but shared fixtures must use this manifest form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureManifest {
    pub format_version: u32,
    pub synthetic: bool,
    pub sources: Vec<FixtureSource>,
}

impl FixtureManifest {
    /// Build the current manifest format from descriptors without identifiers
    /// that can identify a host or controller connection.
    #[must_use]
    pub fn sanitized(synthetic: bool, devices: &[DeviceDescriptor]) -> Self {
        Self {
            format_version: 1,
            synthetic,
            sources: devices
                .iter()
                .map(|device| FixtureSource {
                    reported_name: device.reported_name.clone(),
                    bus_type: device.identity.bus_type,
                    vendor_id: device.identity.vendor_id,
                    product_id: device.identity.product_id,
                    version: device.identity.version,
                    transport: format!("{:?}", device.transport),
                    control_count: device.controls.len(),
                })
                .collect(),
        }
    }

    /// Serialize the public, sanitized manifest as stable pretty JSON.
    ///
    /// # Errors
    ///
    /// Returns serialization failures from `serde_json`.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a shared fixture manifest.
    ///
    /// # Errors
    ///
    /// Returns invalid JSON or schema errors from `serde_json`.
    pub fn from_json(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }
}

/// One non-sensitive source entry in a [`FixtureManifest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureSource {
    pub reported_name: String,
    pub bus_type: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
    pub transport: String,
    pub control_count: usize,
}

/// A sanitized fixture bundle containing the discovery manifest and raw frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureRecording {
    pub manifest: FixtureManifest,
    /// Native controls without kernel labels, paths, or persistent IDs.
    pub controls: Vec<FixtureControl>,
    pub frames: Vec<FixtureFrame>,
    /// Labeled frame ranges and optional operator notes from the capture session.
    pub segments: Vec<FixtureSegment>,
}

impl FixtureRecording {
    /// Convert native batches to a bundle without retaining source paths or IDs.
    #[must_use]
    pub fn sanitized(
        manifest: FixtureManifest,
        batches: &[EventBatch],
        source_indices: &BTreeMap<SourceId, usize>,
    ) -> Self {
        Self::with_controls(manifest, Vec::new(), batches, source_indices, Vec::new())
    }

    /// Create a complete public capture bundle from descriptors, frames, and notes.
    ///
    /// Names, paths, unique identifiers, serials, and Bluetooth addresses are
    /// intentionally excluded from the control records and observations are
    /// caller-provided text that must be reviewed before sharing.
    #[must_use]
    pub fn sanitized_capture(
        manifest: FixtureManifest,
        devices: &[DeviceDescriptor],
        batches: &[EventBatch],
        source_indices: &BTreeMap<SourceId, usize>,
        operator_observations: Vec<String>,
    ) -> Self {
        let controls = devices
            .iter()
            .filter_map(|device| {
                source_indices
                    .get(&device.source_id)
                    .map(|index| (*index, &device.controls))
            })
            .flat_map(|(source_index, controls)| {
                controls
                    .iter()
                    .map(move |control| FixtureControl::from_native(source_index, control))
            })
            .collect();
        Self::with_controls(
            manifest,
            controls,
            batches,
            source_indices,
            operator_observations,
        )
    }

    fn with_controls(
        manifest: FixtureManifest,
        controls: Vec<FixtureControl>,
        batches: &[EventBatch],
        source_indices: &BTreeMap<SourceId, usize>,
        operator_observations: Vec<String>,
    ) -> Self {
        let frames = batches
            .iter()
            .filter_map(|batch| {
                let source_index = *source_indices.get(&batch.source_id)?;
                Some(FixtureFrame {
                    source_index,
                    sequence: batch.sequence,
                    events: batch
                        .events
                        .iter()
                        .map(|event| FixtureNativeEvent {
                            timestamp_micros: event
                                .timestamp
                                .duration_since(UNIX_EPOCH)
                                .map_or(0, |duration| duration.as_micros()),
                            event_type: event.event_type,
                            code: event.code,
                            value: event.value,
                        })
                        .collect(),
                })
            })
            .collect::<Vec<_>>();
        let segments = vec![FixtureSegment {
            label: "bounded-capture".to_owned(),
            first_frame: 0,
            frame_count: frames.len(),
            operator_observations,
        }];
        Self {
            manifest,
            controls,
            frames,
            segments,
        }
    }

    /// Serialize the shared bundle as pretty JSON.
    ///
    /// # Errors
    ///
    /// Returns serialization failures from `serde_json`.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// One control advertised by a source, represented without a local label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureControl {
    pub source_index: usize,
    pub event_type: u16,
    pub code: u16,
    pub absolute_range: Option<FixtureAxisRange>,
}

impl FixtureControl {
    fn from_native(source_index: usize, control: &ControlDescriptor) -> Self {
        match control {
            ControlDescriptor::Key { code, .. } => Self::simple(source_index, 1, *code),
            ControlDescriptor::RelativeAxis { code, .. } => Self::simple(source_index, 2, *code),
            ControlDescriptor::AbsoluteAxis { code, info, .. } => Self {
                source_index,
                event_type: 3,
                code: *code,
                absolute_range: Some(FixtureAxisRange {
                    minimum: info.minimum,
                    maximum: info.maximum,
                    fuzz: info.fuzz,
                    flat: info.flat,
                    resolution: info.resolution,
                }),
            },
            ControlDescriptor::Switch { code, .. } => Self::simple(source_index, 5, *code),
            ControlDescriptor::Led { code, .. } => Self::simple(source_index, 17, *code),
            ControlDescriptor::ForceFeedback { code, .. } => Self::simple(source_index, 21, *code),
        }
    }

    const fn simple(source_index: usize, event_type: u16, code: u16) -> Self {
        Self {
            source_index,
            event_type,
            code,
            absolute_range: None,
        }
    }
}

/// Advertised native absolute-axis bounds and tuning metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureAxisRange {
    pub minimum: i32,
    pub maximum: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

/// A named interval of frames together with reviewed operator observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureSegment {
    pub label: String,
    pub first_frame: usize,
    pub frame_count: usize,
    pub operator_observations: Vec<String>,
}

/// One native event frame, indexed into [`FixtureManifest::sources`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureFrame {
    pub source_index: usize,
    pub sequence: u64,
    pub events: Vec<FixtureNativeEvent>,
}

/// Timestamped native input evidence stored in a fixture frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureNativeEvent {
    pub timestamp_micros: u128,
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
}

/// Read results emitted by one source when it is opened by [`ReplayProvider`].
#[derive(Debug, Clone)]
pub struct ReplaySourcePlan {
    pub access: CaptureAccess,
    pub reads: VecDeque<Result<Vec<EventBatch>, CaptureError>>,
}

impl ReplaySourcePlan {
    #[must_use]
    pub fn new(
        access: CaptureAccess,
        reads: impl IntoIterator<Item = Result<Vec<EventBatch>, CaptureError>>,
    ) -> Self {
        Self {
            access,
            reads: reads.into_iter().collect(),
        }
    }
}

/// A deterministic provider whose scans and source reads are supplied by tests
/// or diagnostic tools.
#[derive(Debug)]
pub struct ReplayProvider {
    scans: VecDeque<Result<DiscoverySnapshot, CaptureError>>,
    sources: BTreeMap<SourceId, VecDeque<ReplaySourcePlan>>,
}

impl ReplayProvider {
    #[must_use]
    pub fn new(
        scans: impl IntoIterator<Item = Result<DiscoverySnapshot, CaptureError>>,
        sources: impl IntoIterator<Item = (SourceId, ReplaySourcePlan)>,
    ) -> Self {
        let mut grouped = BTreeMap::<SourceId, VecDeque<ReplaySourcePlan>>::new();
        for (source_id, plan) in sources {
            grouped.entry(source_id).or_default().push_back(plan);
        }
        Self {
            scans: scans.into_iter().collect(),
            sources: grouped,
        }
    }
}

impl CaptureProvider for ReplayProvider {
    fn enumerate(&mut self) -> Result<DiscoverySnapshot, CaptureError> {
        self.scans
            .pop_front()
            .unwrap_or_else(|| Ok(DiscoverySnapshot::default()))
    }

    fn open(
        &mut self,
        device: &DeviceDescriptor,
        _mode: crate::AccessMode,
    ) -> Result<Box<dyn EventSource>, CaptureError> {
        let Some(plans) = self.sources.get_mut(&device.source_id) else {
            return Err(CaptureError::new(
                CaptureErrorKind::Open,
                "source has no replay plan",
            ));
        };
        let Some(plan) = plans.pop_front() else {
            return Err(CaptureError::new(
                CaptureErrorKind::Open,
                "source replay plan was exhausted",
            ));
        };
        Ok(Box::new(ReplaySource { plan }))
    }
}

#[derive(Debug)]
struct ReplaySource {
    plan: ReplaySourcePlan,
}

impl EventSource for ReplaySource {
    fn access(&self) -> CaptureAccess {
        self.plan.access
    }

    fn read_batches(&mut self) -> Result<Vec<EventBatch>, CaptureError> {
        self.plan
            .reads
            .pop_front()
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use super::*;
    use crate::{
        AbsoluteAxisInfo, AccessMode, CaptureEvent, CaptureSession, ControlDescriptor,
        ControllerClass, DeviceIdentity, DeviceProvenance, IdentityStability, PhysicalDeviceId,
        Transport,
    };

    fn device() -> DeviceDescriptor {
        DeviceDescriptor {
            physical_id: PhysicalDeviceId::new("fixture-pad"),
            source_id: SourceId::new("fixture-source"),
            reported_name: "Synthetic Pad".to_owned(),
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
            device_path: PathBuf::from("/private/path"),
            physical_path: Some("private-topology".to_owned()),
            unique_id: Some("private-serial".to_owned()),
            controls: vec![ControlDescriptor::AbsoluteAxis {
                code: 0,
                name: "private-control-label".to_owned(),
                info: AbsoluteAxisInfo {
                    minimum: -10,
                    maximum: 10,
                    current: 0,
                    fuzz: 1,
                    flat: 2,
                    resolution: 3,
                },
            }],
        }
    }

    #[test]
    fn replay_isolates_source_errors_and_hot_unplug() {
        let device = device();
        let read_error = CaptureError::new(CaptureErrorKind::Read, "synthetic failure");
        let provider = ReplayProvider::new(
            [
                Ok(DiscoverySnapshot {
                    devices: vec![device.clone()],
                    issues: Vec::new(),
                }),
                Ok(DiscoverySnapshot::default()),
            ],
            [(
                device.source_id.clone(),
                ReplaySourcePlan::new(CaptureAccess::Shared, [Err(read_error.clone())]),
            )],
        );
        let mut session = CaptureSession::new(provider, AccessMode::Shared);
        let first = session.poll().unwrap();
        assert!(matches!(first[0], CaptureEvent::Connected { .. }));
        assert!(
            matches!(first[1], CaptureEvent::SourceError { ref error, .. } if error == &read_error)
        );
        assert!(matches!(
            session.poll().unwrap()[0],
            CaptureEvent::Disconnected { .. }
        ));
    }

    #[test]
    fn manifest_excludes_private_connection_identifiers() {
        let manifest = FixtureManifest::sanitized(true, &[device()]);
        assert_eq!(manifest.format_version, 1);
        assert!(manifest.synthetic);
        let debug = format!("{manifest:?}");
        assert!(!debug.contains("private-path"));
        assert!(!debug.contains("private-topology"));
        assert!(!debug.contains("private-serial"));
        let json = manifest.to_json_pretty().unwrap();
        assert!(json.contains("\"format_version\": 1"));
        assert!(!json.contains("private"));
        assert_eq!(FixtureManifest::from_json(&json).unwrap(), manifest);
    }

    #[test]
    fn recording_uses_source_indices_not_private_source_ids() {
        let descriptor = device();
        let manifest = FixtureManifest::sanitized(true, &[descriptor.clone()]);
        let indices = BTreeMap::from([(descriptor.source_id.clone(), 0)]);
        let recording = FixtureRecording::sanitized(
            manifest,
            &[EventBatch {
                source_id: descriptor.source_id,
                sequence: 3,
                events: vec![crate::NativeEvent {
                    timestamp: SystemTime::UNIX_EPOCH,
                    event_type: 3,
                    code: 1,
                    value: 42,
                }],
            }],
            &indices,
        );
        assert_eq!(recording.frames[0].source_index, 0);
        assert_eq!(recording.frames[0].events[0].value, 42);
        assert!(
            !recording
                .to_json_pretty()
                .unwrap()
                .contains("fixture-source")
        );
    }

    #[test]
    fn recording_includes_sanitized_controls_and_observations() {
        let descriptor = device();
        let manifest = FixtureManifest::sanitized(false, &[descriptor.clone()]);
        let indices = BTreeMap::from([(descriptor.source_id.clone(), 0)]);
        let recording = FixtureRecording::sanitized_capture(
            manifest,
            &[descriptor],
            &[],
            &indices,
            vec!["operator saw the synthetic test".to_owned()],
        );
        assert_eq!(recording.controls.len(), 1);
        assert_eq!(recording.controls[0].event_type, 3);
        assert_eq!(
            recording.controls[0]
                .absolute_range
                .as_ref()
                .unwrap()
                .maximum,
            10
        );
        assert_eq!(recording.segments[0].operator_observations.len(), 1);
        assert!(
            !recording
                .to_json_pretty()
                .unwrap()
                .contains("private-control-label")
        );
    }
}
