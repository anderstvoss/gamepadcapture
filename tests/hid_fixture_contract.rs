#![cfg(feature = "hid")]

use gamepad_capture::hid::{
    EndpointPairing, HidDescriptor, HidFixture, frame_report, pair_endpoint,
};

#[test]
fn checked_in_hid_fixture_is_versioned_sanitized_and_replayable() {
    let fixture = HidFixture::from_json(include_str!("fixtures/synthetic-hid.json"))
        .expect("HID fixture must remain valid");
    assert_eq!(fixture.format_version, 1);
    assert!(fixture.synthetic);
    let descriptor = HidDescriptor::parse(&fixture.descriptor).expect("descriptor must parse");
    let framed = frame_report(&descriptor, &fixture.reports[0]).expect("input report must frame");
    assert_eq!(framed.trailing, [99]);
    assert_eq!(
        pair_endpoint(&fixture.hid_endpoint, &fixture.evdev_endpoints),
        EndpointPairing::Unique { evdev_index: 1 }
    );
    let json = fixture.to_json_pretty().unwrap();
    for prohibited in ["/dev/", "serial", "bluetooth_address", "source_id"] {
        assert!(!json.to_ascii_lowercase().contains(prohibited));
    }
}
