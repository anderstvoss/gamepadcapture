use crate::{DeviceIdentity, PhysicalDeviceId, SourceId, Transport};

/// Build an inspectable physical identity, preferring a hardware unique ID and
/// then topology. The returned value is stable only as far as the supplied
/// kernel identifiers are stable.
#[must_use]
pub fn build_physical_id(
    identity: &DeviceIdentity,
    transport: Transport,
    unique_id: Option<&str>,
    physical_path: Option<&str>,
) -> PhysicalDeviceId {
    let discriminator = unique_id
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("uniq:{}", escape(value)))
        .or_else(|| {
            physical_path
                .filter(|value| !value.trim().is_empty())
                .map(|value| format!("phys:{}", escape(value)))
        })
        .unwrap_or_else(|| "anonymous".to_owned());
    PhysicalDeviceId::new(format!(
        "{:04x}:{:04x}:{:04x}:{}:{}",
        identity.bus_type,
        identity.vendor_id,
        identity.product_id,
        transport_label(transport),
        discriminator
    ))
}

/// Build the identity of one evdev source. Event node numbers are connection
/// scoped, which is appropriate here but not for persisted physical identity.
#[must_use]
pub fn build_source_id(physical_id: &PhysicalDeviceId, device_path: &str) -> SourceId {
    SourceId::new(format!("{}:source:{}", physical_id, escape(device_path)))
}

fn transport_label(transport: Transport) -> String {
    match transport {
        Transport::Usb => "usb".to_owned(),
        Transport::Bluetooth => "bluetooth".to_owned(),
        Transport::Virtual => "virtual".to_owned(),
        Transport::Other(value) => format!("bus-{value:04x}"),
        Transport::Unknown => "unknown".to_owned(),
    }
}

fn escape(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/') {
                vec![char::from(byte)]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> DeviceIdentity {
        DeviceIdentity {
            bus_type: 3,
            vendor_id: 0x045e,
            product_id: 0x0b13,
            version: 1,
        }
    }

    #[test]
    fn serial_identity_survives_event_node_change() {
        let physical = build_physical_id(&identity(), Transport::Usb, Some("serial 1"), Some("a"));
        assert_eq!(
            physical,
            build_physical_id(&identity(), Transport::Usb, Some("serial 1"), Some("b"))
        );
        assert_ne!(
            build_source_id(&physical, "/dev/input/event3"),
            build_source_id(&physical, "/dev/input/event9")
        );
    }

    #[test]
    fn topology_distinguishes_identical_serial_less_devices() {
        assert_ne!(
            build_physical_id(&identity(), Transport::Usb, None, Some("usb-1-2/input0")),
            build_physical_id(&identity(), Transport::Usb, None, Some("usb-1-3/input0"))
        );
    }
}
