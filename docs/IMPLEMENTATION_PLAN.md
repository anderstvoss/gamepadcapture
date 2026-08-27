# Phased implementation plan

This plan is deliberately split between work agents can complete without access
to controllers and a short, disciplined hardware phase. Each phase has a hard
exit gate; later phases must not depend on unrecorded assumptions about a pad.

## Phase 0 — contract freeze and repository hygiene

**Goal:** make the boundary reviewable before protocol work begins.

- Maintain the architecture specification and hardware-validation plan.
- Define public-versioning policy and compatibility expectations for IDs,
  evidence, profile IDs, fixtures, and error reporting.
- Establish CI for formatting, linting, unit tests, documentation tests, and
  Linux compilation.
- Keep `unsafe` forbidden in the portable core; isolate any future platform FFI
  behind narrowly reviewed adapters.

**No-device exit gate:** a clean checkout passes `cargo fmt --check`, `cargo check --all-targets`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.

## Phase 1 — deterministic core and profile contract

**Goal:** establish evidence-preserving types that agents can evolve without
hardware.

- Complete native device/interface, descriptor, report-channel, capability, and
  evidence models without adding IUCM semantics.
- Expand the profile catalog format and matcher interfaces; keep matching
  passive and deterministic.
- Keep SDL Joystick as the mandatory final fallback candidate.
- Implement forced capture-profile and independent requested-output-profile
  policy as data only; no routing implementation.
- Create synthetic fixtures for exact match, ambiguous match, unknown device,
  forced mismatch, virtual device, serial-less duplicate, and transport change.

**No-device exit gate:** unit/property tests prove that every selection retains
the SDL fallback, no automatic profile is selected without explicit evidence,
and forced output choice is not constrained by capture shape.

## Phase 2 — Linux evdev hardening

**Goal:** make current native Linux capture robust and replayable.

- Finish source grouping, lifecycle state transitions, `SYN_DROPPED` handling,
  reconnect behavior, duplicate suppression policy hooks, and access-lease
  reporting.
- Add recorded-event replay provider and tests for hot-unplug during a frame,
  read failure isolation, repeated scans, and exclusive-grab contention.
- Define a stable serialized device manifest; no raw personal identifiers are
  required in shared fixtures.

**No-device exit gate:** all behavior runs against fake and recorded providers;
Linux-only tests do not need `/dev/input` access.

## Phase 3 — raw HID evidence layer

**Goal:** preserve what evdev cannot express.

- Add a Linux hidraw backend behind a feature flag.
- Record HID report descriptors, input/output/feature channel shapes, report
  IDs, and raw report frames.
- Implement descriptor parsing as a pure library with malformed-descriptor
  corpus and fuzz/property tests.
- Associate hidraw and evdev endpoints using evidence, never pathname guesses.
- Expose opaque output channels without automatic writes.

**No-device exit gate:** descriptor parser corpus, report framing, endpoint
grouping, and replay tests pass from checked-in synthetic/recorded fixtures.

## Phase 4 — SDL Joystick fallback backend

**Goal:** deliver a portable generic view without allowing it to become
normalization.

- Add SDL3 dynamically or feature-gated, subject to platform packaging review.
- Enumerate only SDL Joystick controls: axis index, button index, hat state,
  ball delta, GUID, name, and instance ID.
- Do not use SDL Gamepad mapping as the canonical source; it may be offered as
  diagnostics later.
- Link SDL source identity to native evidence only where confidence is explicit.
- Test SDL fixture/replay adapters and ensure lack of SDL never prevents a
  native backend from functioning.

**No-device exit gate:** simulated SDL inventory confirms fallback selection,
connection lifecycle, and source/output profile independence.

## Phase 5 — first-class protocol implementations

**Goal:** add native support one family at a time, with fixture-first design.

Implementation order is chosen for protocol diversity and future reuse:

1. DualSense: rich HID, USB/Bluetooth report differences, touch/motion/output.
2. Xbox family: XUSB/GIP/XInput presentation differences and fixed core surface.
3. Nintendo Switch Pro/Joy-Con family: specialized HID, IMU and Nintendo rumble.

For each family, an agent must first add a written profile contract, parser and
encoder tests from public protocol evidence or fixtures, negative tests, and an
explicit capability matrix. Hardware testing only validates and expands that
work; it must not be the first time parsing logic is designed.

**No-device exit gate:** each decoder/encoder passes replay fixtures, malformed
input tests, and output safety checks. No active command is sent by default.

## Phase 6 — profile catalog and generic-controller support

**Goal:** scale support without falsely identifying devices.

- Add profile catalog review rules: matcher evidence, expected controls, known
  omissions, outputs, OS/transport limitations, fixture links, and owner.
- Add generic HID, SDL Joystick, and optional XInput-family profiles.
- Support third-party multi-mode controllers as separate presented
  personalities, not one assumed device model.
- Support adapters as adapters; do not label the upstream pad as detected unless
  an explicit adapter side channel proves it.

**No-device exit gate:** catalog lint rejects broad VID-only claims unless they
are explicitly marked tentative; every profile has generic fallback behavior.

## Phase 7 — physical benchmark validation

Run the plan in [`HARDWARE_VALIDATION.md`](HARDWARE_VALIDATION.md). Convert every
approved recording into fixtures, then repeat Phase 5/6 gates with those
fixtures before calling a profile validated.

## Phase 8 — release readiness

**Goal:** make support credible and maintainable.

- Publish generated support matrix by OS, transport, backend, and capability
  status.
- Run compatibility tests against supported Rust versions and feature sets.
- Require fixture replay in CI and a regression issue template that requests a
  sanitized manifest/report capture.
- Document security, privacy, output safety, permissions, known limitations,
  and how to force/revert profiles.

**Release gate:** no profile may advertise first-class support without its
fixture corpus and hardware validation record.

## Autonomous-agent operating rules

Agents may autonomously edit pure models, parsers, fixture tooling, replay
backends, catalog linting, and documentation. They must not add a device to the
validated-support matrix, infer a device-specific write command from a name, or
change host permissions. Hardware-dependent uncertainty is recorded as a
capability gap with a planned lab test, not guessed away.
