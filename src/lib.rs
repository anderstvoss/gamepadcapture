#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod identity;
mod model;
mod profile;
mod replay;
mod session;

#[cfg(feature = "tester")]
pub mod tester;

#[cfg(target_os = "linux")]
pub mod linux;

pub use identity::{build_physical_id, build_source_id};
pub use model::{
    AbsoluteAxisInfo, AccessMode, CaptureAccess, CaptureError, CaptureErrorKind, CaptureEvent,
    ControlDescriptor, ControllerClass, DeviceDescriptor, DeviceIdentity, DeviceProvenance,
    DiscoveryIssue, DiscoverySnapshot, EventBatch, IdentityStability, NativeEvent,
    PhysicalDeviceId, SourceId, Transport,
};
pub use profile::{
    AutoProfileDetector, CaptureProfile, CaptureProfileFamily, DetectionConfidence,
    DeviceProfileAssignment, ProfileCandidate, ProfileEvidence, ProfileId, ProfileMatch,
    ProfileSelection, ProfileSelectionMode,
};
pub use replay::{
    FixtureAxisRange, FixtureControl, FixtureFrame, FixtureManifest, FixtureNativeEvent,
    FixtureRecording, FixtureSegment, FixtureSource, ReplayProvider, ReplaySourcePlan,
};
pub use session::{CaptureProvider, CaptureSession, EventSource};
