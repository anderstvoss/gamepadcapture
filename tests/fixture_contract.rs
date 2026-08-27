use gamepad_capture::FixtureManifest;

#[test]
fn checked_in_synthetic_manifest_is_versioned_and_sanitized() {
    let manifest = FixtureManifest::from_json(include_str!("fixtures/synthetic-manifest.json"))
        .expect("fixture JSON must remain valid");
    assert_eq!(manifest.format_version, 1);
    assert!(manifest.synthetic);
    assert_eq!(manifest.sources.len(), 1);
    let serialized = manifest.to_json_pretty().unwrap();
    for prohibited in [
        "/dev/",
        "serial",
        "bluetooth address",
        "physical_path",
        "unique_id",
    ] {
        assert!(!serialized.to_ascii_lowercase().contains(prohibited));
    }
}
