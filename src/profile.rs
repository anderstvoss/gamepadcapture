//! Conservative capture-profile selection.
//!
//! Profiles describe how a physical source is *read*. They are not IUCM
//! normalization and never replace the native [`DeviceDescriptor`] or event
//! stream. A separate output-profile request is carried as opaque policy so a
//! later routing layer may intentionally target a different virtual shape.

use crate::{DeviceDescriptor, Transport};

/// Stable identifier for a capture or output profile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileId(String);

impl ProfileId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How much semantic knowledge a capture profile contributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureProfileFamily {
    /// A fully understood device-specific protocol.
    Native,
    /// A protocol family such as XUSB, GIP, or Switch controller mode.
    Protocol,
    /// A descriptor-driven generic HID view.
    GenericHid,
    /// SDL's unnormalized joystick surface: axes, buttons, hats, and balls.
    SdlJoystick,
}

/// Confidence in an automatic choice. It is evidence, not a promise of feature parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetectionConfidence {
    Fallback,
    Tentative,
    Strong,
    /// Confirmed by a sanitized physical recording and its regression fixture.
    ///
    /// Passive matching never produces this level. A product name, VID/PID, or
    /// transport match is evidence for a candidate, not proof of feature parity.
    Verified,
}

/// A deliberately narrow passive matcher. Rich protocol probes are added by platform backends.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileMatch {
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub transport: Option<Transport>,
}

impl ProfileMatch {
    fn matches(&self, device: &DeviceDescriptor) -> bool {
        self.vendor_id
            .is_none_or(|value| value == device.identity.vendor_id)
            && self
                .product_id
                .is_none_or(|value| value == device.identity.product_id)
            && self.transport.is_none_or(|value| value == device.transport)
    }

    fn specificity(&self) -> u8 {
        u8::from(self.vendor_id.is_some())
            + u8::from(self.product_id.is_some())
            + u8::from(self.transport.is_some())
    }
}

/// A registered profile and its passive identity rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureProfile {
    pub id: ProfileId,
    pub family: CaptureProfileFamily,
    pub matches: Vec<ProfileMatch>,
}

/// Why a profile was selected or offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileEvidence {
    IdentityMatch,
    TransportMatch(Transport),
    GenericFallback,
}

/// One possible mechanical interpretation of the attached source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCandidate {
    pub profile_id: ProfileId,
    pub family: CaptureProfileFamily,
    pub confidence: DetectionConfidence,
    pub evidence: Vec<ProfileEvidence>,
}

/// A conservative, ordered automatic choice and every candidate that justified it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSelection {
    pub selected: ProfileCandidate,
    pub candidates: Vec<ProfileCandidate>,
}

impl ProfileSelection {
    /// Apply an explicit capture-profile choice without losing automatic evidence.
    #[must_use]
    pub fn force(self, profile_id: ProfileId) -> ProfileSelectionMode {
        ProfileSelectionMode::Forced {
            profile_id,
            automatic: self,
        }
    }
}

/// Whether the capture profile came from detection or an explicit user override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileSelectionMode {
    Auto(ProfileSelection),
    /// A user-requested reader plus the automatic result it replaced.
    ///
    /// The automatic selection remains available for diagnostics and later
    /// manager policy. The forced ID may be incompatible with the source; it is
    /// an instruction to attempt that mechanical interpretation, not a claim
    /// that the source has matching capabilities.
    Forced {
        profile_id: ProfileId,
        automatic: ProfileSelection,
    },
}

impl ProfileSelectionMode {
    /// Returns the automatic result, including when a user override is active.
    #[must_use]
    pub const fn automatic(&self) -> &ProfileSelection {
        match self {
            Self::Auto(selection)
            | Self::Forced {
                automatic: selection,
                ..
            } => selection,
        }
    }

    /// Returns the explicit capture choice, if one replaced automatic selection.
    #[must_use]
    pub const fn forced_profile(&self) -> Option<&ProfileId> {
        match self {
            Self::Auto(_) => None,
            Self::Forced { profile_id, .. } => Some(profile_id),
        }
    }
}

