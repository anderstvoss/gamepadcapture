//! Experimental, pure HID descriptor and report-fixture support.
//!
//! This module never opens hidraw devices or writes controller output. It
//! preserves generic descriptor structure and opaque report bytes so hardware
//! recordings can later drive a validated live backend.

use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::Transport;

const MAX_COLLECTION_DEPTH: usize = 32;

/// The class encoded by a short HID item prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HidItemType {
    Main,
    Global,
    Local,
    Reserved,
}

/// Whether an item uses the compact or long HID encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HidItemKind {
    Short { item_type: HidItemType, tag: u8 },
    Long { tag: u8 },
}

/// A lossless HID item with its descriptor byte offset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HidItem {
    pub offset: usize,
    pub kind: HidItemKind,
    pub data: Vec<u8>,
}

/// Errors raised while preserving a descriptor item stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidParseError {
    TruncatedShortItem { offset: usize, expected: usize },
    TruncatedLongItem { offset: usize, expected: usize },
}

impl fmt::Display for HidParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedShortItem { offset, expected } => {
                write!(
                    formatter,
                    "short HID item at {offset} needs {expected} data bytes"
                )
            }
            Self::TruncatedLongItem { offset, expected } => {
                write!(
                    formatter,
                    "long HID item at {offset} needs {expected} data bytes"
                )
            }
        }
    }
}

impl Error for HidParseError {}

/// Generic report channel. These values do not imply a live output transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum HidReportType {
    Input,
    Output,
    Feature,
}

/// One variable or array field in a report layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HidField {
    pub bit_offset: u32,
    pub bit_size: u32,
    pub count: u32,
    pub flags: u32,
    pub variable: bool,
    pub usages: Vec<u32>,
}

/// One report ID and channel shape declared by a HID descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HidReportLayout {
    pub report_type: HidReportType,
    pub report_id: u8,
    pub payload_bits: u32,
    pub fields: Vec<HidField>,
}

impl HidReportLayout {
    #[must_use]
    pub const fn minimum_payload_bytes(&self) -> usize {
        self.payload_bits.div_ceil(8) as usize
    }
}

/// A generic parsed HID descriptor. It does not assign gamepad semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HidDescriptor {
    pub items: Vec<HidItem>,
    pub layouts: Vec<HidReportLayout>,
}

/// Semantic descriptor-validation failures with reproducible byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidValidationError {
    GlobalPopUnderflow { offset: usize },
    UnclosedGlobalPush { depth: usize },
    CollectionEndWithoutStart { offset: usize },
    UnclosedCollection { depth: usize },
    CollectionDepthExceeded { offset: usize },
    ZeroReportId { offset: usize },
    MissingReportSize { offset: usize },
    MissingReportCount { offset: usize },
    ArithmeticOverflow { offset: usize },
}

impl fmt::Display for HidValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid HID descriptor: {self:?}")
    }
}

impl Error for HidValidationError {}

/// Parse a descriptor into lossless items without assigning device semantics.
///
/// # Errors
///
/// Returns an error only when the raw item stream is truncated.
pub fn parse_items(bytes: &[u8]) -> Result<Vec<HidItem>, HidParseError> {
    let mut items = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        let prefix = bytes[offset];
        if prefix == 0xfe {
            if offset + 2 >= bytes.len() {
                return Err(HidParseError::TruncatedLongItem {
                    offset,
                    expected: 2,
                });
            }
            let size = usize::from(bytes[offset + 1]);
            let tag = bytes[offset + 2];
            let end = offset + 3 + size;
            if end > bytes.len() {
                return Err(HidParseError::TruncatedLongItem {
                    offset,
                    expected: size,
                });
            }
            items.push(HidItem {
                offset,
                kind: HidItemKind::Long { tag },
                data: bytes[offset + 3..end].to_vec(),
            });
            offset = end;
            continue;
        }
        let size = match prefix & 0x03 {
            3 => 4,
            value => usize::from(value),
        };
        let end = offset + 1 + size;
        if end > bytes.len() {
            return Err(HidParseError::TruncatedShortItem {
                offset,
                expected: size,
            });
        }
        let item_type = match (prefix >> 2) & 0x03 {
            0 => HidItemType::Main,
            1 => HidItemType::Global,
            2 => HidItemType::Local,
            _ => HidItemType::Reserved,
        };
        items.push(HidItem {
            offset,
            kind: HidItemKind::Short {
                item_type,
                tag: prefix >> 4,
            },
            data: bytes[offset + 1..end].to_vec(),
        });
        offset = end;
    }
    Ok(items)
}

