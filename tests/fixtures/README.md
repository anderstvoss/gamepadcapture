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