/// Policy passed to the later routing layer without performing any routing here.
///
/// `requested_output_profile` is intentionally not validated against the source.
/// A fight stick may be routed to a DualSense-shaped virtual target, for example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProfileAssignment {
    pub capture: ProfileSelectionMode,
    pub requested_output_profile: Option<ProfileId>,
}

/// Pure, deterministic selector suitable for fixtures and agent-authored profile catalogs.
#[derive(Debug, Clone)]
pub struct AutoProfileDetector {
    profiles: Vec<CaptureProfile>,
    sdl_joystick: ProfileId,
}

impl AutoProfileDetector {
    /// Creates a selector. `sdl_joystick` is always available as the final fallback.
    #[must_use]
    pub fn new(profiles: Vec<CaptureProfile>, sdl_joystick: ProfileId) -> Self {
        Self {
            profiles,
            sdl_joystick,
        }
    }

    /// Select the most specific passive profile; ties remain visible to the caller.
    ///
    /// This passive detector returns only [`DetectionConfidence::Tentative`],
    /// [`DetectionConfidence::Strong`], or [`DetectionConfidence::Fallback`].
    /// Fixture-backed hardware evidence is required before another layer may
    /// represent a candidate as verified.
    #[must_use]
    pub fn detect(&self, device: &DeviceDescriptor) -> ProfileSelection {
        let mut candidates: Vec<_> = self
            .profiles
            .iter()
            .filter_map(|profile| {
                let rule = profile
                    .matches
                    .iter()
                    .filter(|rule| rule.matches(device))
                    .max_by_key(|rule| rule.specificity())?;
                let specificity = rule.specificity();
                if specificity == 0 {
                    return None;
                }
                let confidence = match specificity {
                    2 | 3 => DetectionConfidence::Strong,
                    _ => DetectionConfidence::Tentative,
                };
                let mut evidence = Vec::new();
                if rule.vendor_id.is_some() || rule.product_id.is_some() {
                    evidence.push(ProfileEvidence::IdentityMatch);
                }
                if rule.transport.is_some() {
                    evidence.push(ProfileEvidence::TransportMatch(device.transport));
                }
                Some(ProfileCandidate {
                    profile_id: profile.id.clone(),
                    family: profile.family,
                    confidence,
                    evidence,
                })
            })
            .collect();
        candidates.sort_by(|left, right| {
            right
                .confidence
                .cmp(&left.confidence)
                .then_with(|| left.profile_id.cmp(&right.profile_id))
        });
        candidates.push(ProfileCandidate {
            profile_id: self.sdl_joystick.clone(),
            family: CaptureProfileFamily::SdlJoystick,
            confidence: DetectionConfidence::Fallback,
            evidence: vec![ProfileEvidence::GenericFallback],
        });
        ProfileSelection {
            selected: candidates[0].clone(),
            candidates,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        ControllerClass, DeviceIdentity, DeviceProvenance, IdentityStability, PhysicalDeviceId,
        SourceId,
    };

    fn device(vendor_id: u16, product_id: u16) -> DeviceDescriptor {
        DeviceDescriptor {
            physical_id: PhysicalDeviceId::new("physical"),
            source_id: SourceId::new("source"),
            reported_name: "fixture".into(),
            identity: DeviceIdentity {
                bus_type: 3,
                vendor_id,
                product_id,
                version: 1,
            },
            transport: Transport::Usb,
            provenance: DeviceProvenance::Physical,
            identity_stability: IdentityStability::ConnectionOnly,
            class: ControllerClass::Gamepad,
            device_path: PathBuf::from("/fixture"),
            physical_path: None,
            unique_id: None,
            controls: Vec::new(),
        }
    }

    #[test]
    fn exact_profile_is_preferred_but_sdl_remains_visible() {
        let detector = AutoProfileDetector::new(
            vec![CaptureProfile {
                id: ProfileId::new("sony-dualsense-usb"),
                family: CaptureProfileFamily::Native,
                matches: vec![ProfileMatch {
                    vendor_id: Some(0x054c),
                    product_id: Some(0x0ce6),
                    transport: Some(Transport::Usb),
                }],
            }],
            ProfileId::new("sdl-joystick"),
        );
        let selection = detector.detect(&device(0x054c, 0x0ce6));
        assert_eq!(selection.selected.profile_id.as_str(), "sony-dualsense-usb");
        assert_eq!(selection.selected.confidence, DetectionConfidence::Strong);
        assert!(
            selection
                .candidates
                .iter()
                .any(|candidate| candidate.profile_id.as_str() == "sdl-joystick")
        );
    }

    #[test]
    fn unknown_device_falls_back_to_sdl_joystick() {
        let detector = AutoProfileDetector::new(Vec::new(), ProfileId::new("sdl-joystick"));
        let selection = detector.detect(&device(0xffff, 0x0001));
        assert_eq!(selection.selected.family, CaptureProfileFamily::SdlJoystick);
        assert_eq!(selection.selected.confidence, DetectionConfidence::Fallback);
    }

    #[test]
    fn output_profile_is_independent_of_capture_choice() {
        let automatic = AutoProfileDetector::new(Vec::new(), ProfileId::new("sdl-joystick"))
            .detect(&device(0xffff, 0x0001));
        let assignment = DeviceProfileAssignment {
            capture: automatic.force(ProfileId::new("sdl-joystick")),
            requested_output_profile: Some(ProfileId::new("dualsense")),
        };
        assert_eq!(
            assignment.capture.forced_profile().unwrap().as_str(),
            "sdl-joystick"
        );
        assert_eq!(
            assignment.capture.automatic().selected.profile_id.as_str(),
            "sdl-joystick"
        );
        assert_eq!(
            assignment.requested_output_profile.unwrap().as_str(),
            "dualsense"
        );
    }

    #[test]
    fn transport_specific_profiles_are_selected_only_for_matching_transport() {
        let detector = AutoProfileDetector::new(
            vec![
                CaptureProfile {
                    id: ProfileId::new("usb"),
                    family: CaptureProfileFamily::Protocol,
                    matches: vec![ProfileMatch {
                        vendor_id: Some(1),
                        product_id: Some(2),
                        transport: Some(Transport::Usb),
                    }],
                },
                CaptureProfile {
                    id: ProfileId::new("bluetooth"),
                    family: CaptureProfileFamily::Protocol,
                    matches: vec![ProfileMatch {
                        vendor_id: Some(1),
                        product_id: Some(2),
                        transport: Some(Transport::Bluetooth),
                    }],
                },
            ],
            ProfileId::new("sdl-joystick"),
        );
        let mut bluetooth = device(1, 2);
        bluetooth.transport = Transport::Bluetooth;
        let selection = detector.detect(&bluetooth);
        assert_eq!(selection.selected.profile_id.as_str(), "bluetooth");
        assert_eq!(selection.candidates.len(), 2);
    }

    #[test]
    fn ties_are_sorted_and_remain_ambiguous_to_the_caller() {
        let detector = AutoProfileDetector::new(
            vec!["zeta", "alpha"]
                .into_iter()
                .map(|id| CaptureProfile {
                    id: ProfileId::new(id),
                    family: CaptureProfileFamily::Protocol,
                    matches: vec![ProfileMatch {
                        vendor_id: Some(1),
                        product_id: Some(2),
                        transport: None,
                    }],
                })
                .collect(),
            ProfileId::new("sdl-joystick"),
        );
        let selection = detector.detect(&device(1, 2));
        assert_eq!(selection.selected.profile_id.as_str(), "alpha");
        assert_eq!(
            selection
                .candidates
                .iter()
                .map(|candidate| candidate.profile_id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta", "sdl-joystick"]
        );
    }

    #[test]
    fn passive_detector_never_claims_verified_evidence() {
        let detector = AutoProfileDetector::new(
            vec![CaptureProfile {
                id: ProfileId::new("exact"),
                family: CaptureProfileFamily::Native,
                matches: vec![ProfileMatch {
                    vendor_id: Some(1),
                    product_id: Some(2),
                    transport: Some(Transport::Usb),
                }],
            }],
            ProfileId::new("sdl-joystick"),
        );
        assert!(
            detector
                .detect(&device(1, 2))
                .candidates
                .iter()
                .all(|candidate| candidate.confidence != DetectionConfidence::Verified)
        );
    }
}
