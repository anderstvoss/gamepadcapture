//! Linux evdev provider. Enumeration is side-effect free; devices are grabbed only on open.

use std::fs;
use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::Path;

use evdev::{BusType, Device, EventType};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};

use crate::{
    AbsoluteAxisInfo, AccessMode, CaptureAccess, CaptureError, CaptureErrorKind, CaptureProvider,
    ControlDescriptor, ControllerClass, DeviceDescriptor, DeviceIdentity, DeviceProvenance,
    DiscoveryIssue, DiscoverySnapshot, EventBatch, EventSource, IdentityStability, NativeEvent,
    Transport, build_physical_id, build_source_id,
};

#[derive(Debug, Default)]
pub struct EvdevProvider;

impl EvdevProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl CaptureProvider for EvdevProvider {
    fn enumerate(&mut self) -> Result<DiscoverySnapshot, CaptureError> {
        let mut devices = Vec::new();
        let mut issues = Vec::new();
        for (path, device) in evdev::enumerate() {
            match describe(&path, &device) {
                Ok(Some(descriptor)) => devices.push(descriptor),
                Ok(None) => {}
                Err(error) => issues.push(DiscoveryIssue {
                    device_path: path,
                    error,
                }),
            }
        }
        devices.sort_by(|left, right| left.source_id.cmp(&right.source_id));
        Ok(DiscoverySnapshot { devices, issues })
    }

    fn open(
        &mut self,
        descriptor: &DeviceDescriptor,
        mode: AccessMode,
    ) -> Result<Box<dyn EventSource>, CaptureError> {
        let mut device = Device::open(&descriptor.device_path)
            .map_err(|error| io_error(CaptureErrorKind::Open, &descriptor.device_path, &error))?;
        device
            .set_nonblocking(true)
            .map_err(|error| io_error(CaptureErrorKind::Open, &descriptor.device_path, &error))?;
        let access = match mode {
            AccessMode::Shared => CaptureAccess::Shared,
            AccessMode::Exclusive => {
                device.grab().map_err(|error| {
                    io_error(
                        CaptureErrorKind::ExclusiveGrab,
                        &descriptor.device_path,
                        &error,
                    )
                })?;
                CaptureAccess::Exclusive
            }
            AccessMode::PreferExclusive => match device.grab() {
                Ok(()) => CaptureAccess::Exclusive,
                Err(_) => CaptureAccess::SharedFallback,
            },
        };
        Ok(Box::new(EvdevSource {
            device,
            source_id: descriptor.source_id.clone(),
            access,
            sequence: 0,
            pending: Vec::new(),
        }))
    }
}

struct EvdevSource {
    device: Device,
    source_id: crate::SourceId,
    access: CaptureAccess,
    sequence: u64,
    pending: Vec<NativeEvent>,
}

impl EventSource for EvdevSource {
    fn access(&self) -> CaptureAccess {
        self.access
    }

    fn read_batches(&mut self) -> Result<Vec<EventBatch>, CaptureError> {
        if !readable_now(self.device.as_fd()).map_err(|error| {
            io_error(
                CaptureErrorKind::Read,
                Path::new(self.source_id.as_str()),
                &error,
            )
        })? {
            return Ok(Vec::new());
        }
        let events = match self.device.fetch_events() {
            Ok(events) => events.collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(Vec::new()),
            Err(error) => {
                return Err(io_error(
                    CaptureErrorKind::Read,
                    Path::new(self.source_id.as_str()),
                    &error,
                ));
            }
        };
        let mut batches = Vec::new();
        for event in events {
            if is_synchronization_lost(event.event_type().0, event.code()) {
                self.pending.clear();
                return Err(CaptureError::new(
                    CaptureErrorKind::SynchronizationLost,
                    format!("{}: kernel event buffer overrun", self.source_id),
                ));
            }
            let is_report = event.event_type() == EventType::SYNCHRONIZATION && event.code() == 0;
            if !is_report {
                self.pending.push(NativeEvent {
                    timestamp: event.timestamp(),
                    event_type: event.event_type().0,
                    code: event.code(),
                    value: event.value(),
                });
                continue;
            }
            if self.pending.is_empty() {
                continue;
            }
            self.sequence = self.sequence.wrapping_add(1);
            batches.push(EventBatch {
                source_id: self.source_id.clone(),
                sequence: self.sequence,
                events: std::mem::take(&mut self.pending),
            });
        }
        Ok(batches)
    }
}

