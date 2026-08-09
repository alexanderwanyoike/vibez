//! Runtime coordination for manual saves and debounced autosave.

use std::time::{Duration, Instant};

use crate::message::ProjectSaveToken;

pub(super) const AUTO_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub(super) struct SaveRuntime {
    document_id: u64,
    revision: u64,
    auto_save_deadline: Option<Instant>,
    in_flight: Option<ProjectSaveToken>,
    manual_save_queued: bool,
}

impl SaveRuntime {
    pub(super) fn project_changed(
        &mut self,
        auto_save_enabled: bool,
        has_project_path: bool,
        now: Instant,
    ) {
        self.revision = self.revision.wrapping_add(1);
        if auto_save_enabled && has_project_path {
            self.auto_save_deadline = Some(now + AUTO_SAVE_DEBOUNCE);
        }
    }

    pub(super) fn set_auto_save_enabled(&mut self, enabled: bool, eligible: bool, now: Instant) {
        self.auto_save_deadline = (enabled && eligible).then_some(now + AUTO_SAVE_DEBOUNCE);
    }

    pub(super) fn auto_save_due(&self, now: Instant) -> bool {
        self.in_flight.is_none() && self.auto_save_deadline.is_some_and(|due| now >= due)
    }

    pub(super) fn cancel_pending_auto_save(&mut self) {
        self.auto_save_deadline = None;
    }

    pub(super) fn begin_save(&mut self, automatic: bool) -> Option<ProjectSaveToken> {
        if self.in_flight.is_some() {
            if !automatic {
                self.manual_save_queued = true;
            }
            return None;
        }
        let token = ProjectSaveToken {
            document_id: self.document_id,
            revision: self.revision,
            automatic,
        };
        self.in_flight = Some(token);
        self.auto_save_deadline = None;
        Some(token)
    }

    /// Returns whether the completion belongs to the currently tracked save.
    pub(super) fn finish_save(&mut self, token: ProjectSaveToken) -> bool {
        if self.in_flight != Some(token) {
            return false;
        }
        self.in_flight = None;
        true
    }

    pub(super) fn completion_is_current(&self, token: ProjectSaveToken) -> bool {
        token.document_id == self.document_id && token.revision == self.revision
    }

    pub(super) fn take_manual_save_queued(&mut self) -> bool {
        std::mem::take(&mut self.manual_save_queued)
    }

    pub(super) fn is_saving(&self) -> bool {
        self.in_flight.is_some()
    }

    pub(super) fn reset_document(&mut self) {
        self.document_id = self.document_id.wrapping_add(1);
        self.revision = 0;
        self.auto_save_deadline = None;
        self.in_flight = None;
        self.manual_save_queued = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autosave_is_debounced_and_only_scheduled_for_named_projects() {
        let started = Instant::now();
        let mut runtime = SaveRuntime::default();

        runtime.project_changed(true, false, started);
        assert!(!runtime.auto_save_due(started + AUTO_SAVE_DEBOUNCE));

        runtime.project_changed(true, true, started);
        assert!(!runtime.auto_save_due(started + AUTO_SAVE_DEBOUNCE / 2));
        runtime.project_changed(true, true, started + AUTO_SAVE_DEBOUNCE / 2);
        assert!(!runtime.auto_save_due(started + AUTO_SAVE_DEBOUNCE));
        assert!(runtime.auto_save_due(started + AUTO_SAVE_DEBOUNCE * 2));
    }

    #[test]
    fn a_save_completion_cannot_clean_a_newer_revision() {
        let started = Instant::now();
        let mut runtime = SaveRuntime::default();
        runtime.project_changed(true, true, started);
        let token = runtime.begin_save(true).unwrap();
        runtime.project_changed(true, true, started + Duration::from_millis(10));

        assert!(runtime.finish_save(token));
        assert!(!runtime.completion_is_current(token));
        assert!(runtime.auto_save_due(started + AUTO_SAVE_DEBOUNCE * 2));
    }

    #[test]
    fn manual_save_waits_behind_an_in_flight_autosave() {
        let mut runtime = SaveRuntime::default();
        let automatic = runtime.begin_save(true).unwrap();

        assert!(runtime.begin_save(false).is_none());
        assert!(runtime.finish_save(automatic));
        assert!(runtime.take_manual_save_queued());
        assert!(!runtime.take_manual_save_queued());
    }

    #[test]
    fn first_save_makes_newer_untitled_edits_eligible_for_autosave() {
        let started = Instant::now();
        let mut runtime = SaveRuntime::default();
        runtime.project_changed(true, false, started);
        let first_save = runtime.begin_save(false).unwrap();
        runtime.project_changed(true, false, started + Duration::from_millis(10));
        assert!(runtime.finish_save(first_save));
        assert!(!runtime.completion_is_current(first_save));

        runtime.set_auto_save_enabled(true, true, started + Duration::from_millis(20));
        assert!(runtime.auto_save_due(started + AUTO_SAVE_DEBOUNCE * 2));
    }

    #[test]
    fn old_document_completions_are_ignored_after_reset() {
        let mut runtime = SaveRuntime::default();
        let token = runtime.begin_save(false).unwrap();
        runtime.reset_document();

        assert!(!runtime.finish_save(token));
        assert!(!runtime.completion_is_current(token));
    }
}
