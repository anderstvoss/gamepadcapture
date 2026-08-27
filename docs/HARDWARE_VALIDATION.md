# Physical input validation plan

## Objective

Use a small device lab to validate the widest range of host presentations,
while turning every useful observation into reusable regression fixtures. The
lab is a validation stage, not the place where architecture is improvised.

## Core benchmark set

| Device | Why it is a benchmark | Required connections |
| --- | --- | --- |
| Sony DualSense | Rich native HID: touch, IMU, LEDs, adaptive triggers, haptics, audio-adjacent interfaces | USB and Bluetooth |
| Microsoft Xbox Wireless Controller (Series) | Xbox-family presentation; compare Bluetooth with USB and, if available, official receiver | USB, Bluetooth, receiver optional |
| Nintendo Switch Pro Controller | Specialized Switch HID, IMU, proprietary rumble, USB/Bluetooth difference | USB and Bluetooth |
| 8BitDo Pro 2 or equivalent multi-mode controller | One physical unit exposing Switch, XInput, DInput, and/or Apple personalities | every supported PC-visible mode |

These four cover rich first-party HID, Xbox-family compatibility, Nintendo
protocol behavior, and the critical fact that one physical controller can
present several different host devices.

## Expansion devices

- A licensed reduced-feature controller, such as a PowerA Switch pad, validates
  that platform compatibility is not feature equivalence.
- A USB/Bluetooth adapter, such as an 8BitDo adapter, validates that the host
  sees the adapter's emulated personality rather than the upstream controller.
- A low-cost generic HID/DInput pad validates the true unknown-device path.
- A second identical serial-less controller validates identity and assignment
  safety.

## Lab host matrix

Linux is the first required host because the current backend is evdev. Record
kernel version, distribution, desktop/input stack, SDL version, and device
firmware for every run. Add Windows raw HID/GameInput and macOS IOHID only after
their backends exist; they repeat the same logical test cards rather than
creating unrelated behavior.

## Capture-lab artifact

Build a guided `capture-lab` tool before first physical validation. It must
produce a sanitized, versioned bundle containing:

```text
manifest.json       host/backend versions, device identity, topology redaction policy
interfaces.json     evdev/HID/SDL endpoints and their grouping evidence
descriptor.bin      raw HID descriptor when available
controls.json       native advertised controls and ranges
input/*.trace       timestamped raw/native report and event sequences
output/*.trace      only approved safe command/result pairs
observations.md     operator answers, visible effects, anomalies, firmware
```

Serials and Bluetooth addresses must be redacted or salted before a bundle is
shared. The original private bundle may be retained by the tester for repeat
matching but must not be required by CI.

## Per-connection test card

Run one card for each device × transport × operating mode. The tool should
prompt the operator, record an idle baseline, then collect each step in a
separate labeled segment.

1. Connect, enumerate, and record all host interfaces and selected profile
   candidates.
2. Confirm shared capture. Request required exclusive capture; verify that an
   independent observer receives or stops receiving events as expected.
3. Test every face/shoulder/menu/system button individually. Test D-pad
   cardinal and diagonal combinations where supported.
4. Sweep sticks through center, cardinal extremes, diagonals, and slow circles.
   Record advertised ranges, flat/fuzz, observed values, and recenter behavior.
5. Test each trigger independently, then both simultaneously. This detects
   APIs that collapse two triggers into one axis.
6. Test auxiliary controls: paddles, profile buttons, touch, gyro, accelerometer,
   microphone/mute, headset state, share/capture, and battery changes when present.
7. Repeat after disconnect/reconnect, sleep/wake, and mode switch. For Bluetooth,
   test re-pair/reconnect only when it is safe and authorized.
8. Run only profile-approved output tests: simple rumble, LEDs/player LEDs,
   and documented device-family commands. Restore a neutral state at the end.
9. Hot-unplug during active input; verify one source failure does not stop other
   sources and no stale identity persists.

## Validation criteria

For each run, compare the recording with the expected profile contract:

- The profile either identifies the presentation with documented evidence or
  selects generic SDL Joystick fallback.
- Every observed native control is preserved or the loss is named and linked to
  the responsible host API.
- Capability status is `verified` only after a recorded physical observation.
- Any output that succeeds is recorded with its exact supported scope; ignored
  or unavailable outputs remain explicit.
- A forced capture profile and deliberately different output profile can be
  represented without mutating raw evidence.

## Promotion path

1. Sanitize and commit the manifest, descriptor, and representative traces.
2. Add replay tests that assert parsing, profile selection, and safe output
   encoding against those artifacts.
3. Record deviations by transport, firmware, and OS driver.
4. Update the support matrix from `experimental` to `validated` only after CI
   passes using the new fixtures.

This loop is what makes the fifth first-class device cheaper than the first:
new support becomes profile evidence plus traces and tests, not a bespoke live
debugging effort.