/// Check readiness before asking evdev to read, so an idle source never stalls a session poll.
fn readable_now(fd: BorrowedFd<'_>) -> io::Result<bool> {
    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
    let ready = poll(&mut fds, PollTimeout::ZERO).map_err(io::Error::from)?;
    if ready == 0 {
        return Ok(false);
    }
    Ok(fds[0].revents().is_some_and(|events| {
        events.intersects(PollFlags::POLLIN | PollFlags::POLLERR | PollFlags::POLLHUP)
    }))
}

fn is_synchronization_lost(event_type: u16, code: u16) -> bool {
    event_type == EventType::SYNCHRONIZATION.0 && code == 3
}

fn describe(path: &Path, device: &Device) -> Result<Option<DeviceDescriptor>, CaptureError> {
    let keys = device.supported_keys();
    let axes = device.supported_absolute_axes();
    let Some(class) = classify(
        keys.map(|values| values.iter().map(|value| value.0)),
        axes.map(|values| values.iter().map(|value| value.0)),
    ) else {
        return Ok(None);
    };
    let input_id = device.input_id();
    let identity = DeviceIdentity {
        bus_type: input_id.bus_type().0,
        vendor_id: input_id.vendor(),
        product_id: input_id.product(),
        version: input_id.version(),
    };
    let transport = transport(input_id.bus_type());
    let unique_id = non_empty(device.unique_name());
    let physical_path = non_empty(device.physical_path());
    let physical_id = build_physical_id(
        &identity,
        transport,
        unique_id.as_deref(),
        physical_path.as_deref(),
    );
    let source_id = build_source_id(&physical_id, &path.to_string_lossy());
    let mut controls = controls(device)?;
    controls.sort_by_key(control_sort_key);
    Ok(Some(DeviceDescriptor {
        physical_id,
        source_id,
        reported_name: device.name().unwrap_or("Unknown controller").to_owned(),
        identity,
        transport,
        provenance: if input_id.bus_type() == BusType::BUS_VIRTUAL || is_virtual_sysfs(path) {
            DeviceProvenance::Virtual
        } else {
            DeviceProvenance::Physical
        },
        identity_stability: if unique_id.is_some() {
            IdentityStability::Hardware
        } else if physical_path.is_some() {
            IdentityStability::Topology
        } else {
            IdentityStability::ConnectionOnly
        },
        class,
        device_path: path.to_path_buf(),
        physical_path,
        unique_id,
        controls,
    }))
}

fn classify<K, A>(keys: Option<K>, axes: Option<A>) -> Option<ControllerClass>
where
    K: Iterator<Item = u16>,
    A: Iterator<Item = u16>,
{
    let keys: Vec<_> = keys?.collect();
    let axes: Vec<_> = axes?.collect();
    let has_gamepad_button = keys.iter().any(|code| (0x130..=0x13f).contains(code));
    let has_joystick_button = keys.iter().any(|code| (0x120..=0x12f).contains(code));
    if (!has_gamepad_button && !has_joystick_button) || axes.is_empty() {
        return None;
    }
    if has_gamepad_button {
        Some(ControllerClass::Gamepad)
    } else {
        Some(ControllerClass::ControllerLike)
    }
}

