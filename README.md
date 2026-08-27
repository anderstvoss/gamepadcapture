# gamepad-capture

`gamepad-capture` is a small Rust library for discovering Linux physical
controllers, describing the controls they actually expose, and capturing their
native evdev input. It is intended to be the physical-input layer beneath a
later IUCM mapper and manager; it does not normalize, remap, route, or create
virtual controllers.

The public model deliberately preserves:

- the kernel-reported interface name and input identity (bus, VID, PID, version);
- the transport reported by the input bus rather than assuming USB;
- every native key, axis, switch, LED, and force-feedback code;
- absolute-axis minimum, maximum, current, fuzz, flat, and resolution values;
- kernel synchronization-frame boundaries and native event values;
- explicit `SYN_DROPPED` loss reporting; incomplete frames are discarded;
- separate physical-device and event-source identities for compound devices;
- identity stability (`Hardware`, `Topology`, or `ConnectionOnly`) so anonymous
  devices are never mistaken for safe persisted assignments;
- whether capture is shared, exclusive, or an explicitly reported fallback.

It also establishes the profile-selection seam needed for broad controller
support: native profiles win only on explicit evidence, SDL **Joystick** is the
portable generic fallback, and a requested virtual output profile is opaque
policy for the later manager rather than capture-layer normalization.

The SDL fallback is currently a **selection contract**, not an implemented SDL
backend. The existing runtime backend is Linux evdev; SDL Joystick integration
is a planned, separately feature-gated phase.

When a caller forces a capture profile, the assignment retains the complete
automatic profile selection (including candidates and evidence). A forced
choice is therefore inspectable policy, not a replacement for the native
evidence or an assertion of capability parity. Requested virtual-output
profiles remain independent opaque policy for a later manager.

## Boundary

```text
physical device -> platform evidence -> capture profile -> native event batches -> IUCM -> virtualgamepad
                    descriptor/reports   native/HID/SDL         no normalization   mapping
                    identity             forced override allowed
                    exclusive grab
```

Names such as `BTN_SOUTH` are diagnostic labels. The numeric `(event_type,
code)` pair and advertised range are the contract. A later IUCM layer decides
whether a physical control means `face_bottom`, `capture`, a paddle, or
something controller-specific.

## Example

```rust,no_run
#[cfg(target_os = "linux")]
fn main() -> Result<(), gamepad_capture::CaptureError> {
    use gamepad_capture::{AccessMode, CaptureEvent, CaptureSession};
    use gamepad_capture::linux::EvdevProvider;

    let mut capture = CaptureSession::new(EvdevProvider::new(), AccessMode::Exclusive);
    loop {
        for event in capture.poll()? {
            match event {
                CaptureEvent::Connected { device, access } => {
                    println!("{} ({access:?})", device.reported_name);
                }
                CaptureEvent::Input(batch) => println!("{batch:?}"),
                CaptureEvent::Disconnected { source_id, .. } => {
                    println!("disconnected: {source_id}");
                }
                CaptureEvent::SourceError { source_id, error } => {
                    eprintln!("{source_id}: {error}");
                }
                CaptureEvent::DiscoveryError(issue) => eprintln!("{issue:?}"),
                _ => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
```

Dropping a session drops its evdev handles and releases any exclusive grabs.
Opening `/dev/input/event*` requires host permissions supplied by the embedding
application or deployment environment. The library does not modify udev rules,
permissions, kernel modules, or host configuration.

## Validation

```bash
cargo run --example inspect
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test
```

The profile-selection invariant can additionally be fuzzed on Unix with a
nightly Rust toolchain:

```bash
cargo install cargo-fuzz --locked
cargo fuzz run profile_selection --fuzz-dir fuzz -- -max_total_time=60
cargo fuzz run hid_descriptor_report --fuzz-dir fuzz -- -max_total_time=60
```

The default-off `hid` feature provides experimental, pure descriptor parsing,
opaque report framing, and synthetic endpoint pairing. It performs no hidraw
I/O and cannot write controller output:

```bash
cargo test --features hid
```

The optional native diagnostics window is available without changing library
consumers:

```bash
cargo run --features tester --bin gamepad-tester
```

It is a passive evidence viewer: it does not normalize inputs, create virtual
controllers, write controller output, or change host permissions.

Create a safe synthetic capture-lab manifest without controller hardware:

```bash
cargo run --features fixture-lab --bin capture-lab -- --synthetic manifest.json
```

`capture-lab --live recording.json [poll-count] [--observation TEXT]...`
performs bounded shared Linux evdev capture (250 polls by default) and stores
native frames, control metadata, a labeled capture segment, and reviewed
operator observations by sanitized source index. Its versioned JSON omits
paths, serials, unique IDs, and Bluetooth addresses; it never writes controller
output. Do not include personal or connection-identifying information in an
observation intended for sharing.
Linux hardware validation should additionally cover two identical serial-less
controllers, Bluetooth reconnects, compound controller interfaces, hot-unplug
during input, exclusive-grab contention, and an evdev ring-buffer overrun.

## Project documents

- [`ARCHITECTURE_SPEC.md`](docs/ARCHITECTURE_SPEC.md) defines the stable library
  boundary, evidence model, profile policy, and platform seams.
- [`IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md) is the phased,
  agent-executable plan, including gates that require no physical hardware.
- [`HARDWARE_VALIDATION.md`](docs/HARDWARE_VALIDATION.md) defines the compact
  benchmark-device lab and the recordings that become regression fixtures.

## Governance

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing or reporting a
security concern. This project is licensed under [AGPL-3.0-only](LICENSE).
