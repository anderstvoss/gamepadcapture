# gamepad-capture architecture specification

## Purpose

`gamepad-capture` is the physical-controller mechanical layer beneath IUCM and
`virtualgamepad`. It discovers controller-facing host interfaces, preserves the
host's native evidence and event streams, obtains requested exclusive access,
and offers conservative capture-profile candidates.

It does **not** normalize controls into a universal layout, select routing
semantics, remap controls, create virtual devices, or claim that an emulated or
compatible controller is the first-party device it resembles.

```text
physical hardware
  -> host interfaces / native reports
  -> gamepad-capture: evidence, access lease, capture profile, native events
  -> IUCM: semantic normalization and routing policy
  -> virtualgamepad: requested virtual target
```

## Non-negotiable invariants

1. The raw/native representation remains available after profile selection.
2. Automatic detection makes the least-assumptive supported choice. An unknown
   controller falls back to SDL **Joystick**, not SDL Gamepad.
3. A profile is a mechanical reading strategy, never an assertion of physical
   model equivalence or feature parity.
4. Capture and output profiles are independent. A user may force a generic
   source into a DualSense-shaped output target; unavailable source functions
   remain unavailable.
5. Identity is evidence with a stability grade, not a universally safe key.
6. Required exclusivity fails closed. A shared fallback is explicit.
7. Output writes are capability-gated. Unknown feature/output reports are never
   probed by default.
8. Every behavior supported by a hardware recording becomes a deterministic
   regression fixture before a profile is called validated.

## Core model

### Device, interface, and source

- `PhysicalDeviceId` identifies a probable physical controller. It may group
  multiple host interfaces, such as gamepad, touch, motion, audio, or vendor
  interfaces.
- `SourceId` identifies one capture endpoint. It is connection scoped and must
  not be used as a persisted controller assignment.
- `DeviceDescriptor` is the native discovery record. It preserves transport,
  VID/PID/version, host name, topology/unique identity, provenance, controls,
  and identity stability.
- `EventBatch` preserves native frame boundaries and values. IUCM performs all
  semantic translation after this boundary.

The present Linux evdev backend implements this first layer. Future HID and SDL
backends add complementary evidence; they do not replace evdev data.

### Current implementation status

The repository currently implements Linux evdev discovery/capture, identity
stability, exclusive-grab reporting, session lifecycle handling, and a pure
initial profile selector. It does **not** yet implement hidraw, HID descriptors
or report preservation, SDL runtime enumeration, Windows/macOS backends,
protocol decoders, or output channels. References to those elements below are
target architecture, not claims of present support.

### Evidence and capabilities

The expanded descriptor must retain evidence separately from conclusions:

```text
identity:       VID/PID, revision, names, serial/unique ID, physical path
transport:      USB, Bluetooth, receiver, virtual, unknown
interfaces:     HID, evdev, audio, vendor-specific, SDL-visible endpoint
descriptors:    HID report descriptor and hash when available
reports:        input/output/feature channel metadata and raw recording handles
driver state:   OS driver/backend and access outcome
capabilities:   declared | observed | verified | unsupported | unknown
```

Capabilities carry their evidence source: descriptor, known protocol, passive
recording, controlled active test, or user profile. A product name, VID/PID, or
marketing claim is not enough to mark a feature as verified.

### Profile selection

`CaptureProfile` defines a reader/decoder strategy. Initial passive matching is
narrow: identity, transport, interface layout, descriptor hash, report shape,
and explicit known-protocol evidence. The selection result returns every
candidate, selected candidate, confidence, and evidence.

Profile order is:

1. verified native profile;
2. strongly evidenced protocol-family profile;
3. descriptor-driven generic HID profile;
4. SDL Joystick fallback.

The final fallback exposes `axis[n]`, `button[n]`, `hat[n]`, and equivalent SDL
controls exactly as SDL exposes them. It does not adopt SDL's standard Gamepad
mapping. SDL may conceal device-specific information on some platforms, so a
native backend remains the authority whenever it is available.

`ProfileSelectionMode::Forced` is a user policy override. It must preserve the
automatic candidates and native evidence for diagnostics. A forced profile may
be incompatible; it is an instruction to attempt that mechanical interpretation,
not a guarantee of correctness.

### Capture profile versus output profile

```text
Capture profile: "How do I read this attached source?"
Output profile:  "What virtual device shape does IUCM intend to produce?"
Route:           "How do the available source controls drive that target?"
```

`DeviceProfileAssignment` transports a requested output profile without
implementing mapping. The IUCM/manager owns target realization and must report
unmapped, synthesized, and unavailable functions. `gamepad-capture` must never
reject an intentionally different output shape merely because it is not a
physical match.

## Platform backends

Platform backends conform to `CaptureProvider` and should be independently
testable through record/replay fixtures.

| Backend | Authority | Intended role |
| --- | --- | --- |
| Linux evdev | Kernel input controls and event frames | Current baseline and exclusive grab |
| Linux hidraw | HID descriptor plus raw input/output/feature reports | Native protocol preservation |
| SDL Joystick | Portable generic axes/buttons/hats surface | Universal fallback, not canonical raw capture |
| Windows raw HID/GameInput | Device interfaces and rich Windows metadata | Future Windows native path |
| macOS IOHID | HID elements/reports and device properties | Future macOS native path |

Backends must report what access they obtained. They must not silently convert a
failed exclusive grab into a shared session except under `PreferExclusive`.

## Output and safety boundary

The library may later expose controller output channels, but the initial safety
policy is fixed:

- `KnownSafe`: documented and fixture-validated writes.
- `KnownProtocol`: known encoding; device support still stated explicitly.
- `Opaque`: raw channel available only through an opt-in expert API.
- `Unavailable`: no host path or feature known.

Read-only discovery and passive capture are safe defaults. Blind output or
feature-report fuzzing is outside normal operation because it can alter modes,
pairing, calibration, or firmware state.

## Module layout

```text
src/
  model.rs          stable native data types
  identity.rs       identity construction and stability policy
  session.rs        provider contract, lifecycle, exclusive access outcome
  profile.rs        pure profile catalog, selection, forcing contract
  linux.rs          current evdev provider
  hid/              future raw HID descriptor/report backend
  sdl/              future SDL Joystick fallback backend
  protocols/        future native and family decoders
tests/
  fixtures/         recorded manifests, descriptors, and report sequences
  replay/           deterministic backend and profile tests
  contract/         public API and safety-property tests
tools/
  capture-lab/      future fixture recorder and guided hardware runner
```

The future directories are planned boundaries; they are not promises that the
first implementation will expose all of them simultaneously.

## Acceptance definition

A first-class device profile is complete only when it has: documented evidence
and capabilities; USB and Bluetooth results where both apply; input and safe
output fixture coverage; disconnect/reconnect and exclusivity coverage;
captured fixtures committed or securely referenced; and a hardware validation
record tied to a firmware/OS/driver matrix.