impl HidDescriptor {
    /// Parse and validate a generic HID descriptor.
    ///
    /// # Errors
    ///
    /// Returns raw-stream or semantic descriptor errors.
    pub fn parse(bytes: &[u8]) -> Result<Self, HidDescriptorError> {
        let items = parse_items(bytes).map_err(HidDescriptorError::Parse)?;
        let layouts = build_layouts(&items).map_err(HidDescriptorError::Validation)?;
        Ok(Self { items, layouts })
    }

    #[must_use]
    pub fn layout(&self, report_type: HidReportType, report_id: u8) -> Option<&HidReportLayout> {
        self.layouts
            .iter()
            .find(|layout| layout.report_type == report_type && layout.report_id == report_id)
    }
}

/// Error returned by [`HidDescriptor::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidDescriptorError {
    Parse(HidParseError),
    Validation(HidValidationError),
}

impl fmt::Display for HidDescriptorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl Error for HidDescriptorError {}

#[derive(Debug, Clone, Default)]
struct GlobalState {
    usage_page: Option<u32>,
    report_size: Option<u32>,
    report_count: Option<u32>,
    report_id: u8,
}

fn value(data: &[u8]) -> u32 {
    data.iter()
        .enumerate()
        .fold(0_u32, |result, (index, byte)| {
            result | (u32::from(*byte) << (index * 8))
        })
}

// One pass intentionally mirrors HID's stateful item grammar.
#[allow(clippy::too_many_lines)]
fn build_layouts(items: &[HidItem]) -> Result<Vec<HidReportLayout>, HidValidationError> {
    let mut global = GlobalState::default();
    let mut global_stack = Vec::new();
    let mut usages = Vec::new();
    let mut collection_depth = 0;
    let mut layouts = BTreeMap::<(HidReportType, u8), HidReportLayout>::new();
    for item in items {
        let HidItemKind::Short { item_type, tag } = item.kind else {
            continue;
        };
        let item_value = value(&item.data);
        match (item_type, tag) {
            (HidItemType::Global, 0) => global.usage_page = Some(item_value),
            (HidItemType::Global, 7) => global.report_size = Some(item_value),
            (HidItemType::Global, 8) => {
                if item_value == 0 || item_value > u32::from(u8::MAX) {
                    return Err(HidValidationError::ZeroReportId {
                        offset: item.offset,
                    });
                }
                global.report_id =
                    u8::try_from(item_value).map_err(|_| HidValidationError::ZeroReportId {
                        offset: item.offset,
                    })?;
            }
            (HidItemType::Global, 9) => global.report_count = Some(item_value),
            (HidItemType::Global, 10) => global_stack.push(global.clone()),
            (HidItemType::Global, 11) => {
                global = global_stack
                    .pop()
                    .ok_or(HidValidationError::GlobalPopUnderflow {
                        offset: item.offset,
                    })?;
            }
            (HidItemType::Local, 0) => {
                let usage = global
                    .usage_page
                    .map_or(item_value, |page| (page << 16) | item_value);
                usages.push(usage);
            }
            (HidItemType::Main, 10) => {
                collection_depth += 1;
                if collection_depth > MAX_COLLECTION_DEPTH {
                    return Err(HidValidationError::CollectionDepthExceeded {
                        offset: item.offset,
                    });
                }
                usages.clear();
            }
            (HidItemType::Main, 12) => {
                if collection_depth == 0 {
                    return Err(HidValidationError::CollectionEndWithoutStart {
                        offset: item.offset,
                    });
                }
                collection_depth -= 1;
                usages.clear();
            }
            (HidItemType::Main, 8 | 9 | 11) => {
                let report_type = match tag {
                    8 => HidReportType::Input,
                    9 => HidReportType::Output,
                    _ => HidReportType::Feature,
                };
                let bit_size = global
                    .report_size
                    .ok_or(HidValidationError::MissingReportSize {
                        offset: item.offset,
                    })?;
                let count = global
                    .report_count
                    .ok_or(HidValidationError::MissingReportCount {
                        offset: item.offset,
                    })?;
                let layout = layouts
                    .entry((report_type, global.report_id))
                    .or_insert_with(|| HidReportLayout {
                        report_type,
                        report_id: global.report_id,
                        payload_bits: 0,
                        fields: Vec::new(),
                    });
                let width =
                    bit_size
                        .checked_mul(count)
                        .ok_or(HidValidationError::ArithmeticOverflow {
                            offset: item.offset,
                        })?;
                let next = layout.payload_bits.checked_add(width).ok_or(
                    HidValidationError::ArithmeticOverflow {
                        offset: item.offset,
                    },
                )?;
                layout.fields.push(HidField {
                    bit_offset: layout.payload_bits,
                    bit_size,
                    count,
                    flags: item_value,
                    variable: item_value & 0x02 != 0,
                    usages: std::mem::take(&mut usages),
                });
                layout.payload_bits = next;
            }
            (HidItemType::Main, _) => usages.clear(),
            _ => {}
        }
    }
    if !global_stack.is_empty() {
        return Err(HidValidationError::UnclosedGlobalPush {
            depth: global_stack.len(),
        });
    }
    if collection_depth != 0 {
        return Err(HidValidationError::UnclosedCollection {
            depth: collection_depth,
        });
    }
    Ok(layouts.into_values().collect())
}

