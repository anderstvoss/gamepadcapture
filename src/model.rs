use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::time::SystemTime;

/// Identity for a physical controller. Multiple evdev interfaces may share it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysicalDeviceId(String);

impl PhysicalDeviceId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PhysicalDeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Identity for one kernel event source belonging to a physical device.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(String);

impl SourceId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Transport {
    Usb,
    Bluetooth,
    Virtual,
    Other(u16),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceProvenance {
    Physical,
    Virtual,
    Unknown,
}

/// The scope in which a physical identity can safely be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityStability {
    /// Based on a hardware-provided unique identifier.
    Hardware,
    /// Stable while the controller remains attached at the same host topology.
    Topology,
    /// No persistent discriminator was exposed; do not persist assignments.
    ConnectionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControllerClass {
    Gamepad,
    WheelOrPedals,
    FlightController,
    ControllerLike,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub bus_type: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsoluteAxisInfo {
    pub minimum: i32,
    pub maximum: i32,
    pub current: i32,
    pub fuzz: i32,
    pub flat: i32,
    pub resolution: i32,
}

/// A native Linux control. Names are diagnostic labels; type and code are authoritative.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControlDescriptor {
    Key {
        code: u16,
        name: String,
    },
    AbsoluteAxis {
        code: u16,
        name: String,
        info: AbsoluteAxisInfo,
    },
    RelativeAxis {
        code: u16,
        name: String,
    },
    Switch {
        code: u16,
        name: String,
    },
    Led {
        code: u16,
        name: String,
    },
    ForceFeedback {
        code: u16,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescriptor {
    pub physical_id: PhysicalDeviceId,
    pub source_id: SourceId,
    /// Name reported by the kernel driver for this event interface.
    pub reported_name: String,
    pub identity: DeviceIdentity,
    pub transport: Transport,
    pub provenance: DeviceProvenance,
    pub identity_stability: IdentityStability,
    pub class: ControllerClass,
    pub device_path: PathBuf,
    pub physical_path: Option<String>,
    pub unique_id: Option<String>,
    pub controls: Vec<ControlDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryIssue {
    pub device_path: PathBuf,
    pub error: CaptureError,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoverySnapshot {
    pub devices: Vec<DeviceDescriptor>,
    /// Per-device failures. Healthy devices in this snapshot remain usable.
    pub issues: Vec<DiscoveryIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// Observe events while allowing other consumers to receive them.
    Shared,
    /// Capture exclusively and fail rather than silently leaking input.
    Exclusive,
    /// Try exclusive capture and explicitly report a shared fallback.
    PreferExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAccess {
    Shared,
    Exclusive,
    SharedFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEvent {
    pub timestamp: SystemTime,
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
}

/// One kernel synchronization frame. Values remain in their native domains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventBatch {
    pub source_id: SourceId,
    pub sequence: u64,
    pub events: Vec<NativeEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureEvent {
    Connected {
        device: DeviceDescriptor,
        access: CaptureAccess,
    },
    Input(EventBatch),
    Disconnected {
        source_id: SourceId,
        physical_id: PhysicalDeviceId,
    },
    SourceError {
        source_id: SourceId,
        error: CaptureError,
    },
    DiscoveryError(DiscoveryIssue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureErrorKind {
    Enumeration,
    Open,
    PermissionDenied,
    ExclusiveGrab,
    Read,
    /// The kernel reported `SYN_DROPPED`; incomplete native evidence was discarded.
    SynchronizationLost,
    InvalidDevice,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureError {
    pub kind: CaptureErrorKind,
    pub message: String,
}

impl CaptureError {
    #[must_use]
    pub fn new(kind: CaptureErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.message)
    }
}

impl Error for CaptureError {}
