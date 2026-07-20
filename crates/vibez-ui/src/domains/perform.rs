//! Perform workspace interaction state.
//!
//! Computer keyboards and later hardware adapters converge on [`PadGesture`]
//! before the active Perform Mode assigns musical meaning.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use vibez_core::id::{SectionId, TrackId};
use vibez_project::SectionLaunchQuantization;

use super::EngineHandle;
use crate::state::ProjectTrack;

mod instrument;
mod sections;
pub use instrument::SixteenLevelsParameter;
use instrument::{ActiveInstrumentNote, InstrumentPerformanceState};
pub use sections::{Section, SectionStore, SectionTimelineEditor, TimelineContentLocation};

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

/// Per-mode bank cursors are UI interaction state. Instrument target banks are
/// paged only while its target overlay is visible; normal Instrument bracket
/// navigation changes the separate octave range instead.
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
/// Input mapping, mode, banks, live Instrument controls, focus, and edit buffers
/// remain global/runtime; only the Section store enters project persistence and
/// undo.
#[derive(Default)]
pub struct PerformState {
    pub mode: PerformMode,
    pub banks: PerformBanks,
    pub selected_pad: Option<PadPosition>,
    pub editor_focus: PerformEditorFocus,
    pub input_mapping: PerformInputMapping,
    pub key_rebind_target: Option<PadPosition>,
    pub instrument_target_overlay: bool,
    instrument: InstrumentPerformanceState,
    pub sections: Arc<SectionStore>,
    pub selected_section: Option<SectionId>,
    pub section_editor: SectionTimelineEditor,
    pub section_timeline_expanded: bool,
    pub editing_section_name: Option<SectionId>,
    pub section_name_edit: String,
    pub duplicate_source: Option<SectionId>,
    /// Engine-owned playback truth mirrored from Section events.
    pub playing_section: Option<SectionId>,
    pub queued_section: Option<SectionId>,
    pub pending_section_boundary_samples: Option<u64>,
    pub section_playhead_samples: u64,
    active_computer_keys:
        HashMap<String, (PadPosition, PadGestureSource, Option<ActiveInstrumentNote>)>,
    track_mute_slots: Vec<Option<TrackId>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PerformMsg {
    SelectMode(PerformMode),
    FocusEditor(PerformEditorFocus),
    BeginKeyRebind(PadPosition),
    CancelKeyRebind,
    SelectSection(SectionId),
    LaunchSection(SectionId),
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
    ToggleSectionTimelineExpanded,
    RemoveTrackContent {
        section_id: SectionId,
        track_id: TrackId,
    },
    PreviousBank,
    NextBank,
    ToggleTrackMuteFromPad(PadPosition),
    SetInstrumentTargetOverlay(bool),
    SelectInstrumentTarget(TrackId),
    SetFixedComputerVelocity(u8),
    ToggleFullLevel,
    ToggleSixteenLevels,
    SelectSixteenLevelsParameter(SixteenLevelsParameter),
    SetSixteenLevelsMinimum(i16),
    SetSixteenLevelsMaximum(i16),
    ChooseSixteenLevelsSource,
    ComputerKeyPressed {
        key: ComputerKey,
        key_id: String,
        occurred_at: Instant,
    },
    ComputerKeyReleased {
        key_id: String,
        occurred_at: Instant,
    },
    WindowUnfocused,
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
                | Self::RemoveTrackContent { .. }
        )
    }
}

/// Read-only facts supplied by the router.
#[derive(Debug, Clone, Copy, Default)]
pub struct PerformCtx<'a> {
    pub workspace_visible: bool,
    pub project_tracks: &'a [ProjectTrack],
    pub selected_project_track: Option<TrackId>,
}

/// A semantic mute request resolved by Perform against a stable pad slot.
/// The router applies it to the single shared Project Track state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackMuteRequest {
    pub track_id: TrackId,
    pub muted: bool,
}

/// Cross-domain effects requested by Perform. Pad Gestures leave the adapter
/// through this boundary so later musical consumers do not observe UI state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PerformAction {
    pub keyboard_consumed: bool,
    pub persist_settings: bool,
    pub gesture: Option<PadGesture>,
    pub track_mute_request: Option<TrackMuteRequest>,
    pub select_project_track: Option<TrackId>,
    pub section_launch: Option<SectionId>,
    pub section_content_changed: Option<SectionId>,
}