/// Opaque raw report bytes. Constructing an Output frame does not transmit it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HidReportFrame {
    pub report_type: HidReportType,
    pub bytes: Vec<u8>,
}

/// A report selected against a descriptor layout without normalizing bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramedHidReport<'a> {
    pub report_id: u8,
    pub layout: &'a HidReportLayout,
    pub payload: &'a [u8],
    pub trailing: &'a [u8],
}

/// Report framing failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HidReportError {
    MissingReportId,
    UnknownReportId {
        report_type: HidReportType,
        report_id: u8,
    },
    ShortReport {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for HidReportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid HID report: {self:?}")
    }
}

impl Error for HidReportError {}

/// Select a descriptor layout for opaque report bytes.
///
/// # Errors
///
/// Returns an error when the report ID is absent, unknown, or too short.
pub fn frame_report<'a>(
    descriptor: &'a HidDescriptor,
    frame: &'a HidReportFrame,
) -> Result<FramedHidReport<'a>, HidReportError> {
    let layouts = descriptor
        .layouts
        .iter()
        .filter(|layout| layout.report_type == frame.report_type)
        .collect::<Vec<_>>();
    let has_ids = layouts.iter().any(|layout| layout.report_id != 0);
    let (report_id, payload) = if has_ids {
        let Some((id, payload)) = frame.bytes.split_first() else {
            return Err(HidReportError::MissingReportId);
        };
        (*id, payload)
    } else {
        (0, frame.bytes.as_slice())
    };
    let layout =
        descriptor
            .layout(frame.report_type, report_id)
            .ok_or(HidReportError::UnknownReportId {
                report_type: frame.report_type,
                report_id,
            })?;
    let expected = layout.minimum_payload_bytes();
    if payload.len() < expected {
        return Err(HidReportError::ShortReport {
            expected,
            actual: payload.len(),
        });
    }
    Ok(FramedHidReport {
        report_id,
        layout,
        payload: &payload[..expected],
        trailing: &payload[expected..],
    })
}

/// Evidence used to pair synthetic HID and evdev endpoints. It contains no path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointEvidence {
    pub identity: FixtureDeviceIdentity,
    pub transport: TransportFixture,
    /// Opaque, fixture-local topology evidence. It is never a host path.
    pub topology_token: Option<u64>,
    /// Opaque, fixture-local connection evidence. It is never a serial or address.
    pub connection_token: Option<u64>,
}

/// Serializable, non-identifying USB/Bluetooth device identity evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureDeviceIdentity {
    pub bus_type: u16,
    pub vendor_id: u16,
    pub product_id: u16,
    pub version: u16,
}

/// Serializable subset of transport values used in shared fixture evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportFixture {
    Usb,
    Bluetooth,
    Virtual,
    Other,
    Unknown,
}

impl From<Transport> for TransportFixture {
    fn from(value: Transport) -> Self {
        match value {
            Transport::Usb => Self::Usb,
            Transport::Bluetooth => Self::Bluetooth,
            Transport::Virtual => Self::Virtual,
            Transport::Other(_) => Self::Other,
            Transport::Unknown => Self::Unknown,
        }
    }
}

/// One fixture endpoint identified only by a deterministic fixture-local index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointFixture {
    pub fixture_index: usize,
    pub evidence: EndpointEvidence,
}

/// Result of pairing one HID fixture endpoint to evdev fixture endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointPairing {
    Unique { evdev_index: usize },
    Ambiguous { evdev_indices: Vec<usize> },
    Unmatched,
}

