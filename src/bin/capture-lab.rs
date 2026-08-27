//! Read-only fixture bundle recorder for the Linux capture-lab workflow.
//!
//! It records sanitized evdev discovery, controls, complete native event frames,
//! and caller-supplied operator observations. HID reports and controller output
//! remain unavailable until the corresponding validated backend exists.

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
    let mut args = env::args().skip(1);
    let mode = args.next().ok_or_else(usage)?;
    let output = PathBuf::from(args.next().ok_or("missing output path")?);
    let remaining = args.collect::<Vec<_>>();
    let (polls, observations) = parse_options(&remaining)?;
    let recording = match mode.as_str() {
        "--synthetic" => FixtureRecording::sanitized_capture(
            FixtureManifest::sanitized(true, &[]),
            &[],
            &[],
            &BTreeMap::new(),
            observations,
        ),
        "--live" => live_recording(polls, observations)?,
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

fn usage() -> String {
    "usage: capture-lab (--synthetic | --live) OUTPUT.json [poll-count] [--observation TEXT]..."
        .to_owned()
}

fn parse_options(args: &[String]) -> Result<(usize, Vec<String>), String> {
    let mut polls = 250;
    let mut observations = Vec::new();
    let mut index = 0;
    if let Some(value) = args.first().filter(|value| !value.starts_with('-')) {
        polls = value
            .parse::<usize>()
            .map_err(|_| "poll-count must be a positive integer".to_owned())?;
        index = 1;
    }
    while index < args.len() {
        if args[index] != "--observation" {
            return Err(usage());
        }
        let observation = args.get(index + 1).ok_or_else(usage)?;
        observations.push(observation.clone());
        index += 2;
    }
    if polls == 0 {
        return Err("poll-count must be a positive integer".to_owned());
    }
    Ok((polls, observations))
}

#[cfg(target_os = "linux")]
fn live_recording(polls: usize, observations: Vec<String>) -> Result<FixtureRecording, String> {
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
    Ok(FixtureRecording::sanitized_capture(
        manifest,
        &snapshot.devices,
        &batches,
        &indices,
        observations,
    ))
}

#[cfg(not(target_os = "linux"))]
fn live_recording(_polls: usize, _observations: Vec<String>) -> Result<FixtureRecording, String> {
    Err("live capture-lab discovery currently requires Linux evdev".to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_options;

    #[test]
    fn options_preserve_multiple_operator_observations() {
        let args = [
            "3".to_owned(),
            "--observation".to_owned(),
            "idle baseline".to_owned(),
            "--observation".to_owned(),
            "button press".to_owned(),
        ];
        assert_eq!(
            parse_options(&args).unwrap(),
            (
                3,
                vec!["idle baseline".to_owned(), "button press".to_owned()]
            )
        );
    }

    #[test]
    fn options_reject_zero_polls_and_unknown_flags() {
        assert!(parse_options(&["0".to_owned()]).is_err());
        assert!(parse_options(&["--unexpected".to_owned()]).is_err());
    }
}
