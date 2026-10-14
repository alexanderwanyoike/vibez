//! Boundary-scoped observations separate project mute policy from live device reads.

use super::AudioEngine;

#[derive(Clone, Copy)]
pub(super) enum ResidentRestartDemand {
    Clear,
    Nonlocal,
}

#[cfg(test)]
impl ResidentRestartDemand {
    pub(super) fn is_pending(self) -> bool {
        matches!(self, Self::Nonlocal)
    }
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
        self.observe_project_processing_with(|| self.observe_resident_restart())
    }

    #[cfg(test)]
    pub(super) fn quiescent_project_processing_muted(
        &self,
        resident: ResidentRestartDemand,
    ) -> bool {
        self.observe_project_processing_with(|| resident).is_muted()
    }

    fn observe_project_processing_with(
        &self,
        resident: impl FnOnce() -> ResidentRestartDemand,
    ) -> ProjectProcessingFacts {
        // An owned blocker suppresses the live scan. Its variant cannot be
        // mistaken for an observed clear resident restart demand.
        if self.device_handoff.is_suspended() {
            ProjectProcessingFacts::Suspended
        } else if self.compensation_plan.blocks_processing() {
            ProjectProcessingFacts::CompensationBlocked
        } else if self.presentation_state.is_faulted() {
            ProjectProcessingFacts::PresentationFaulted
        } else {
            ProjectProcessingFacts::ResidentRestart(resident())
        }
    }

    pub(super) fn observe_resident_restart(&self) -> ResidentRestartDemand {
        if self
            .tracks
            .iter()
            .chain(&self.buses)
            .chain(std::iter::once(&self.master))
            .any(|track| {
                track.effects.iter().any(|slot| {
                    slot.effect.reconfiguration_requested()
                        && !slot.effect.processing_recovery_requested()
                }) || track.instrument.as_ref().is_some_and(|instrument| {
                    instrument.reconfiguration_requested()
                        && !instrument.processing_recovery_requested()
                })
            })
        {
            ResidentRestartDemand::Nonlocal
        } else {
            ResidentRestartDemand::Clear
        }
    }
}