/// Pair endpoints only from explicit supplied evidence, never local path order.
#[must_use]
pub fn pair_endpoint(hid: &EndpointFixture, evdev: &[EndpointFixture]) -> EndpointPairing {
    let mut scored = evdev
        .iter()
        .filter_map(|candidate| {
            evidence_score(&hid.evidence, &candidate.evidence).map(|score| (score, candidate))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.fixture_index.cmp(&right.1.fixture_index))
    });
    let Some((best, _)) = scored.first() else {
        return EndpointPairing::Unmatched;
    };
    let indices = scored
        .iter()
        .take_while(|(score, _)| score == best)
        .map(|(_, endpoint)| endpoint.fixture_index)
        .collect::<Vec<_>>();
    if indices.len() == 1 {
        EndpointPairing::Unique {
            evdev_index: indices[0],
        }
    } else {
        EndpointPairing::Ambiguous {
            evdev_indices: indices,
        }
    }
}

fn evidence_score(left: &EndpointEvidence, right: &EndpointEvidence) -> Option<u8> {
    if left.identity != right.identity || left.transport != right.transport {
        return None;
    }
    let mut score = 1;
    if left.topology_token.is_some() && left.topology_token == right.topology_token {
        score += 2;
    }
    if left.connection_token.is_some() && left.connection_token == right.connection_token {
        score += 4;
    }
    Some(score)
}

/// Versioned, sanitized HID replay fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HidFixture {
    pub format_version: u32,
    pub synthetic: bool,
    pub descriptor: Vec<u8>,
    pub reports: Vec<HidReportFrame>,
    pub hid_endpoint: EndpointFixture,
    pub evdev_endpoints: Vec<EndpointFixture>,
}

impl HidFixture {
    #[must_use]
    pub const fn new(
        synthetic: bool,
        descriptor: Vec<u8>,
        reports: Vec<HidReportFrame>,
        hid_endpoint: EndpointFixture,
        evdev_endpoints: Vec<EndpointFixture>,
    ) -> Self {
        Self {
            format_version: 1,
            synthetic,
            descriptor,
            reports,
            hid_endpoint,
            evdev_endpoints,
        }
    }

