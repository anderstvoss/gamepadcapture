use std::collections::{BTreeMap, BTreeSet};

use crate::{
    AccessMode, CaptureAccess, CaptureError, CaptureEvent, DeviceDescriptor, DiscoverySnapshot,
    EventBatch, SourceId,
};

/// Non-blocking source returned by a platform provider.
pub trait EventSource: Send {
    fn access(&self) -> CaptureAccess;

    /// Read every complete frame currently available without blocking.
    ///
    /// # Errors
    ///
    /// Returns a source-specific read error. Other opened sources remain valid.
    fn read_batches(&mut self) -> Result<Vec<EventBatch>, CaptureError>;
}

/// Platform boundary. Tests and applications can provide deterministic fakes.
pub trait CaptureProvider {
    /// Discover a point-in-time device snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when discovery itself cannot run. Failures describing
    /// individual paths belong in [`DiscoverySnapshot::issues`].
    fn enumerate(&mut self) -> Result<DiscoverySnapshot, CaptureError>;

    /// Open one discovered event source using the requested access policy.
    ///
    /// # Errors
    ///
    /// Returns an open, permission, or exclusive-grab error.
    fn open(
        &mut self,
        device: &DeviceDescriptor,
        mode: AccessMode,
    ) -> Result<Box<dyn EventSource>, CaptureError>;
}

struct ActiveSource {
    descriptor: DeviceDescriptor,
    source: Box<dyn EventSource>,
}

/// Owns hotplug reconciliation and opened capture sources.
pub struct CaptureSession<P> {
    provider: P,
    mode: AccessMode,
    active: BTreeMap<SourceId, ActiveSource>,
}

impl<P: CaptureProvider> CaptureSession<P> {
    #[must_use]
    pub fn new(provider: P, mode: AccessMode) -> Self {
        Self {
            provider,
            mode,
            active: BTreeMap::new(),
        }
    }

    /// Reconcile discovery and collect all currently available input batches.
    /// A failed source is reported without stopping unrelated controllers.
    ///
    /// # Errors
    ///
    /// Returns an error only when the provider cannot produce a discovery
    /// snapshot. Per-device discovery, open, and read failures are events.
    pub fn poll(&mut self) -> Result<Vec<CaptureEvent>, CaptureError> {
        let snapshot = self.provider.enumerate()?;
        let mut output = self.reconcile(snapshot);
        output.extend(self.poll_active());
        Ok(output)
    }

    /// Reconcile an externally obtained point-in-time discovery snapshot.
    ///
    /// This permits an embedding application to perform potentially slow host
    /// discovery away from its active input-read loop. Opening a source still
    /// occurs through this session's provider, and complete input frames remain
    /// available only through [`Self::poll_active`].
    #[must_use]
    pub fn reconcile(&mut self, snapshot: DiscoverySnapshot) -> Vec<CaptureEvent> {
        let discovered_ids: BTreeSet<_> = snapshot
            .devices
            .iter()
            .map(|device| device.source_id.clone())
            .collect();
        let mut output: Vec<_> = snapshot
            .issues
            .into_iter()
            .map(CaptureEvent::DiscoveryError)
            .collect();

        let removed: Vec<_> = self
            .active
            .keys()
            .filter(|source_id| !discovered_ids.contains(*source_id))
            .cloned()
            .collect();
        for source_id in removed {
            if let Some(active) = self.active.remove(&source_id) {
                output.push(CaptureEvent::Disconnected {
                    source_id,
                    physical_id: active.descriptor.physical_id,
                });
            }
        }

        for device in snapshot.devices {
            if self.active.contains_key(&device.source_id) {
                continue;
            }
            match self.provider.open(&device, self.mode) {
                Ok(source) => {
                    let access = source.access();
                    output.push(CaptureEvent::Connected {
                        device: device.clone(),
                        access,
                    });
                    self.active.insert(
                        device.source_id.clone(),
                        ActiveSource {
                            descriptor: device,
                            source,
                        },
                    );
                }
                Err(error) => output.push(CaptureEvent::SourceError {
                    source_id: device.source_id,
                    error,
                }),
            }
        }

        output
    }

    /// Collect batches from already-open sources without rescanning host devices.
    ///
    /// Use [`Self::poll`] regularly when hotplug reconciliation is required. This
    /// method is useful for a tight capture loop after an initial discovery,
    /// because it avoids reopening and inspecting every evdev node per frame.
    #[must_use]
    pub fn poll_active(&mut self) -> Vec<CaptureEvent> {
        let mut output = Vec::new();
        for (source_id, active) in &mut self.active {
            match active.source.read_batches() {
                Ok(batches) => output.extend(batches.into_iter().map(CaptureEvent::Input)),
                Err(error) => output.push(CaptureEvent::SourceError {
                    source_id: source_id.clone(),
                    error,
                }),
            }
        }
        output
    }

    #[must_use]
    pub fn connected_devices(&self) -> Vec<&DeviceDescriptor> {
        self.active
            .values()
            .map(|active| &active.descriptor)
            .collect()
    }

