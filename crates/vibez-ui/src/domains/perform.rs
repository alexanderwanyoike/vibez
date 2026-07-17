//! Perform workspace interaction state.
//!
//! Card 02 intentionally owns UI state only. Playback truth and Pad Gestures
//! arrive in later slices; changing Perform Mode must therefore be silent at
//! the engine boundary.

use super::EngineHandle;

/// The three Perform Modes exposed in V1. Macros stays absent until its
/// behavior and Capture semantics are defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PerformMode {
    #[default]
    Sections,
    TrackMutes,
    Instrument,
}

impl PerformMode {
    pub const ALL: [Self; 3] = [Self::Sections, Self::TrackMutes, Self::Instrument];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sections => "Sections",
            Self::TrackMutes => "Track Mutes",
            Self::Instrument => "Instrument",
        }
    }

    pub const fn shortcut(self) -> &'static str {
        match self {
            Self::Sections => "F1",
            Self::TrackMutes => "F2",
            Self::Instrument => "F3",
        }
    }
}

/// A stable physical location on the 4x4 Pad Surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PadPosition {
    pub row: u8,
    pub column: u8,
}

impl PadPosition {
    pub const ALL: [Self; 16] = [
        Self::new(0, 0),
        Self::new(0, 1),
        Self::new(0, 2),
        Self::new(0, 3),
        Self::new(1, 0),
        Self::new(1, 1),
        Self::new(1, 2),
        Self::new(1, 3),
        Self::new(2, 0),
        Self::new(2, 1),
        Self::new(2, 2),
        Self::new(2, 3),
        Self::new(3, 0),
        Self::new(3, 1),
        Self::new(3, 2),
        Self::new(3, 3),
    ];

    const fn new(row: u8, column: u8) -> Self {
        Self { row, column }
    }

    /// One-based mode order for this stable position. Sections and Track
    /// Mutes begin at the top-left; Instrument begins at the bottom-left.
    pub const fn ordinal(self, mode: PerformMode) -> u8 {
        match mode {
            PerformMode::Sections | PerformMode::TrackMutes => self.row * 4 + self.column + 1,
            PerformMode::Instrument => (3 - self.row) * 4 + self.column + 1,
        }
    }
}

/// Which part of Perform owns keyboard/editor focus. This remains runtime UI
/// state and is deliberately not persisted in the project document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PerformEditorFocus {
    #[default]
    PadSurface,
    SectionConstruction,
}

/// Per-mode bank cursors are UI interaction state. Navigation is added by a
/// later card; Card 02 establishes the ownership boundary only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PerformBanks {
    pub sections: u8,
    pub track_mutes: u8,
    pub instrument: u8,
}

impl PerformBanks {
    pub const fn for_mode(self, mode: PerformMode) -> u8 {
        match mode {
            PerformMode::Sections => self.sections,
            PerformMode::TrackMutes => self.track_mutes,
            PerformMode::Instrument => self.instrument,
        }
    }
}

/// Perform's runtime interaction slice. It is not part of project persistence
/// or undo because changing the workspace presentation does not edit music.
#[derive(Debug, Default)]
pub struct PerformState {
    pub mode: PerformMode,
    pub banks: PerformBanks,
    pub selected_pad: Option<PadPosition>,
    pub editor_focus: PerformEditorFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformMsg {
    SelectMode(PerformMode),
    FocusEditor(PerformEditorFocus),
}

impl PerformMsg {
    /// Perform shell interaction is UI state, never a project edit.
    pub const fn marks_dirty(&self) -> bool {
        false
    }
}

/// Read-only facts supplied by the router.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerformCtx {
    pub workspace_visible: bool,
}

/// Cross-domain effects requested by Perform. The shell currently needs none,
/// but the explicit action boundary keeps later slices out of the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PerformAction {
    #[default]
    None,
}

impl PerformState {
    pub fn update(
        &mut self,
        msg: PerformMsg,
        _engine: &mut impl EngineHandle,
        ctx: PerformCtx,
    ) -> PerformAction {
        if !ctx.workspace_visible {
            return PerformAction::None;
        }

        match msg {
            PerformMsg::SelectMode(mode) => {
                self.mode = mode;
            }
            PerformMsg::FocusEditor(focus) => {
                self.editor_focus = focus;
            }
        }
        PerformAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;

    #[test]
    fn exposes_exactly_the_three_settled_v1_modes() {
        assert_eq!(
            PerformMode::ALL.map(PerformMode::label),
            ["Sections", "Track Mutes", "Instrument"]
        );
        assert_eq!(
            PerformMode::ALL.map(PerformMode::shortcut),
            ["F1", "F2", "F3"]
        );
    }

    #[test]
    fn visible_mode_switches_are_ui_only() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();

        let action = state.update(
            PerformMsg::SelectMode(PerformMode::Instrument),
            &mut engine,
            PerformCtx {
                workspace_visible: true,
            },
        );

        assert_eq!(state.mode, PerformMode::Instrument);
        assert_eq!(action, PerformAction::None);
        assert!(engine.0.is_empty());
        assert!(!PerformMsg::SelectMode(PerformMode::Sections).marks_dirty());
    }

    #[test]
    fn shortcuts_do_not_change_hidden_perform_state() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();

        state.update(
            PerformMsg::SelectMode(PerformMode::TrackMutes),
            &mut engine,
            PerformCtx {
                workspace_visible: false,
            },
        );

        assert_eq!(state.mode, PerformMode::Sections);
        assert!(engine.0.is_empty());
    }

    #[test]
    fn pad_positions_are_stable_with_mode_specific_order_origins() {
        let top_left = PadPosition::ALL[0];
        let bottom_left = PadPosition::ALL[12];

        assert_eq!(top_left.ordinal(PerformMode::Sections), 1);
        assert_eq!(bottom_left.ordinal(PerformMode::Sections), 13);
        assert_eq!(top_left.ordinal(PerformMode::Instrument), 13);
        assert_eq!(bottom_left.ordinal(PerformMode::Instrument), 1);

        let mut instrument_ordinals =
            PadPosition::ALL.map(|position| position.ordinal(PerformMode::Instrument));
        instrument_ordinals.sort_unstable();
        assert_eq!(
            instrument_ordinals,
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn bank_selection_and_focus_default_to_ui_owned_shell_state() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();
        assert_eq!(state.banks, PerformBanks::default());
        assert_eq!(state.selected_pad, None);
        assert_eq!(state.editor_focus, PerformEditorFocus::PadSurface);

        state.update(
            PerformMsg::FocusEditor(PerformEditorFocus::SectionConstruction),
            &mut engine,
            PerformCtx {
                workspace_visible: true,
            },
        );
        assert_eq!(state.editor_focus, PerformEditorFocus::SectionConstruction);
        assert!(engine.0.is_empty());
    }
}
