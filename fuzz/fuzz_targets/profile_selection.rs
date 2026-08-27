#![no_main]

use std::path::PathBuf;

use gamepad_capture::{
    AutoProfileDetector, CaptureProfile, CaptureProfileFamily, ControllerClass, DeviceDescriptor,
    DeviceIdentity, DeviceProvenance, IdentityStability, PhysicalDeviceId, ProfileId, ProfileMatch,
    SourceId, Transport,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: (u16, u16, u8)| {
    let transport = match input.2 % 3 {
        0 => Transport::Usb,
        1 => Transport::Bluetooth,
        _ => Transport::Unknown,
    };
    let device = DeviceDescriptor {
        physical_id: PhysicalDeviceId::new("fuzz-device"),
        source_id: SourceId::new("fuzz-source"),
        reported_name: "fuzz".into(),
        identity: DeviceIdentity { bus_type: 3, vendor_id: input.0, product_id: input.1, version: 1 },
        transport,
        provenance: DeviceProvenance::Unknown,
        identity_stability: IdentityStability::ConnectionOnly,
        class: ControllerClass::ControllerLike,
        device_path: PathBuf::new(),
        physical_path: None,
        unique_id: None,
        controls: Vec::new(),
    };
    let detector = AutoProfileDetector::new(
        vec![CaptureProfile {
            id: ProfileId::new("candidate"),
            family: CaptureProfileFamily::Protocol,
            matches: vec![ProfileMatch { vendor_id: Some(input.0), product_id: Some(input.1), transport: Some(transport) }],
        }],
        ProfileId::new("sdl-joystick"),
    );
    let selection = detector.detect(&device);
    assert!(selection.candidates.iter().any(|candidate| candidate.profile_id.as_str() == "sdl-joystick"));
});
