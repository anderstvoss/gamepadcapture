//! Pure state model used by the optional gamepad tester window.

use std::collections::BTreeMap;

use crate::{
    AutoProfileDetector, CaptureAccess, CaptureEvent, DeviceDescriptor, EventBatch, ProfileId,
    ProfileSelectionMode, SourceId,
};

/// The current state of one connected source, including the access result and
/// inspectable automatic profile evidence.
#[derive(Debug, Clone)]
pub struct TesterSource {
    pub device: DeviceDescriptor,
    pub access: CaptureAccess,
    pub profile: ProfileSelectionMode,
}

/// Native values and lifecycle state rendered by the tester.
#[derive(Debug, Default)]
pub struct TesterState {
    sources: BTreeMap<SourceId, TesterSource>,
    values: BTreeMap<(SourceId, u16, u16), i32>,
    frames: Vec<EventBatch>,
    log: Vec<String>,
}

impl TesterState {
    /// Incorporate one capture event without changing its native interpretation.
    pub fn apply(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::Connected { device, access } => {
                self.log
                    .push(format!("connected {} ({access:?})", device.source_id));
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
            }
            CaptureEvent::Input(batch) => {
                for event in &batch.events {
                    self.values.insert(
                        (batch.source_id.clone(), event.event_type, event.code),
                        event.value,
                    );
                }
                self.log
                    .push(format!("frame {} from {}", batch.sequence, batch.source_id));
                self.frames.push(batch);
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

    /// Preview an explicit reader without discarding automatic candidates.
    pub fn force_profile(&mut self, source_id: &SourceId, profile_id: ProfileId) {
        if let Some(source) = self.sources.get_mut(source_id) {
            let automatic = source.profile.automatic().clone();
            source.profile = automatic.force(profile_id);
            self.log
                .push(format!("forced profile preview for {source_id}"));
        }
    }

    /// Return a source to automatic profile selection.
    pub fn clear_forced_profile(&mut self, source_id: &SourceId) {
        if let Some(source) = self.sources.get_mut(source_id) {
            source.profile = ProfileSelectionMode::Auto(source.profile.automatic().clone());
            self.log
                .push(format!("cleared forced profile preview for {source_id}"));
        }
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
        assert_eq!(state.frames().len(), 1);
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
}
