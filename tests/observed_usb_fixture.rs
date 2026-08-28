#![cfg(feature = "tester")]

use std::{path::PathBuf, time::UNIX_EPOCH};

use gamepad_capture::{
    AbsoluteAxisInfo, CaptureAccess, CaptureEvent, ControlDescriptor, DeviceDescriptor,
    DeviceIdentity, DeviceProvenance, EventBatch, FixtureRecording, IdentityStability, NativeEvent,
    PhysicalDeviceId, SourceId, Transport, tester::TesterState,
};

#[test]
fn observed_usb_fixture_replays_native_evidence_without_a_device() {
    let fixture =
        FixtureRecording::from_json(include_str!("fixtures/xbox-series-usb-observed-input.json"))
            .expect("observed fixture must remain valid");
    assert!(!fixture.manifest.synthetic);
    assert_eq!(fixture.manifest.sources.len(), 1);
    assert_eq!(fixture.controls.len(), 26);
    assert_eq!(fixture.frames.len(), 14);

    let state = replay_fixture(&fixture);

    let native = state
        .native_input_view()
        .expect("native dashboard is available");
    assert_eq!(native.dpad, Some((1, 0)));
    assert_eq!(native.recent_frame_count, 14);
    assert!(
        native
            .axes
            .iter()
            .any(|axis| axis.code == 2 && axis.value == 756)
    );
    assert!(
        native
            .axes
            .iter()
            .any(|axis| axis.code == 5 && axis.value == 139)
    );

    let xbox = state
        .xbox_display_view()
        .expect("observed Linux control shape enables only the display demo");
    assert_eq!(xbox.dpad, (1, 0));
    assert_eq!(xbox.left_trigger, Some(756));
    assert_eq!(xbox.right_trigger, Some(139));
    assert!(xbox.buttons.iter().any(|(button, value)| {
        *button == gamepad_capture::tester::XboxDisplayButton::South && *value == 1
    }));

    let json = fixture.to_json_pretty().expect("fixture serializes");
    for prohibited in [
        "/dev/",
        "serial",
        "bluetooth address",
        "source_id",
        "unique_id",
        "physical_path",
    ] {
        assert!(!json.to_ascii_lowercase().contains(prohibited));
    }
}

fn replay_fixture(fixture: &FixtureRecording) -> TesterState {
    let source_id = SourceId::new("observed-usb-fixture-0");
    let source = &fixture.manifest.sources[0];
    let controls = fixture
        .controls
        .iter()
        .map(
            |control| match (&control.absolute_range, control.event_type) {
                (Some(range), 3) => ControlDescriptor::AbsoluteAxis {
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
                (None, 21) => ControlDescriptor::ForceFeedback {
                    code: control.code,
                    name: format!("fixture-force-feedback-{}", control.code),
                },
                (None, 1) => ControlDescriptor::Key {
                    code: control.code,
                    name: format!("fixture-key-{}", control.code),
                },
                (range, event_type) => {
                    panic!("unexpected fixture control: type={event_type}, range={range:?}")
                }
            },
        )
        .collect();
    let device = DeviceDescriptor {
        physical_id: PhysicalDeviceId::new("observed-usb-physical-0"),
        source_id: source_id.clone(),
        reported_name: source.reported_name.clone(),
        identity: DeviceIdentity {
            bus_type: source.bus_type,
            vendor_id: source.vendor_id,
            product_id: source.product_id,
            version: source.version,
        },
        transport: Transport::Usb,
        provenance: DeviceProvenance::Physical,
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
    state
}
