//! Own Capture activity and aggregate local-failure episodes independently of presentation delivery.

#[derive(Default)]
enum CaptureActivity {
    #[default]
    Idle,
    Active,
}

#[derive(Default)]
enum LocalFailureEpisode {
    #[default]
    Clear,
    ClosureRequested,
}

#[derive(Default)]
pub(super) struct CaptureLifecycle {
    activity: CaptureActivity,
    local_failure_episode: LocalFailureEpisode,
}

impl CaptureLifecycle {
    pub(super) fn begin(&mut self) {
        self.activity = CaptureActivity::Active;
        self.local_failure_episode = LocalFailureEpisode::Clear;
    }

    pub(super) fn end(&mut self) {
        self.activity = CaptureActivity::Idle;
    }

    pub(super) fn is_active(&self) -> bool {
        matches!(self.activity, CaptureActivity::Active)
    }

    pub(super) fn needs_first_local_close(&self, local_failure: bool) -> bool {
        local_failure && !self.local_failure_closure_requested()
    }

    pub(super) fn record_local_failure(&mut self, local_failure: bool) {
        self.local_failure_episode = if local_failure {
            LocalFailureEpisode::ClosureRequested
        } else {
            LocalFailureEpisode::Clear
        };
    }

    pub(super) fn local_failure_closure_requested(&self) -> bool {
        matches!(
            self.local_failure_episode,
            LocalFailureEpisode::ClosureRequested
        )
    }
}