fn controls(device: &Device) -> Result<Vec<ControlDescriptor>, CaptureError> {
    let mut output = Vec::new();
    if let Some(keys) = device.supported_keys() {
        output.extend(keys.iter().map(|code| ControlDescriptor::Key {
            code: code.0,
            name: format!("{code:?}"),
        }));
    }
    let absinfo = device
        .get_absinfo()
        .map_err(|error| CaptureError::new(CaptureErrorKind::InvalidDevice, error.to_string()))?;
    output.extend(absinfo.map(|(code, info)| ControlDescriptor::AbsoluteAxis {
        code: code.0,
        name: format!("{code:?}"),
        info: AbsoluteAxisInfo {
            minimum: info.minimum(),
            maximum: info.maximum(),
            current: info.value(),
            fuzz: info.fuzz(),
            flat: info.flat(),
            resolution: info.resolution(),
        },
    }));
    if let Some(axes) = device.supported_relative_axes() {
        output.extend(axes.iter().map(|code| ControlDescriptor::RelativeAxis {
            code: code.0,
            name: format!("{code:?}"),
        }));
    }
    if let Some(switches) = device.supported_switches() {
        output.extend(switches.iter().map(|code| ControlDescriptor::Switch {
            code: code.0,
            name: format!("{code:?}"),
        }));
    }
    if let Some(leds) = device.supported_leds() {
        output.extend(leds.iter().map(|code| ControlDescriptor::Led {
            code: code.0,
            name: format!("{code:?}"),
        }));
    }
    if let Some(effects) = device.supported_ff() {
        output.extend(effects.iter().map(|code| ControlDescriptor::ForceFeedback {
            code: code.0,
            name: format!("{code:?}"),
        }));
    }
    Ok(output)
}

fn control_sort_key(control: &ControlDescriptor) -> (u8, u16) {
    match control {
        ControlDescriptor::Key { code, .. } => (1, *code),
        ControlDescriptor::RelativeAxis { code, .. } => (2, *code),
        ControlDescriptor::AbsoluteAxis { code, .. } => (3, *code),
        ControlDescriptor::Switch { code, .. } => (5, *code),
        ControlDescriptor::Led { code, .. } => (17, *code),
        ControlDescriptor::ForceFeedback { code, .. } => (21, *code),
    }
}

fn transport(bus: BusType) -> Transport {
    match bus {
        BusType::BUS_USB => Transport::Usb,
        BusType::BUS_BLUETOOTH => Transport::Bluetooth,
        BusType::BUS_VIRTUAL => Transport::Virtual,
        BusType(0) => Transport::Unknown,
        BusType(value) => Transport::Other(value),
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn is_virtual_sysfs(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    fs::canonicalize(Path::new("/sys/class/input").join(name).join("device"))
        .is_ok_and(|target| target.starts_with("/sys/devices/virtual/"))
}

fn io_error(kind: CaptureErrorKind, path: &Path, error: &io::Error) -> CaptureError {
    let kind = if error.kind() == io::ErrorKind::PermissionDenied {
        CaptureErrorKind::PermissionDenied
    } else {
        kind
    };
    CaptureError::new(kind, format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;

    use super::*;

    #[test]
    fn classification_requires_button_and_axis_capabilities() {
        assert_eq!(
            classify(Some([0x130].into_iter()), Some([0].into_iter())),
            Some(ControllerClass::Gamepad)
        );
        assert_eq!(
            classify(Some([30].into_iter()), Some([0].into_iter())),
            None
        );
        assert_eq!(
            classify(Some([0x130].into_iter()), Some([].into_iter())),
            None
        );
    }

    #[test]
    fn synchronization_loss_is_recognized_without_emitting_a_partial_frame() {
        assert!(is_synchronization_lost(EventType::SYNCHRONIZATION.0, 3));
        assert!(!is_synchronization_lost(EventType::SYNCHRONIZATION.0, 0));
        assert!(!is_synchronization_lost(3, 3));
    }

    #[test]
    fn readiness_gate_returns_without_waiting_for_an_idle_source() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        assert!(!readable_now(reader.as_fd()).unwrap());
        writer.write_all(&[1]).unwrap();
        assert!(readable_now(reader.as_fd()).unwrap());
    }
}
