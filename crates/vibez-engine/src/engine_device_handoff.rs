//! Own device handoff identity, scope, suspension and retained intent across transfer attempts.

use vibez_core::id::{EffectId, TrackId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RecoveryScope {
    LocalRecovery,
    ProjectRestart,
}

impl RecoveryScope {
    pub(super) fn from_recovery_request(local_recovery: bool) -> Self {
        if local_recovery {
            Self::LocalRecovery
        } else {
            Self::ProjectRestart
        }
    }
}

#[derive(PartialEq, Eq)]
enum ProjectSuspension {
    Clear,
    Suspended,
}

#[derive(Clone, Copy)]
pub(super) struct PendingDeviceReconfiguration {
    pub(super) handoff_id: u64,
    pub(super) track_id: TrackId,
    pub(super) effect_id: Option<EffectId>,
    pub(super) position: usize,
    pub(super) bypass: bool,
    pub(super) scope: RecoveryScope,
}

impl PendingDeviceReconfiguration {
    pub(super) fn is_local_recovery(&self) -> bool {
        self.scope == RecoveryScope::LocalRecovery
    }
}

pub(super) enum ParameterRetention {
    Stored,
    Full,
}

pub(super) struct DeviceHandoffState {
    last_issued_identity: u64,
    pending: Option<PendingDeviceReconfiguration>,
    // Publication and restoration move metadata before project suspension changes.
    suspension: ProjectSuspension,
    // Canceling a token clears intent; its backing allocation survives until engine teardown.
    parameters: Vec<(usize, f32)>,
}

impl Default for DeviceHandoffState {
    fn default() -> Self {
        Self {
            last_issued_identity: 0,
            pending: None,
            suspension: ProjectSuspension::Clear,
            parameters: Vec::with_capacity(2048),
        }
    }
}

impl DeviceHandoffState {
    pub(super) fn is_suspended(&self) -> bool {
        self.suspension == ProjectSuspension::Suspended
    }

    pub(super) fn publication_succeeded(&mut self, scope: RecoveryScope) {
        self.suspension = match scope {
            RecoveryScope::LocalRecovery => ProjectSuspension::Clear,
            RecoveryScope::ProjectRestart => ProjectSuspension::Suspended,
        };
    }

    pub(super) fn clear_suspension(&mut self) {
        self.suspension = ProjectSuspension::Clear;
    }

    pub(super) fn candidate_identity(&self) -> Option<u64> {
        self.last_issued_identity.checked_add(1)
    }

    pub(super) fn record_issued_identity(&mut self, identity: u64) {
        self.last_issued_identity = identity;
    }

    pub(super) fn pending(&self) -> Option<PendingDeviceReconfiguration> {
        self.pending
    }

    pub(super) fn track_transfer(&mut self, pending: PendingDeviceReconfiguration) {
        self.pending = Some(pending);
    }

    pub(super) fn retain_edited_target(&mut self, pending: PendingDeviceReconfiguration) {
        self.pending = Some(pending);
    }

    pub(super) fn take_for_restoration(&mut self) -> PendingDeviceReconfiguration {
        self.pending.take().expect("validated device handoff")
    }

    pub(super) fn cancel_pending(&mut self) {
        self.pending = None;
        self.parameters.clear();
    }

    pub(super) fn retain_parameter(&mut self, index: usize, value: f32) -> ParameterRetention {
        if let Some(previous) = self.parameters.iter_mut().find(|(id, _)| *id == index) {
            previous.1 = value;
        } else if self.parameters.len() < self.parameters.capacity() {
            self.parameters.push((index, value));
        } else {
            return ParameterRetention::Full;
        }
        ParameterRetention::Stored
    }

    pub(super) fn apply_retained_parameters(
        &mut self,
        mut set: impl FnMut(usize, f32) -> bool,
    ) -> bool {
        let mut rejected = false;
        for &(index, value) in &self.parameters {
            rejected |= !set(index, value);
        }
        self.parameters.clear();
        rejected
    }

    #[cfg(test)]
    pub(super) fn retained_parameters(&self) -> &[(usize, f32)] {
        &self.parameters
    }

    #[cfg(test)]
    pub(super) fn retained_parameter_capacity(&self) -> usize {
        self.parameters.capacity()
    }

    #[cfg(test)]
    pub(super) fn last_issued_identity(&self) -> u64 {
        self.last_issued_identity
    }

    #[cfg(test)]
    pub(super) fn exhaust_identity_for_test(&mut self) {
        self.last_issued_identity = u64::MAX;
    }
}