    /// Serialize a shared fixture. Endpoint evidence must remain opaque and sanitized.
    ///
    /// # Errors
    ///
    /// Returns serialization errors from `serde_json`.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse a fixture with unknown fields rejected.
    ///
    /// # Errors
    ///
    /// Returns schema or JSON errors from `serde_json`.
    pub fn from_json(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> Vec<u8> {
        vec![
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x05, // Usage (Game Pad)
            0xa1, 0x01, // Collection (Application)
            0x85, 0x01, // Report ID 1
            0x75, 0x08, // Report Size 8
            0x95, 0x02, // Report Count 2
            0x09, 0x30, // Usage X
            0x09, 0x31, // Usage Y
            0x81, 0x02, // Input (Data,Var,Abs)
            0xc0, // End Collection
        ]
    }

    fn endpoint(fixture_index: usize, topology: Option<u64>) -> EndpointFixture {
        EndpointFixture {
            fixture_index,
            evidence: EndpointEvidence {
                identity: FixtureDeviceIdentity {
                    bus_type: 3,
                    vendor_id: 1,
                    product_id: 2,
                    version: 1,
                },
                transport: TransportFixture::Usb,
                topology_token: topology,
                connection_token: None,
            },
        }
    }

    #[test]
    fn parses_lossless_short_and_long_items() {
        let items = parse_items(&[0x05, 0x01, 0xfe, 0x02, 0x99, 0xaa, 0xbb]).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].offset, 2);
        assert_eq!(items[1].data, vec![0xaa, 0xbb]);
    }

    #[test]
    fn rejects_truncated_items() {
        assert!(matches!(
            parse_items(&[0x75]),
            Err(HidParseError::TruncatedShortItem { .. })
        ));
        assert!(matches!(
            parse_items(&[0xfe, 2]),
            Err(HidParseError::TruncatedLongItem { .. })
        ));
    }

    #[test]
    fn builds_report_layout_and_retains_trailing_bytes() {
        let parsed = HidDescriptor::parse(&descriptor()).unwrap();
        assert_eq!(parsed.layouts[0].payload_bits, 16);
        assert!(parsed.layouts[0].fields[0].variable);
        assert_eq!(
            parsed.layouts[0].fields[0].usages,
            [0x0001_0030, 0x0001_0031]
        );
        let frame = HidReportFrame {
            report_type: HidReportType::Input,
            bytes: vec![1, 10, 20, 30],
        };
        let framed = frame_report(&parsed, &frame).unwrap();
        assert_eq!(framed.payload, [10, 20]);
        assert_eq!(framed.trailing, [30]);
    }

    #[test]
    fn detects_invalid_global_and_collection_state() {
        assert!(matches!(
            HidDescriptor::parse(&[0xb4]),
            Err(HidDescriptorError::Validation(
                HidValidationError::GlobalPopUnderflow { .. }
            ))
        ));
        assert!(matches!(
            HidDescriptor::parse(&[0xa4]),
            Err(HidDescriptorError::Validation(
                HidValidationError::UnclosedGlobalPush { .. }
            ))
        ));
        assert!(matches!(
            HidDescriptor::parse(&[0xc0]),
            Err(HidDescriptorError::Validation(
                HidValidationError::CollectionEndWithoutStart { .. }
            ))
        ));
        assert!(matches!(
            HidDescriptor::parse(&[0x85, 0]),
            Err(HidDescriptorError::Validation(
                HidValidationError::ZeroReportId { .. }
            ))
        ));
        assert!(matches!(
            HidDescriptor::parse(&[0x95, 1, 0x81, 2]),
            Err(HidDescriptorError::Validation(
                HidValidationError::MissingReportSize { .. }
            ))
        ));
        assert!(matches!(
            HidDescriptor::parse(&[0x75, 8, 0x81, 2]),
            Err(HidDescriptorError::Validation(
                HidValidationError::MissingReportCount { .. }
            ))
        ));
        assert!(matches!(
            HidDescriptor::parse(&[0x77, 0xff, 0xff, 0xff, 0xff, 0x95, 2, 0x81, 2]),
            Err(HidDescriptorError::Validation(
                HidValidationError::ArithmeticOverflow { .. }
            ))
        ));
        let excessive_collections = [0xa1, 1].repeat(MAX_COLLECTION_DEPTH + 1);
        assert!(matches!(
            HidDescriptor::parse(&excessive_collections),
            Err(HidDescriptorError::Validation(
                HidValidationError::CollectionDepthExceeded { .. }
            ))
        ));
    }

    #[test]
    fn rejects_unknown_and_short_reports() {
        let parsed = HidDescriptor::parse(&descriptor()).unwrap();
        assert!(matches!(
            frame_report(
                &parsed,
                &HidReportFrame {
                    report_type: HidReportType::Input,
                    bytes: vec![]
                }
            ),
            Err(HidReportError::MissingReportId)
        ));
        assert!(matches!(
            frame_report(
                &parsed,
                &HidReportFrame {
                    report_type: HidReportType::Input,
                    bytes: vec![2, 0, 0]
                }
            ),
            Err(HidReportError::UnknownReportId { .. })
        ));
    }

    #[test]
    fn preserves_array_fields_without_gamepad_mapping() {
        let descriptor = HidDescriptor::parse(&[0x75, 8, 0x95, 1, 0x81, 0]).unwrap();
        assert!(!descriptor.layouts[0].fields[0].variable);
        assert!(descriptor.layouts[0].fields[0].usages.is_empty());
    }

    #[test]
    fn pairing_is_deterministic_and_reports_ambiguity() {
        let hid = endpoint(0, Some(7));
        assert_eq!(
            pair_endpoint(&hid, &[endpoint(1, Some(7))]),
            EndpointPairing::Unique { evdev_index: 1 }
        );
        assert_eq!(
            pair_endpoint(&endpoint(0, None), &[endpoint(2, None), endpoint(1, None)]),
            EndpointPairing::Ambiguous {
                evdev_indices: vec![1, 2]
            }
        );
        assert_eq!(
            pair_endpoint(
                &hid,
                &[EndpointFixture {
                    fixture_index: 1,
                    evidence: EndpointEvidence {
                        transport: TransportFixture::Bluetooth,
                        ..endpoint(1, Some(8)).evidence
                    },
                }]
            ),
            EndpointPairing::Unmatched
        );
    }

    #[test]
    fn fixture_rejects_unknown_private_fields() {
        let fixture = HidFixture::new(
            true,
            descriptor(),
            Vec::new(),
            endpoint(0, None),
            Vec::new(),
        );
        let json = fixture.to_json_pretty().unwrap();
        assert_eq!(HidFixture::from_json(&json).unwrap(), fixture);
        assert!(
            HidFixture::from_json(&json.replacen('}', ",\"device_path\":\"/dev/hidraw0\"}", 1))
                .is_err()
        );
    }
}
