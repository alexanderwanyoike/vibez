//! Own presentation health independently of retained terminal notification lifetimes.

use super::terminal_notifications::TerminalNotifications;

#[derive(Default, PartialEq, Eq)]
enum PresentationHealth {
    #[default]
    Normal,
    Faulted,
}

#[derive(Default)]
pub(super) struct PresentationState {
    health: PresentationHealth,
    pub(super) terminals: TerminalNotifications,
}

impl PresentationState {
    pub(super) fn is_faulted(&self) -> bool {
        self.health == PresentationHealth::Faulted
    }

    pub(super) fn enter_fault(&mut self) -> bool {
        if self.is_faulted() {
            return false;
        }
        self.health = PresentationHealth::Faulted;
        true
    }

    pub(super) fn resume(&mut self) {
        // A resumed transport can coexist with terminal owners still blocked
        // by the UI ring. Clearing health must not replace their aggregate.
        self.health = PresentationHealth::Normal;
    }
}
