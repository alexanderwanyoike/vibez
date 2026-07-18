//! Perform workspace interaction state.
//!
//! Computer keyboards and later hardware adapters converge on [`PadGesture`]
//! before the active Perform Mode assigns musical meaning.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use vibez_core::id::SectionId;
use vibez_project::SectionLaunchQuantization;

use super::EngineHandle;

mod sections;
pub use sections::{Section, SectionStore};

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

    const fn index(self) -> usize {
        self.row as usize * 4 + self.column as usize
    }

    pub const fn slot_in_bank(self, bank: u8) -> u16 {
        bank as u16 * 16 + self.index() as u16
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

/// A portable physical computer-key position supported by Perform rebinding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComputerKey {
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
}

impl ComputerKey {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }
}

const DEFAULT_COMPUTER_KEYS: [ComputerKey; 16] = [
    ComputerKey::Digit1,
    ComputerKey::Digit2,
    ComputerKey::Digit3,
    ComputerKey::Digit4,
    ComputerKey::Q,
    ComputerKey::W,
    ComputerKey::E,
    ComputerKey::R,
    ComputerKey::A,
    ComputerKey::S,
    ComputerKey::D,
    ComputerKey::F,
    ComputerKey::Z,
    ComputerKey::X,
    ComputerKey::C,
    ComputerKey::V,
];

/// Global producer preference assigning physical computer keys to Pad Positions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformInputMapping {
    #[serde(default = "default_computer_keys")]
    computer_keys: [ComputerKey; 16],
}

const fn default_computer_keys() -> [ComputerKey; 16] {
    DEFAULT_COMPUTER_KEYS
}

impl Default for PerformInputMapping {
    fn default() -> Self {
        Self {
            computer_keys: DEFAULT_COMPUTER_KEYS,
        }
    }
}

impl PerformInputMapping {
    pub fn key_for(&self, position: PadPosition) -> ComputerKey {
        self.computer_keys[position.index()]
    }

    pub fn position_for(&self, key: ComputerKey) -> Option<PadPosition> {
        self.computer_keys
            .iter()
            .position(|candidate| *candidate == key)
            .map(|index| PadPosition::ALL[index])
    }

