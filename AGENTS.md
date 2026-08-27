# Gamepad Capture agent guide

This directory is the `gamepad-capture` Rust crate. It supplies the physical
controller capture layer for a later IUCM manager and `virtualgamepad`; it is
not the manager, normalization layer, or virtual-controller implementation.

## Read first

Read these files in order before changing code:

1. [`README.md`](README.md) for the current public boundary and runtime status.
2. [`docs/ARCHITECTURE_SPEC.md`](docs/ARCHITECTURE_SPEC.md) for non-negotiable
   design constraints.
3. [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md) for phase gates.
4. [`docs/HARDWARE_VALIDATION.md`](docs/HARDWARE_VALIDATION.md) before adding a
   first-class device profile or output support.
5. [`tests/fixtures/README.md`](tests/fixtures/README.md) before adding test
   recordings or synthetic fixtures.

## Current state

Implemented today:

- Linux evdev discovery, native event-frame capture, hotplug reconciliation,
  source error isolation, and shared/exclusive access outcomes.
- Identity construction with hardware/topology/connection-only stability.
- Native control metadata and a small, pure profile selector.
- A nominal SDL Joystick fallback candidate. **SDL is not yet linked or used as
  a runtime backend.**

Not implemented yet: hidraw/raw report capture, HID descriptors, SDL runtime
enumeration, protocol-specific decoders, controller output channels, fixture
replay tooling, Windows, macOS, or IUCM routing.

## Invariants

- Preserve native evidence and event values. Do not normalize inputs into a
  common gamepad layout here.
- SDL **Joystick** is the generic fallback; do not make SDL Gamepad mapping the
  canonical source representation.
- Automatic profile selection is conservative. VID/PID, product name, or
  compatibility marketing never proves feature parity.
- A capture profile answers how to read the source. An output profile is an
  independent later-manager request. Do not implement routing in this crate.
- Unknown output and feature reports are unsafe to probe by default. Do not add
  blind writes, firmware-mode commands, pairing commands, or host permission
  changes.
- A hardware-derived capability is only `verified` when backed by a recorded,
  sanitized physical test and regression fixture.
- Keep `unsafe` out of portable core code; do not alter host configuration.

## First assignment

Complete the profile-selection/override contract without requiring any physical
device.

The architecture requires automatic candidates to remain inspectable even when
the user forces a capture profile. The current `ProfileSelectionMode::Forced`
only carries a profile ID, so it loses those automatic candidates. Fix that
contract before adding more device matchers or backends.

Deliverables:

1. Redesign the public profile-assignment types so a forced capture choice also
   preserves the preceding `ProfileSelection` for diagnostics and later policy.
2. Keep forced output profile selection independent and unconstrained by the
   source profile.
3. Add deterministic tests for unknown-device SDL-Joystick fallback,
   transport-specific selection, ties/ambiguity, a forced capture override that
   retains automatic candidates, and a deliberately mismatched output profile.
4. Ensure passive identity matches cannot claim `Verified`; that level is
   reserved for fixture-backed/hardware-validated evidence.
5. Update the architecture spec and README only where the public contract
   changes. Do not add an SDL dependency or live-device implementation in this
   assignment.

Acceptance: all tests run without `/dev/input`, a connected controller, or
network access. The implementation must be a small, backwards-considered public
API change with rustdoc explaining the evidence and override behavior.

## Security follow-up: Scorecard findings

The public repository's workflow runs pass, but OpenSSF Scorecard publishes five
open Code Scanning findings. Address them as a focused follow-up:

1. In `.github/workflows/scorecard.yml`, remove the top-level
   `security-events: write` permission and retain it only on the `analysis`
   job that uploads SARIF.
2. Decide and document an enforceable code-review policy; configure branch
   protection to match it before claiming the Scorecard code-review finding is
   resolved.
3. Establish a maintenance signal (for example, release/support expectations
   and responsive issue handling) instead of dismissing the maintenance
   finding without evidence.
4. Add an appropriate Rust fuzzing target and a reproducible local/CI entry
   point, or explicitly document why fuzzing is deferred for this early API.
5. Review the CII Best Practices criteria and address only requirements that
   fit this project's scope. Do not weaken security controls merely to raise a
   score.

Keep security findings open until the corresponding public evidence or
configuration is in place. Do not close alerts as false positives solely
because CI is green.

## Working practice

- Prefer pure parsers, deterministic fake providers, and fixtures over live
  hardware assumptions.
- Make a focused change; leave unrelated dirty work intact.
- Run the documented Cargo checks when a Rust toolchain is available. This
  environment previously had no installed Rust toolchain, so report that
  limitation rather than installing or changing toolchains without approval.
- Do not mark a device/profile validated or add device-specific output commands
  without completing the hardware-validation promotion path.
