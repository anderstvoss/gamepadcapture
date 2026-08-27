#![no_main]

use gamepad_capture::hid::{frame_report, HidDescriptor, HidReportFrame, HidReportType};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = data.first().map_or(0, |byte| usize::from(*byte) % (data.len() + 1));
    let (descriptor_bytes, report_bytes) = data.split_at(split);
    if let Ok(descriptor) = HidDescriptor::parse(descriptor_bytes) {
        for report_type in [
            HidReportType::Input,
            HidReportType::Output,
            HidReportType::Feature,
        ] {
            let _ = frame_report(
                &descriptor,
                &HidReportFrame {
                    report_type,
                    bytes: report_bytes.to_vec(),
                },
            );
        }
    }
});
