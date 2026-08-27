#[cfg(target_os = "linux")]
fn main() -> Result<(), gamepad_capture::CaptureError> {
    use gamepad_capture::linux::EvdevProvider;
    use gamepad_capture::{CaptureProvider, ControlDescriptor};

    let snapshot = EvdevProvider::new().enumerate()?;
    for issue in snapshot.issues {
        eprintln!(
            "could not inspect {}: {}",
            issue.device_path.display(),
            issue.error
        );
    }
    for device in snapshot.devices {
        println!(
            "{}\n  source: {}\n  physical: {} ({:?})\n  VID:PID: {:04x}:{:04x}\n  transport: {:?}\n  provenance: {:?}",
            device.reported_name,
            device.device_path.display(),
            device.physical_id,
            device.identity_stability,
            device.identity.vendor_id,
            device.identity.product_id,
            device.transport,
            device.provenance,
        );
        for control in device.controls {
            match control {
                ControlDescriptor::AbsoluteAxis { code, name, info } => println!(
                    "    ABS {code:#06x} {name}: {}..={} current={} flat={} fuzz={} resolution={}",
                    info.minimum, info.maximum, info.current, info.flat, info.fuzz, info.resolution,
                ),
                other => println!("    {other:?}"),
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("gamepad-capture inspection requires Linux evdev");
}
