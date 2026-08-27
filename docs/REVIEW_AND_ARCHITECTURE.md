# Review findings and capture architecture

## Earlier attempts

`GamepadManager` proved the end-to-end route—enumeration, SDL/evdev input,
exclusive grabs, IUCM translation, uinput output, diagnostics, and frontends—but
placed those responsibilities inside one service. Its two input paths also had
different identity and value behavior: SDL synthesized evdev-like events and
normalized some axes, while evdev retained kernel values. That makes the mapper
dependent on which backend happened to capture the controller.

`GamepadRouter` improved subsystem boundaries and introduced immutable device
connection records, backend protocols, capture policies, and a clearer IUCM
pipeline. The remaining capture seam still mixed discovery, model detection,
fingerprinting, haptics, Steam suppression, and routing-oriented callbacks.
Notable risks visible in that implementation were a USB default when evdev did
not expose transport through its wrapper, path-based identity for devices
without serials, polling hotplug, and broad exception swallowing around device
metadata and output operations.

`virtualgamepad` provides the strongest architectural precedent: preserve native
controller semantics, make target selection explicit, expose immutable surface
metadata, do not silently fall back, and keep providers controller-neutral. The
capture library mirrors those principles on the input side.

## Decisions

1. Native event frames remain the authoritative capture record. IUCM owns
   semantic translation and scaling; `gamepad-capture` never calls an axis
   "left stick" by inference. Capture profiles may add a conservative mechanical
   interpretation, while retaining the original evidence and controls.
2. `PhysicalDeviceId` and `SourceId` are different. This permits a later manager
   to group a controller's gamepad, touchpad, motion, and companion interfaces
   without conflating their event streams.
3. Identity uses kernel unique identity first and physical topology second.
   Serial-less devices with neither remain explicitly anonymous; no unstable
   event path is represented as a stable physical fingerprint. The descriptor
   marks identity stability so the manager cannot safely persist an anonymous
   assignment by accident.
4. Exclusive capture is requested at open time. Required exclusivity fails
   closed. Preferred exclusivity can fall back, but the caller receives
   `SharedFallback` and can enforce its own safety policy.
5. Hotplug reconciliation and source failures are library concerns. A failure
   on one controller does not terminate input from the others.
6. The Linux provider reports virtual provenance rather than hard-coding names
   of virtual-controller libraries. The embedding manager chooses its own
   feedback-loop policy.
7. Host policy is out of scope. The library never writes udev rules, changes
   permissions, loads modules, or invokes privileged helpers.
8. Detection and output shape are independent. Automatic profile selection is
   evidence based; an unknown controller has an SDL **Joystick** fallback rather
   than an SDL Gamepad-normalized identity. A later manager may force either a
   capture profile or a different virtual output profile, with limitations made
   explicit instead of hidden.

## Intended integration seam

The manager consumes `CaptureEvent`. On `Connected`, it selects an IUCM input
definition using exact identity, profile candidates, and capability metadata.
For each `Input` batch, it maps native controls into IUCM semantics and sends
controller-native updates to `virtualgamepad`. Capture failures and shared
fallbacks remain visible as manager state rather than being logged and ignored.

The active specification and delivery plan are in
[`ARCHITECTURE_SPEC.md`](ARCHITECTURE_SPEC.md),
[`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md), and
[`HARDWARE_VALIDATION.md`](HARDWARE_VALIDATION.md).
