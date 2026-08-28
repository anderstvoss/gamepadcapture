# Regression fixtures

This directory is reserved for deterministic controller evidence used by replay
tests. Do not add a profile solely from a device name or VID/PID table.

Each hardware-derived fixture set should contain a sanitized manifest, its
transport and backend metadata, descriptors where permitted, labeled input
traces, and only safe output traces. See
[`docs/HARDWARE_VALIDATION.md`](../../docs/HARDWARE_VALIDATION.md) for the
required capture-lab bundle and promotion process.

Synthetic fixtures are encouraged for malformed reports, lifecycle failures,
ambiguous profile matches, serial-less duplicate devices, and forced-profile
configuration. They must clearly state that they are synthetic.

`synthetic-manifest.json` is the minimal version-1 public fixture contract. It
contains no host paths, topology, serial, or Bluetooth-address data and is
parsed by the integration test suite.

`synthetic-hid.json` is a separate version-1 experimental HID descriptor and
opaque report fixture. It contains only fixture-local endpoint indices and
opaque numeric evidence tokens; it must never acquire paths, serials,
Bluetooth addresses, or source IDs.

`synthetic-xbox-display.json` drives the display-only Xbox-shaped tester demo.
It is synthetic and must not be used to claim physical controller validation.

`xbox-series-usb-observed-input.json` is a minimal, sanitized observation from
one USB capture session. It supports regression tests for native replay and the
display-only demo, but does not verify a capability, promote a profile, or add
output behavior. Its timestamps have been removed and it contains no local
paths, persistent source identifiers, serials, or Bluetooth addresses.
