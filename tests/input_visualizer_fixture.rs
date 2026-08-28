#![cfg(feature = "tester")]

use std::{path::PathBuf, time::UNIX_EPOCH};

use gamepad_capture::{
    AbsoluteAxisInfo, CaptureAccess, CaptureEvent, ControlDescriptor, DeviceDescriptor,
    DeviceIdentity, DeviceProvenance, EventBatch, FixtureRecording, IdentityStability, NativeEvent,
    PhysicalDeviceId, SourceId, Transport, tester::TesterState,
};

#[test]
fn synthetic_xbox_shaped_fixture_drives_both_visualizer_views() {
    let fixture = FixtureRecording::from_json(include_str!("fixtures/synthetic-xbox-display.json"))
        .expect("fixture must remain valid");
    assert!(fixture.manifest.synthetic);
    let source_id = SourceId::new("fixture-source-0");
    let source = &fixture.manifest.sources[0];
    let controls = fixture
        .controls
        .iter()
        .map(|control| match &control.absolute_range {
            Some(range) => ControlDescriptor::AbsoluteAxis {
                code: control.code,
                name: format!("fixture-axis-{}", control.code),
                info: AbsoluteAxisInfo {
                    minimum: range.minimum,
                    maximum: range.maximum,
                    current: 0,
                    fuzz: range.fuzz,
                    flat: range.flat,
                    resolution: range.resolution,
                },
            },
            None => ControlDescriptor::Key {
                code: control.code,
                name: format!("fixture-key-{}", control.code),
            },
        })
        .collect();
    let device = DeviceDescriptor {
        physical_id: PhysicalDeviceId::new("fixture-physical-0"),
        source_id: source_id.clone(),
        reported_name: source.reported_name.clone(),
        identity: DeviceIdentity {
            bus_type: source.bus_type,
            vendor_id: source.vendor_id,
            product_id: source.product_id,
            version: source.version,
        },
        transport: Transport::Usb,
        provenance: DeviceProvenance::Unknown,
        identity_stability: IdentityStability::ConnectionOnly,
        class: gamepad_capture::ControllerClass::Gamepad,
        device_path: PathBuf::new(),
        physical_path: None,
        unique_id: None,
        controls,
    };
    let mut state = TesterState::default();
    state.apply(CaptureEvent::Connected {
        device,
        access: CaptureAccess::Shared,
    });
    for frame in &fixture.frames {
        state.apply(CaptureEvent::Input(EventBatch {
            source_id: source_id.clone(),
            sequence: frame.sequence,
            events: frame
                .events
                .iter()
                .map(|event| NativeEvent {
                    timestamp: UNIX_EPOCH,
                    event_type: event.event_type,
                    code: event.code,
                    value: event.value,
                })
                .collect(),
        }));
    }
    let native = state.native_input_view().unwrap();
    assert_eq!(native.dpad, Some((-1, 1)));
    assert_eq!(native.recent_frame_count, 2);
    let xbox = state.xbox_display_view().unwrap();
    assert_eq!(xbox.left_stick, (-16_384, 8192));
    assert_eq!(xbox.left_trigger, Some(777));
    let json = fixture.to_json_pretty().unwrap();
    for prohibited in ["/dev/", "serial", "bluetooth address", "source_id"] {
        assert!(!json.to_ascii_lowercase().contains(prohibited));
    }
}