impl PerformState {
    fn sync_track_mute_slots(&mut self, tracks: &[ProjectTrack]) {
        for slot in &mut self.track_mute_slots {
            if slot.is_some_and(|track_id| !tracks.iter().any(|track| track.id == track_id)) {
                *slot = None;
            }
        }

        for track in tracks {
            if self
                .track_mute_slots
                .iter()
                .flatten()
                .any(|track_id| *track_id == track.id)
            {
                continue;
            }
            if let Some(vacant) = self.track_mute_slots.iter_mut().find(|slot| slot.is_none()) {
                *vacant = Some(track.id);
            } else {
                self.track_mute_slots.push(Some(track.id));
            }
        }

        while self.track_mute_slots.last().is_some_and(Option::is_none) {
            self.track_mute_slots.pop();
        }

        let last_bank = self.track_mute_slots.len().saturating_sub(1) / 16;
        self.banks.track_mutes = self.banks.track_mutes.min(last_bank as u8);
        let playable_targets = tracks
            .iter()
            .filter(|track| track.is_playable_midi_target())
            .count();
        let last_instrument_bank = playable_targets.saturating_sub(1) / 16;
        self.banks.instrument = self.banks.instrument.min(last_instrument_bank as u8);
    }

    fn track_mute_request(
        &self,
        position: PadPosition,
        tracks: &[ProjectTrack],
    ) -> Option<TrackMuteRequest> {
        let slot = usize::from(self.banks.track_mutes) * 16 + position.index();
        let track_id = self.track_mute_slots.get(slot).copied().flatten()?;
        let track = tracks.iter().find(|track| track.id == track_id)?;
        Some(TrackMuteRequest {
            track_id,
            muted: !track.mute,
        })
    }

