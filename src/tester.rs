//! Pure state model used by the optional gamepad tester window.

use std::collections::BTreeMap;

use crate::{CaptureEvent, DeviceDescriptor, SourceId};

/// Native values and lifecycle state rendered by the tester.
#[derive(Debug, Default)]
pub struct TesterState {
    sources: BTreeMap<SourceId, DeviceDescriptor>,
    values: BTreeMap<(SourceId, u16, u16), i32>,
    log: Vec<String>,
}

impl TesterState {
    /// Incorporate one capture event without changing its native interpretation.
    pub fn apply(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::Connected { device, access } => {
                self.log
                    .push(format!("connected {} ({access:?})", device.source_id));
                self.sources.insert(device.source_id.clone(), device);
            }
            CaptureEvent::Input(batch) => {
                for event in batch.events {
                    self.values.insert(
                        (batch.source_id.clone(), event.event_type, event.code),
                        event.value,
                    );
                }
                self.log
                    .push(format!("frame {} from {}", batch.sequence, batch.source_id));
            }
            CaptureEvent::Disconnected { source_id, .. } => {
                self.sources.remove(&source_id);
                self.values.retain(|(known, _, _), _| known != &source_id);
                self.log.push(format!("disconnected {source_id}"));
            }
            CaptureEvent::SourceError { source_id, error } => {
                self.log.push(format!("source error {source_id}: {error}"));
            }
            CaptureEvent::DiscoveryError(issue) => self.log.push(format!(
                "discovery error {}: {}",
                issue.device_path.display(),
                issue.error
            )),
        }
    }

    #[must_use]
    pub fn sources(&self) -> &BTreeMap<SourceId, DeviceDescriptor> {
        &self.sources
    }
    #[must_use]
    pub fn values(&self) -> &BTreeMap<(SourceId, u16, u16), i32> {
        &self.values
    }
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        state.apply(CaptureEvent::Disconnected {
            source_id: source,
            physical_id: device.physical_id,
        });
        assert!(state.sources().is_empty());
        assert!(state.values().is_empty());
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
}
