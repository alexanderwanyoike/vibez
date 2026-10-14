//! Boundary-scoped observations separate project mute policy from live device reads.

use super::AudioEngine;

#[derive(Clone, Copy)]
enum ResidentRestartDemand {
    Clear,
    Nonlocal,
}

#[derive(Clone, Copy)]
enum ProjectProcessingFacts {
    Suspended,
    CompensationBlocked,
    PresentationFaulted,
    ResidentRestart(ResidentRestartDemand),
}

impl ProjectProcessingFacts {
    fn is_muted(self) -> bool {
        !matches!(self, Self::ResidentRestart(ResidentRestartDemand::Clear))
    }
}

impl AudioEngine {
    pub(super) fn project_processing_muted(&self) -> bool {
        self.observe_project_processing().is_muted()
    }

    fn observe_project_processing(&self) -> ProjectProcessingFacts {
        // An owned blocker suppresses the live scan. Its variant cannot be
        // mistaken for an observed clear resident restart demand.
        if self.device_handoff.is_suspended() {
            ProjectProcessingFacts::Suspended
        } else if self.compensation_plan.blocks_processing() {
            ProjectProcessingFacts::CompensationBlocked
        } else if self.presentation_state.is_faulted() {
            ProjectProcessingFacts::PresentationFaulted
        } else {
            ProjectProcessingFacts::ResidentRestart(if self.reconfiguration_pending() {
                ResidentRestartDemand::Nonlocal
            } else {
                ResidentRestartDemand::Clear
            })
        }
    }
}