    pub fn track_for_mute_pad<'a>(
        &self,
        position: PadPosition,
        tracks: &'a [ProjectTrack],
    ) -> Option<&'a ProjectTrack> {
        let slot = usize::from(self.banks.track_mutes) * 16 + position.index();
        let track_id = self.track_mute_slots.get(slot).copied().flatten()?;
        tracks.iter().find(|track| track.id == track_id)
    }

    pub fn track_for_instrument_target_pad<'a>(
        &self,
        position: PadPosition,
        tracks: &'a [ProjectTrack],
    ) -> Option<&'a ProjectTrack> {
        let slot = usize::from(self.banks.instrument) * 16
            + usize::from(position.ordinal(PerformMode::Instrument) - 1);
        tracks
            .iter()
            .filter(|track| track.is_playable_midi_target())
            .nth(slot)
    }

    pub fn update(
        &mut self,
        msg: PerformMsg,
        engine: &mut impl EngineHandle,
        ctx: PerformCtx<'_>,
    ) -> PerformAction {
        self.sync_track_mute_slots(ctx.project_tracks);
        self.sync_instrument_target_from_selection(ctx.selected_project_track, ctx.project_tracks);
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
                    self.select_section(id, ctx.selected_project_track);
                }
            }
            PerformMsg::LaunchSection(id) => {
                if ctx.workspace_visible
                    && self.mode == PerformMode::Sections
                    && self.sections.by_id(id).is_some()
                {
                    self.select_section(id, ctx.selected_project_track);
                    return PerformAction {
                        section_launch: Some(id),
                        ..PerformAction::default()
                    };
                }
            }
            PerformMsg::CreateSectionAt(slot) => {
                if ctx.workspace_visible && self.sections.at_slot(slot).is_none() {
                    let section = Section::new(slot);
                    let id = section.id;
                    Arc::make_mut(&mut self.sections).insert(section);
                    self.duplicate_source = None;
                    self.select_section(id, ctx.selected_project_track);
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
                        self.select_section(id, ctx.selected_project_track);
                    }
                }
            }
            PerformMsg::DeleteSection(id) => {
                if ctx.workspace_visible && Arc::make_mut(&mut self.sections).remove(id).is_some() {
                    if self.selected_section == Some(id) {
                        self.selected_section = None;
                        self.section_editor.clear();
                        self.editing_section_name = None;
                        self.section_name_edit.clear();
                    }
                    if self.duplicate_source == Some(id) {
                        self.duplicate_source = None;
                    }
                    let last_bank = self
                        .sections
                        .sections
                        .iter()
                        .map(|section| section.slot / 16)
                        .max()
                        .unwrap_or(0);
                    self.banks.sections = self.banks.sections.min(last_bank as u8);
                }
            }
            PerformMsg::StartEditingSectionName(id) => {
                if ctx.workspace_visible && self.sections.by_id(id).is_some() {
                    self.select_section(id, ctx.selected_project_track);
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
                        return PerformAction {
                            section_content_changed: Some(id),
                            ..PerformAction::default()
                        };
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
                        return PerformAction {
                            section_content_changed: Some(id),
                            ..PerformAction::default()
                        };
                    }
                }
            }
            PerformMsg::ToggleSectionTimelineExpanded => {
                if ctx.workspace_visible && self.selected_section.is_some() {
                    self.section_timeline_expanded = !self.section_timeline_expanded;
                }
            }
            PerformMsg::RemoveTrackContent {
                section_id,
                track_id,
            } => {
                if ctx.workspace_visible && self.selected_section == Some(section_id) {
                    let editor = self.section_editor.editor_mut();
                    Arc::make_mut(&mut editor.timeline).remove(track_id);
                    editor.selected_clips.retain(|selection| match selection {
                        crate::state::ArrangementSelection::AudioClip { track_id: id, .. }
                        | crate::state::ArrangementSelection::NoteClip { track_id: id, .. } => {
                            *id != track_id
                        }
                    });
                    if editor
                        .selected_note_clip
                        .is_some_and(|(id, _)| id == track_id)
                    {
                        editor.selected_note_clip = None;
                    }
                    self.commit_selected_section_timeline();
                    return PerformAction {
                        section_content_changed: Some(section_id),
                        ..PerformAction::default()
                    };
                }
            }
            PerformMsg::PreviousBank => {
                if ctx.workspace_visible {
                    match self.mode {
                        PerformMode::Sections => {
                            self.banks.sections = self.banks.sections.saturating_sub(1)
                        }
                        PerformMode::TrackMutes => {
                            self.banks.track_mutes = self.banks.track_mutes.saturating_sub(1)
                        }
                        PerformMode::Instrument => {
                            if self.instrument_target_overlay {
                                self.banks.instrument = self.banks.instrument.saturating_sub(1);
                            } else {
                                self.shift_instrument_octave(-1);
                            }
                        }
                    }
                }
            }
            PerformMsg::NextBank => {
                if ctx.workspace_visible {
                    match self.mode {
                        PerformMode::Sections => {
                            let last_reachable_bank = self
                                .sections
                                .sections
                                .iter()
                                .map(|section| section.slot / 16)
                                .max()
                                .map(|bank| u8::try_from(bank.saturating_add(1)).unwrap_or(u8::MAX))
                                .unwrap_or(0);
                            self.banks.sections = self
                                .banks
                                .sections
                                .saturating_add(1)
                                .min(last_reachable_bank);
                        }
                        PerformMode::TrackMutes => {
                            let last_bank = self.track_mute_slots.len().saturating_sub(1) / 16;
                            self.banks.track_mutes = self
                                .banks
                                .track_mutes
                                .saturating_add(1)
                                .min(last_bank as u8);
                        }
                        PerformMode::Instrument => {
                            if self.instrument_target_overlay {
                                let playable = ctx
                                    .project_tracks
                                    .iter()
                                    .filter(|track| track.is_playable_midi_target())
                                    .count();
                                let last_bank = playable.saturating_sub(1) / 16;
                                self.banks.instrument =
                                    self.banks.instrument.saturating_add(1).min(last_bank as u8);
                            } else {
                                self.shift_instrument_octave(1);
                            }
                        }
                    }
                }
            }
            PerformMsg::ToggleTrackMuteFromPad(position) => {
                if ctx.workspace_visible && self.mode == PerformMode::TrackMutes {
                    return PerformAction {
                        track_mute_request: self.track_mute_request(position, ctx.project_tracks),
                        ..PerformAction::default()
                    };
                }
            }
            PerformMsg::SetInstrumentTargetOverlay(visible) => {
                if ctx.workspace_visible || !visible {
                    self.instrument_target_overlay = visible;
                }
            }
            PerformMsg::SelectInstrumentTarget(track_id) => {
                if ctx.workspace_visible
                    && ctx
                        .project_tracks
                        .iter()
                        .any(|track| track.id == track_id && track.is_playable_midi_target())
                {
                    if ctx.selected_project_track != Some(track_id) {
                        self.sync_instrument_target(Some(track_id));
                    }
                    return PerformAction {
                        select_project_track: Some(track_id),
                        ..PerformAction::default()
                    };
                }
            }
            PerformMsg::SetFixedComputerVelocity(velocity) => {
                self.set_fixed_computer_velocity(velocity);
                return PerformAction {
                    persist_settings: true,
                    ..PerformAction::default()
                };
            }
            PerformMsg::ToggleFullLevel => {
                if ctx.workspace_visible && self.mode == PerformMode::Instrument {
                    self.toggle_full_level();
                }
            }
            PerformMsg::ToggleSixteenLevels => {
                if ctx.workspace_visible && self.mode == PerformMode::Instrument {
                    self.toggle_sixteen_levels();
                }
            }
            PerformMsg::SelectSixteenLevelsParameter(parameter) => {
                if ctx.workspace_visible && self.mode == PerformMode::Instrument {
                    self.select_sixteen_levels_parameter(parameter);
                }
            }
            PerformMsg::SetSixteenLevelsMinimum(minimum) => {
                if ctx.workspace_visible && self.mode == PerformMode::Instrument {
                    self.set_sixteen_levels_minimum(minimum);
                }
            }
            PerformMsg::SetSixteenLevelsMaximum(maximum) => {
                if ctx.workspace_visible && self.mode == PerformMode::Instrument {
                    self.set_sixteen_levels_maximum(maximum);
                }
            }
            PerformMsg::ChooseSixteenLevelsSource => {
                if ctx.workspace_visible && self.mode == PerformMode::Instrument {
                    self.begin_choosing_sixteen_levels_source();
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
                        ..PerformAction::default()
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
                let selected_instrument_target = (self.mode == PerformMode::Instrument
                    && self.instrument_target_overlay)
                    .then(|| {
                        self.track_for_instrument_target_pad(position, ctx.project_tracks)
                            .map(|track| track.id)
                    })
                    .flatten();
                if let Some(track_id) = selected_instrument_target {
                    self.sync_instrument_target(Some(track_id));
                }
                let instrument_note = if self.mode == PerformMode::Instrument
                    && !self.instrument_target_overlay
                {
                    let velocity = self.fixed_computer_velocity();
                    self.instrument_target()
                        .filter(|track_id| {
                            ctx.project_tracks.iter().any(|track| {
                                track.id == *track_id && track.is_playable_midi_target()
                            })
                        })
                        .map(|track_id| self.resolve_instrument_note(position, velocity, track_id))
                } else {
                    None
                };
                if let Some(note) = instrument_note {
                    engine.send(vibez_engine::commands::EngineCommand::ExternalNoteOn {
                        track_id: note.track_id,
                        pitch: note.pitch,
                        velocity: note.velocity,
                    });
                }
                self.active_computer_keys
                    .insert(key_id, (position, source, instrument_note));
                let track_mute_request = (self.mode == PerformMode::TrackMutes)
                    .then(|| self.track_mute_request(position, ctx.project_tracks))
                    .flatten();
                let section_launch = if self.mode == PerformMode::Sections {
                    let slot = u16::from(self.banks.sections) * 16 + position.index() as u16;
                    self.sections.at_slot(slot).map(|section| section.id)
                } else {
                    None
                };
                if let Some(section_id) = section_launch {
                    self.select_section(section_id, ctx.selected_project_track);
                }
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
                    track_mute_request,
                    select_project_track: selected_instrument_target,
                    section_launch,
                    section_content_changed: None,
                };
            }
            PerformMsg::ComputerKeyReleased {
                key_id,
                occurred_at,
            } => {
                let Some((position, source, instrument_note)) =
                    self.active_computer_keys.remove(&key_id)
                else {
                    return PerformAction::default();
                };
                if let Some(note) = instrument_note {
                    engine.send(vibez_engine::commands::EngineCommand::ExternalNoteOff {
                        track_id: note.track_id,
                        pitch: note.pitch,
                    });
                }
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
                    ..PerformAction::default()
                };
            }
            PerformMsg::WindowUnfocused => {
                self.instrument_target_overlay = false;
                for (_, (_, _, instrument_note)) in self.active_computer_keys.drain() {
                    if let Some(note) = instrument_note {
                        engine.send(vibez_engine::commands::EngineCommand::ExternalNoteOff {
                            track_id: note.track_id,
                            pitch: note.pitch,
                        });
                    }
                }
            }
        }
        PerformAction::default()
    }

    pub fn is_pad_pressed(&self, position: PadPosition) -> bool {
        self.active_computer_keys
            .values()
            .any(|(pressed, _, _)| *pressed == position)
    }

    fn select_section(&mut self, id: SectionId, selected_track: Option<TrackId>) {
        if let Some(section) = self.sections.by_id(id) {
            self.selected_section = Some(id);
            self.section_editor
                .load(Arc::clone(&section.timeline), selected_track);
            self.editing_section_name = None;
            self.section_name_edit = section.name.clone();
            self.editor_focus = PerformEditorFocus::SectionConstruction;
        }
    }

    pub fn sync_project_tracks(&mut self, tracks: &[ProjectTrack]) {
        self.sync_track_mute_slots(tracks);
    }

    pub fn sync_selected_section_editor(&mut self, selected_track: Option<TrackId>) {
        if let Some(section) = self.selected_section.and_then(|id| self.sections.by_id(id)) {
            self.section_editor
                .load(Arc::clone(&section.timeline), selected_track);
        } else {
            self.selected_section = None;
            self.section_editor.clear();
        }
    }

    pub fn commit_selected_section_timeline(&mut self) {
        let Some(id) = self.selected_section else {
            return;
        };
        let timeline = Arc::clone(&self.section_editor.editor().timeline);
        if let Some(section) = Arc::make_mut(&mut self.sections).by_id_mut(id) {
            section.timeline = timeline;
        }
    }
}

#[cfg(test)]
#[path = "perform_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "perform_focus_tests.rs"]
mod focus_tests;
