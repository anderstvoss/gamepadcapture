//! Read-only fixture manifest recorder for the Linux capture-lab workflow.
//!
//! It deliberately records discovery metadata only. HID reports and controller
//! output remain unavailable until the corresponding validated backend exists.

use std::{
    collections::BTreeMap, env, fs, path::PathBuf, process::ExitCode, thread, time::Duration,
};

use gamepad_capture::{
    CaptureEvent, CaptureProvider, CaptureSession, FixtureManifest, FixtureRecording, SourceId,
};

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
        .ok_or("usage: capture-lab (--synthetic | --live) OUTPUT.json [poll-count]")?;
    let output = PathBuf::from(args.next().ok_or("missing output path")?);
    let polls = args
        .next()
        .map(|value| {
            value
                .to_string_lossy()
                .parse::<usize>()
                .map_err(|_| "poll-count must be a positive integer".to_owned())
        })
        .transpose()?
        .unwrap_or(250);
    if polls == 0 || args.next().is_some() {
        return Err(
            "usage: capture-lab (--synthetic | --live) OUTPUT.json [poll-count]".to_owned(),
        );
    }
    let recording = match mode.to_string_lossy().as_ref() {
        "--synthetic" => FixtureRecording::sanitized(
            FixtureManifest::sanitized(true, &[]),
            &[],
            &BTreeMap::new(),
        ),
        "--live" => live_recording(polls)?,
        _ => return Err("mode must be --synthetic or --live".to_owned()),
    };
    fs::write(
        output,
        recording
            .to_json_pretty()
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
fn live_recording(polls: usize) -> Result<FixtureRecording, String> {
    let mut provider = gamepad_capture::linux::EvdevProvider::new();
    let snapshot = provider.enumerate().map_err(|error| error.to_string())?;
    if !snapshot.issues.is_empty() {
        eprintln!(
            "capture-lab: {} source(s) could not be inspected",
            snapshot.issues.len()
        );
    }
    let manifest = FixtureManifest::sanitized(false, &snapshot.devices);
    let indices = snapshot
        .devices
        .iter()
        .enumerate()
        .map(|(index, device)| (device.source_id.clone(), index))
        .collect::<BTreeMap<SourceId, usize>>();
    let mut session = CaptureSession::new(provider, gamepad_capture::AccessMode::Shared);
    let mut batches = Vec::new();
    for _ in 0..polls {
        for event in session.poll().map_err(|error| error.to_string())? {
            if let CaptureEvent::Input(batch) = event {
                batches.push(batch);
            }
        }
        thread::sleep(Duration::from_millis(4));
    }
    Ok(FixtureRecording::sanitized(manifest, &batches, &indices))
}

#[cfg(not(target_os = "linux"))]
fn live_recording(_polls: usize) -> Result<FixtureRecording, String> {
    Err("live capture-lab discovery currently requires Linux evdev".to_owned())
}
