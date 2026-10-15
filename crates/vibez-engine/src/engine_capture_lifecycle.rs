//! Own current Capture activity independently of retained presentation notifications.

#[derive(Default)]
enum CaptureActivity {
    #[default]
    Idle,
    Active,
}

#[derive(Default)]
pub(super) struct CaptureLifecycle {
    activity: CaptureActivity,
}

impl CaptureLifecycle {
    pub(super) fn begin(&mut self) {
        self.activity = CaptureActivity::Active;
    }

    pub(super) fn end(&mut self) {
        self.activity = CaptureActivity::Idle;
    }

    pub(super) fn is_active(&self) -> bool {
        matches!(self.activity, CaptureActivity::Active)
    }
}
