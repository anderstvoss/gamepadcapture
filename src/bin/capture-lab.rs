//! Read-only fixture manifest recorder for the Linux capture-lab workflow.
//!
//! It deliberately records discovery metadata only. HID reports and controller
//! output remain unavailable until the corresponding validated backend exists.

use std::{env, fs, path::PathBuf, process::ExitCode};

use gamepad_capture::{CaptureProvider, FixtureManifest};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("capture-lab: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os();
    let _program = args.next();
    let mode = args
        .next()
        .ok_or("usage: capture-lab (--synthetic | --live) OUTPUT.json")?;
    let output = PathBuf::from(args.next().ok_or("missing output path")?);
    if args.next().is_some() {
        return Err("usage: capture-lab (--synthetic | --live) OUTPUT.json".to_owned());
    }
    let manifest = match mode.to_string_lossy().as_ref() {
        "--synthetic" => FixtureManifest::sanitized(true, &[]),
        "--live" => live_manifest()?,
        _ => return Err("mode must be --synthetic or --live".to_owned()),
    };
    fs::write(
        output,
        manifest
            .to_json_pretty()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn live_manifest() -> Result<FixtureManifest, String> {
    let snapshot = gamepad_capture::linux::EvdevProvider::new()
        .enumerate()
        .map_err(|error| error.to_string())?;
    if !snapshot.issues.is_empty() {
        eprintln!(
            "capture-lab: {} source(s) could not be inspected",
            snapshot.issues.len()
        );
    }
    Ok(FixtureManifest::sanitized(false, &snapshot.devices))
}

#[cfg(not(target_os = "linux"))]
fn live_manifest() -> Result<FixtureManifest, String> {
    Err("live capture-lab discovery currently requires Linux evdev".to_owned())
}