    pub fn rebind(&mut self, position: PadPosition, key: ComputerKey) {
        let target = position.index();
        if let Some(existing) = self
            .computer_keys
            .iter()
            .position(|candidate| *candidate == key)
        {
            self.computer_keys.swap(target, existing);
        } else {
            self.computer_keys[target] = key;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadGestureKind {
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadGestureSource {
    ComputerKeyboard { key: ComputerKey },
}

/// Controller-independent input delivered synchronously from an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadGesture {
    pub position: PadPosition,
    pub kind: PadGestureKind,
    pub velocity: Option<u8>,
    pub source: PadGestureSource,
    pub occurred_at: Instant,
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

/// Perform state combines project-owned Sections with runtime interaction.
/// Input mapping, mode, banks, focus, and edit buffers remain global/runtime;
/// only the Section store enters project persistence and undo.
#[derive(Default)]
pub struct PerformState {
    pub mode: PerformMode,
    pub banks: PerformBanks,
    pub selected_pad: Option<PadPosition>,
    pub editor_focus: PerformEditorFocus,
    pub input_mapping: PerformInputMapping,
    pub key_rebind_target: Option<PadPosition>,
    pub sections: Arc<SectionStore>,
    pub selected_section: Option<SectionId>,
    pub editing_section_name: Option<SectionId>,
    pub section_name_edit: String,
    pub duplicate_source: Option<SectionId>,
    active_computer_keys: HashMap<String, (PadPosition, PadGestureSource)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PerformMsg {
    SelectMode(PerformMode),
    FocusEditor(PerformEditorFocus),
    BeginKeyRebind(PadPosition),
    CancelKeyRebind,
    SelectSection(SectionId),
    CreateSectionAt(u16),
    BeginDuplicateSection(SectionId),
    CancelDuplicateSection,
    DuplicateSectionTo(u16),
    DeleteSection(SectionId),
    StartEditingSectionName(SectionId),
    CancelSectionNameEdit,
    SectionNameInput(String),
    CommitSectionName(SectionId),
    SetSectionLengthBeats(SectionId, f64),
    SetSectionLaunchQuantization(SectionId, SectionLaunchQuantization),
    ToggleSectionLoop(SectionId),
    ComputerKeyPressed {
        key: ComputerKey,
        key_id: String,
        occurred_at: Instant,
    },
    ComputerKeyReleased {
        key_id: String,
        occurred_at: Instant,
    },
}

impl PerformMsg {
    pub const fn marks_dirty(&self) -> bool {
        matches!(
            self,
            Self::CreateSectionAt(_)
                | Self::DuplicateSectionTo(_)
                | Self::DeleteSection(_)
                | Self::CommitSectionName(_)
                | Self::SetSectionLengthBeats(..)
                | Self::SetSectionLaunchQuantization(..)
                | Self::ToggleSectionLoop(_)
        )
    }
}

/// Read-only facts supplied by the router.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerformCtx {
    pub workspace_visible: bool,
}

/// Cross-domain effects requested by Perform. Pad Gestures leave the adapter
/// through this boundary so later musical consumers do not observe UI state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PerformAction {
    pub keyboard_consumed: bool,
    pub persist_settings: bool,
    pub gesture: Option<PadGesture>,
}

impl PerformState {
    pub fn update(
        &mut self,
        msg: PerformMsg,
        _engine: &mut impl EngineHandle,
        ctx: PerformCtx,
    ) -> PerformAction {
        match msg {
            PerformMsg::SelectMode(mode) => {
                if ctx.workspace_visible {
                    self.mode = mode;
                }
            }
            PerformMsg::FocusEditor(focus) => {
                if ctx.workspace_visible {
                    self.editor_focus = focus;
                }
            }
            PerformMsg::BeginKeyRebind(position) => {
                self.key_rebind_target = Some(position);
            }
            PerformMsg::CancelKeyRebind => {
                self.key_rebind_target = None;
            }
            PerformMsg::SelectSection(id) => {
                if ctx.workspace_visible {
                    self.select_section(id);
                }
            }
            PerformMsg::CreateSectionAt(slot) => {
                if ctx.workspace_visible && self.sections.at_slot(slot).is_none() {
                    let section = Section::new(slot);
                    let id = section.id;
                    Arc::make_mut(&mut self.sections).insert(section);
                    self.duplicate_source = None;
                    self.select_section(id);
                }
            }
            PerformMsg::BeginDuplicateSection(id) => {
                if ctx.workspace_visible && self.sections.by_id(id).is_some() {
                    self.duplicate_source = Some(id);
                }
            }
            PerformMsg::CancelDuplicateSection => {
                self.duplicate_source = None;
            }
            PerformMsg::DuplicateSectionTo(slot) => {
                if ctx.workspace_visible && self.sections.at_slot(slot).is_none() {
                    let duplicate = self
                        .duplicate_source
                        .and_then(|id| self.sections.by_id(id))
                        .map(|source| source.duplicate_to(slot));
                    if let Some(section) = duplicate {
                        let id = section.id;
                        Arc::make_mut(&mut self.sections).insert(section);
                        self.duplicate_source = None;
                        self.select_section(id);
                    }
                }
            }
            PerformMsg::DeleteSection(id) => {
                if ctx.workspace_visible && Arc::make_mut(&mut self.sections).remove(id).is_some() {
                    if self.selected_section == Some(id) {
                        self.selected_section = None;
                        self.editing_section_name = None;
                        self.section_name_edit.clear();
                    }
                    if self.duplicate_source == Some(id) {
                        self.duplicate_source = None;
                    }
                }
            }
            PerformMsg::StartEditingSectionName(id) => {
                if ctx.workspace_visible && self.sections.by_id(id).is_some() {
                    self.select_section(id);
                    self.editing_section_name = Some(id);
                }
            }
            PerformMsg::CancelSectionNameEdit => {
                self.editing_section_name = None;
                self.section_name_edit = self
                    .selected_section
                    .and_then(|id| self.sections.by_id(id))
                    .map(|section| section.name.clone())
                    .unwrap_or_default();
            }
            PerformMsg::SectionNameInput(name) => {
                self.section_name_edit = name;
            }
            PerformMsg::CommitSectionName(id) => {
                let name = self.section_name_edit.trim().to_string();
                if ctx.workspace_visible && !name.is_empty() {
                    if let Some(section) = Arc::make_mut(&mut self.sections).by_id_mut(id) {
                        section.name = name;
                        self.section_name_edit = section.name.clone();
                        self.editing_section_name = None;
                    }
                }
            }
            PerformMsg::SetSectionLengthBeats(id, beats) => {
                if ctx.workspace_visible {
                    if let Some(section) = Arc::make_mut(&mut self.sections).by_id_mut(id) {
                        section.length_beats = beats.clamp(
                            sections::MIN_SECTION_LENGTH_BEATS,
                            sections::MAX_SECTION_LENGTH_BEATS,
                        );
                    }
                }
            }
            PerformMsg::SetSectionLaunchQuantization(id, quantization) => {
                if ctx.workspace_visible {
                    if let Some(section) = Arc::make_mut(&mut self.sections).by_id_mut(id) {
                        section.launch_quantization = quantization;
                    }
                }
            }
            PerformMsg::ToggleSectionLoop(id) => {
                if ctx.workspace_visible {
                    if let Some(section) = Arc::make_mut(&mut self.sections).by_id_mut(id) {
                        section.looping = !section.looping;
                    }
                }
            }
            PerformMsg::ComputerKeyPressed {
                key,
                key_id,
                occurred_at,
            } => {
                if let Some(position) = self.key_rebind_target.take() {
                    self.input_mapping.rebind(position, key);
                    return PerformAction {
                        keyboard_consumed: true,
                        persist_settings: true,
                        gesture: None,
                    };
                }
                if !ctx.workspace_visible {
                    return PerformAction::default();
                }
                if self.active_computer_keys.contains_key(&key_id) {
                    return PerformAction {
                        keyboard_consumed: true,
                        ..PerformAction::default()
                    };
                }
                let Some(position) = self.input_mapping.position_for(key) else {
                    return PerformAction::default();
                };
                let source = PadGestureSource::ComputerKeyboard { key };
                self.active_computer_keys.insert(key_id, (position, source));
                return PerformAction {
                    keyboard_consumed: true,
                    persist_settings: false,
                    gesture: Some(PadGesture {
                        position,
                        kind: PadGestureKind::Press,
                        velocity: None,
                        source,
                        occurred_at,
                    }),
                };
            }
            PerformMsg::ComputerKeyReleased {
                key_id,
                occurred_at,
            } => {
                let Some((position, source)) = self.active_computer_keys.remove(&key_id) else {
                    return PerformAction::default();
                };
                return PerformAction {
                    keyboard_consumed: true,
                    persist_settings: false,
                    gesture: Some(PadGesture {
                        position,
                        kind: PadGestureKind::Release,
                        velocity: None,
                        source,
                        occurred_at,
                    }),
                };
            }
        }
        PerformAction::default()
    }

    pub fn is_pad_pressed(&self, position: PadPosition) -> bool {
        self.active_computer_keys
            .values()
            .any(|(pressed, _)| *pressed == position)
    }

    fn select_section(&mut self, id: SectionId) {
        if let Some(section) = self.sections.by_id(id) {
            self.selected_section = Some(id);
            self.editing_section_name = None;
            self.section_name_edit = section.name.clone();
            self.editor_focus = PerformEditorFocus::SectionConstruction;
        }
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
        assert_eq!(action, PerformAction::default());
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

    #[test]
    fn default_mapping_uses_the_settled_physical_layout() {
        let mapping = PerformInputMapping::default();
        assert_eq!(
            PadPosition::ALL.map(|position| mapping.key_for(position).label()),
            ["1", "2", "3", "4", "Q", "W", "E", "R", "A", "S", "D", "F", "Z", "X", "C", "V"]
        );
    }

    #[test]
    fn one_hold_produces_exactly_one_press_and_release() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();
        let pressed_at = Instant::now();
        let released_at = pressed_at + std::time::Duration::from_millis(23);
        let ctx = PerformCtx {
            workspace_visible: true,
        };

        let press = state.update(
            PerformMsg::ComputerKeyPressed {
                key: ComputerKey::Q,
                key_id: "q".into(),
                occurred_at: pressed_at,
            },
            &mut engine,
            ctx,
        );
        let repeat = state.update(
            PerformMsg::ComputerKeyPressed {
                key: ComputerKey::Q,
                key_id: "q".into(),
                occurred_at: pressed_at,
            },
            &mut engine,
            ctx,
        );
        let release = state.update(
            PerformMsg::ComputerKeyReleased {
                key_id: "q".into(),
                occurred_at: released_at,
            },
            &mut engine,
            ctx,
        );
        let extra_release = state.update(
            PerformMsg::ComputerKeyReleased {
                key_id: "q".into(),
                occurred_at: released_at,
            },
            &mut engine,
            ctx,
        );

        let position = PadPosition { row: 1, column: 0 };
        assert_eq!(
            press.gesture,
            Some(PadGesture {
                position,
                kind: PadGestureKind::Press,
                velocity: None,
                source: PadGestureSource::ComputerKeyboard {
                    key: ComputerKey::Q
                },
                occurred_at: pressed_at,
            })
        );
        assert!(repeat.keyboard_consumed);
        assert!(repeat.gesture.is_none());
        assert_eq!(release.gesture.unwrap().kind, PadGestureKind::Release);
        assert_eq!(release.gesture.unwrap().occurred_at, released_at);
        assert!(extra_release.gesture.is_none());
        assert!(!state.is_pad_pressed(position));
        assert!(engine.0.is_empty());
    }

    #[test]
    fn mapping_changes_do_not_change_gesture_structure_between_modes() {
        let at = Instant::now();
        let mut engine = RecordingEngine::default();
        let mut sections = PerformState::default();
        let mut instrument = PerformState {
            mode: PerformMode::Instrument,
            ..PerformState::default()
        };
        sections
            .input_mapping
            .rebind(PadPosition::ALL[0], ComputerKey::Y);
        instrument.input_mapping = sections.input_mapping.clone();

        let mut press = |state: &mut PerformState, key_id: &str| {
            state
                .update(
                    PerformMsg::ComputerKeyPressed {
                        key: ComputerKey::Y,
                        key_id: key_id.into(),
                        occurred_at: at,
                    },
                    &mut engine,
                    PerformCtx {
                        workspace_visible: true,
                    },
                )
                .gesture
                .unwrap()
        };

        assert_eq!(
            press(&mut sections, "sections"),
            press(&mut instrument, "instrument")
        );
    }

    #[test]
    fn release_keeps_the_original_pair_when_mapping_changes_mid_hold() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();
        let ctx = PerformCtx {
            workspace_visible: true,
        };
        let at = Instant::now();
        let press = state.update(
            PerformMsg::ComputerKeyPressed {
                key: ComputerKey::Q,
                key_id: "q".into(),
                occurred_at: at,
            },
            &mut engine,
            ctx,
        );
        state
            .input_mapping
            .rebind(PadPosition::ALL[0], ComputerKey::Q);
        let release = state.update(
            PerformMsg::ComputerKeyReleased {
                key_id: "q".into(),
                occurred_at: at,
            },
            &mut engine,
            ctx,
        );

        assert_eq!(press.gesture.unwrap().position, PadPosition::ALL[4]);
        assert_eq!(release.gesture.unwrap().position, PadPosition::ALL[4]);
    }

    #[test]
    fn rebinding_swaps_an_existing_key_and_requests_global_persistence() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();
        state.update(
            PerformMsg::BeginKeyRebind(PadPosition::ALL[0]),
            &mut engine,
            PerformCtx::default(),
        );
        let action = state.update(
            PerformMsg::ComputerKeyPressed {
                key: ComputerKey::Q,
                key_id: "q".into(),
                occurred_at: Instant::now(),
            },
            &mut engine,
            PerformCtx::default(),
        );

        assert_eq!(
            state.input_mapping.key_for(PadPosition::ALL[0]),
            ComputerKey::Q
        );
        assert_eq!(
            state.input_mapping.key_for(PadPosition::ALL[4]),
            ComputerKey::Digit1
        );
        assert!(action.keyboard_consumed);
        assert!(action.persist_settings);
        assert!(action.gesture.is_none());
    }

    #[test]
    fn section_crud_and_properties_update_the_ordered_store() {
        let mut state = PerformState::default();
        let mut engine = RecordingEngine::default();
        let ctx = PerformCtx {
            workspace_visible: true,
        };

        state.update(PerformMsg::CreateSectionAt(5), &mut engine, ctx);
        let source_id = state.selected_section.expect("new section selected");
        state.update(
            PerformMsg::StartEditingSectionName(source_id),
            &mut engine,
            ctx,
        );
        assert_eq!(state.editing_section_name, Some(source_id));
        state.update(
            PerformMsg::SectionNameInput("Breakdown".into()),
            &mut engine,
            ctx,
        );
        state.update(PerformMsg::CommitSectionName(source_id), &mut engine, ctx);
        assert_eq!(state.editing_section_name, None);
        state.update(
            PerformMsg::SetSectionLengthBeats(source_id, 32.0),
            &mut engine,
            ctx,
        );
        state.update(
            PerformMsg::SetSectionLaunchQuantization(
                source_id,
                SectionLaunchQuantization::EndOfSection,
            ),
            &mut engine,
            ctx,
        );
        state.update(PerformMsg::ToggleSectionLoop(source_id), &mut engine, ctx);
        state.update(
            PerformMsg::BeginDuplicateSection(source_id),
            &mut engine,
            ctx,
        );
        state.update(PerformMsg::DuplicateSectionTo(2), &mut engine, ctx);

        let duplicate_id = state.selected_section.expect("duplicate selected");
        let duplicate = state.sections.by_id(duplicate_id).unwrap();
        assert_eq!(duplicate.slot, 2);
        assert_eq!(duplicate.name, "Breakdown Copy");
        assert_eq!(duplicate.length_beats, 32.0);
        assert_eq!(
            duplicate.launch_quantization,
            SectionLaunchQuantization::EndOfSection
        );
        assert!(!duplicate.looping);
        assert_eq!(state.sections.sections[1].slot, 5);

        state.update(PerformMsg::DeleteSection(duplicate_id), &mut engine, ctx);
        assert!(state.sections.by_id(duplicate_id).is_none());
        assert_eq!(state.selected_section, None);
        assert!(engine.0.is_empty());
    }

    #[test]
    fn only_section_document_edits_are_dirty() {
        let id = SectionId::new();
        assert!(PerformMsg::CreateSectionAt(0).marks_dirty());
        assert!(PerformMsg::DuplicateSectionTo(1).marks_dirty());
        assert!(PerformMsg::DeleteSection(id).marks_dirty());
        assert!(PerformMsg::CommitSectionName(id).marks_dirty());
        assert!(PerformMsg::SetSectionLengthBeats(id, 8.0).marks_dirty());
        assert!(
            PerformMsg::SetSectionLaunchQuantization(id, SectionLaunchQuantization::OneBeat)
                .marks_dirty()
        );
        assert!(PerformMsg::ToggleSectionLoop(id).marks_dirty());
        assert!(!PerformMsg::SelectSection(id).marks_dirty());
        assert!(!PerformMsg::SectionNameInput("Draft".into()).marks_dirty());
        assert!(!PerformMsg::StartEditingSectionName(id).marks_dirty());
        assert!(!PerformMsg::CancelSectionNameEdit.marks_dirty());
        assert!(!PerformMsg::BeginDuplicateSection(id).marks_dirty());
    }
}
