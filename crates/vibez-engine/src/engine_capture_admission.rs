//! Boundary-scoped Capture admission preserves local-failure priority over applied validity.

use super::AudioEngine;

#[derive(Clone, Copy)]
pub(super) enum CaptureAdmissionFacts {
    LocalDeviceFailure,
    AppliedPlan { valid: bool },
}

impl CaptureAdmissionFacts {
    pub(super) fn refusal_reason(self) -> Option<&'static str> {
        match self {
            Self::LocalDeviceFailure => {
                Some("Capture requires recovery or reload of the failed device")
            }
            Self::AppliedPlan { valid: false } => {
                Some("Capture requires a valid applied audio configuration")
            }
            Self::AppliedPlan { valid: true } => None,
        }
    }
}

impl AudioEngine {
    pub(super) fn observe_capture_admission(&self) -> CaptureAdmissionFacts {
        // Global output blockers have a broader policy than Capture admission.
        // Earlier commands in this drain can change both owners and local failure.
        if self.local_device_failure_pending() {
            CaptureAdmissionFacts::LocalDeviceFailure
        } else {
            CaptureAdmissionFacts::AppliedPlan {
                valid: self.compensation_plan.applied_plan_is_valid(),
            }
        }
    }
}