    #[must_use]
    pub fn into_provider(self) -> P {
        self.provider
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;

    use super::*;
    use crate::{
        CaptureErrorKind, ControllerClass, DeviceIdentity, DeviceProvenance, PhysicalDeviceId,
        Transport,
    };

    struct FakeSource {
        access: CaptureAccess,
        reads: VecDeque<Result<Vec<EventBatch>, CaptureError>>,
    }

    impl EventSource for FakeSource {
        fn access(&self) -> CaptureAccess {
            self.access
        }

        fn read_batches(&mut self) -> Result<Vec<EventBatch>, CaptureError> {
            self.reads.pop_front().unwrap_or_else(|| Ok(Vec::new()))
        }
    }

    struct FakeProvider {
        scans: VecDeque<Vec<DeviceDescriptor>>,
        fail_open: bool,
        access: CaptureAccess,
    }

    impl CaptureProvider for FakeProvider {
        fn enumerate(&mut self) -> Result<DiscoverySnapshot, CaptureError> {
            Ok(DiscoverySnapshot {
                devices: self.scans.pop_front().unwrap_or_default(),
                issues: Vec::new(),
            })
        }

        fn open(
            &mut self,
            _device: &DeviceDescriptor,
            mode: AccessMode,
        ) -> Result<Box<dyn EventSource>, CaptureError> {
            if self.fail_open {
                return Err(CaptureError::new(CaptureErrorKind::ExclusiveGrab, "busy"));
            }
            Ok(Box::new(FakeSource {
                access: if mode == AccessMode::Shared {
                    CaptureAccess::Shared
                } else {
                    self.access
                },
                reads: VecDeque::new(),
            }))
        }
    }

    fn device() -> DeviceDescriptor {
        DeviceDescriptor {
            physical_id: PhysicalDeviceId::new("pad"),
            source_id: SourceId::new("pad:source:1"),
            reported_name: "Test Pad".to_owned(),
            identity: DeviceIdentity {
                bus_type: 3,
                vendor_id: 1,
                product_id: 2,
                version: 1,
            },
            transport: Transport::Usb,
            provenance: DeviceProvenance::Physical,
            identity_stability: crate::IdentityStability::ConnectionOnly,
            class: ControllerClass::Gamepad,
            device_path: PathBuf::from("/dev/input/event1"),
            physical_path: None,
            unique_id: None,
            controls: Vec::new(),
        }
    }

    #[test]
    fn lifecycle_emits_connect_then_disconnect_once() {
        let pad = device();
        let provider = FakeProvider {
            scans: VecDeque::from([vec![pad.clone()], vec![], vec![]]),
            fail_open: false,
            access: CaptureAccess::Exclusive,
        };
        let mut session = CaptureSession::new(provider, AccessMode::Exclusive);
        let first = session.poll().unwrap();
        assert!(matches!(
            &first[0],
            CaptureEvent::Connected { device, access: CaptureAccess::Exclusive }
                if device == &pad
        ));
        assert!(matches!(
            &session.poll().unwrap()[0],
            CaptureEvent::Disconnected { source_id, .. } if source_id == &pad.source_id
        ));
        assert!(session.poll().unwrap().is_empty());
    }

    #[test]
    fn active_poll_does_not_rescan_or_disconnect_an_open_source() {
        let pad = device();
        let provider = FakeProvider {
            scans: VecDeque::from([vec![pad]]),
            fail_open: false,
            access: CaptureAccess::Shared,
        };
        let mut session = CaptureSession::new(provider, AccessMode::Shared);
        assert!(matches!(
            session.poll().unwrap().as_slice(),
            [CaptureEvent::Connected { .. }]
        ));
        assert!(session.poll_active().is_empty());
        assert_eq!(session.connected_devices().len(), 1);
    }

    #[test]
    fn externally_obtained_snapshot_reconciles_without_consuming_provider_discovery() {
        let pad = device();
        let provider = FakeProvider {
            scans: VecDeque::from([vec![pad.clone()]]),
            fail_open: false,
            access: CaptureAccess::Shared,
        };
        let mut session = CaptureSession::new(provider, AccessMode::Shared);

        let reconciled = session.reconcile(DiscoverySnapshot {
            devices: vec![pad],
            issues: Vec::new(),
        });
        assert!(matches!(
            reconciled.as_slice(),
            [CaptureEvent::Connected { .. }]
        ));

        // The queued fake scan remains available to `poll`, demonstrating that
        // reconciliation used only the supplied snapshot.
        assert!(session.poll().unwrap().is_empty());
        assert_eq!(session.connected_devices().len(), 1);
    }

    #[test]
    fn open_failure_is_observable_and_does_not_claim_connection() {
        let provider = FakeProvider {
            scans: VecDeque::from([vec![device()]]),
            fail_open: true,
            access: CaptureAccess::Exclusive,
        };
        let mut session = CaptureSession::new(provider, AccessMode::Exclusive);
        let events = session.poll().unwrap();
        assert!(matches!(
            &events[0],
            CaptureEvent::SourceError { error, .. }
                if error.kind == CaptureErrorKind::ExclusiveGrab
        ));
        assert!(session.connected_devices().is_empty());
    }

    #[test]
    fn duplicate_discovery_entries_open_one_source_once() {
        let pad = device();
        let provider = FakeProvider {
            scans: VecDeque::from([vec![pad.clone(), pad], vec![]]),
            fail_open: false,
            access: CaptureAccess::Exclusive,
        };
        let mut session = CaptureSession::new(provider, AccessMode::Exclusive);
        let events = session.poll().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, CaptureEvent::Connected { .. }))
                .count(),
            1
        );
        assert_eq!(session.connected_devices().len(), 1);
    }

    #[test]
    fn preferred_exclusive_fallback_remains_visible() {
        let provider = FakeProvider {
            scans: VecDeque::from([vec![device()]]),
            fail_open: false,
            access: CaptureAccess::SharedFallback,
        };
        let mut session = CaptureSession::new(provider, AccessMode::PreferExclusive);
        assert!(matches!(
            session.poll().unwrap()[0],
            CaptureEvent::Connected {
                access: CaptureAccess::SharedFallback,
                ..
            }
        ));
    }
}
