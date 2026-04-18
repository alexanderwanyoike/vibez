use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::track::{ClipInfo, DrumPadState, MediaSourceRef};
use vibez_dropbox::{AccountInfo, DropboxEntry, Tokens as DropboxTokens};
use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::{PluginId, PluginInfo};
use vibez_project::Project;

use crate::state::{
    ArrangementSelection, ContextMenuTarget, DetailPanelTab, DeviceMenuCategory,
    SampleBrowserEntry, SettingsTab, SnapGrid, Workspace,
};

#[derive(Debug, Clone)]
pub struct LoadedClipData {
    pub info: ClipInfo,
    pub audio: Arc<DecodedAudio>,
}

#[derive(Debug, Clone)]
pub struct LoadedSamplerData {
    pub track_id: TrackId,
    pub source: MediaSourceRef,
    pub audio: Arc<DecodedAudio>,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct LoadedDrumRackPadData {
    pub track_id: TrackId,
    pub pad_index: usize,
    pub source: MediaSourceRef,
    pub audio: Arc<DecodedAudio>,
    pub name: String,
    pub state: DrumPadState,
}

#[derive(Debug, Clone)]
pub struct SampleLibraryScanResult {
    pub entries: Vec<SampleBrowserEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BounceOutcome {
    pub audio: Arc<DecodedAudio>,
    pub source: MediaSourceRef,
    pub path: PathBuf,
    pub clip_name: String,
    pub insert_position_samples: u64,
    pub warnings: Vec<String>,
}

/// OAuth flow success payload passed to the UI as `Message::DropboxConnected`.
#[derive(Debug, Clone)]
pub struct DropboxConnectOutcome {
    pub info: AccountInfo,
    pub tokens: DropboxTokens,
}

#[derive(Debug, Clone)]
pub enum BrowserImportTarget {
    ArrangementClip(Option<TrackId>),
    Sampler(TrackId),
    DrumRackPad { track_id: TrackId, pad_index: usize },
}

#[derive(Debug, Clone)]
pub struct ProjectLoadResult {
    pub path: PathBuf,
    pub project: Project,
    pub clips: Vec<LoadedClipData>,
    pub sampler_samples: Vec<LoadedSamplerData>,
    pub drum_rack_pad_samples: Vec<LoadedDrumRackPadData>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Transport
    Play,
    Stop,
    TogglePlayback,
    Seek(f64),

    // BPM
    BpmChanged(String),
    BpmSubmit,

    // Workspace
    SwitchWorkspace(Workspace),

    // Engine events
    Tick,
    EnginePosition(u64),
    EngineMetering {
        peak_l: f32,
        peak_r: f32,
    },
    EngineStopped,

    // Multi-track
    AddTrack,
    RemoveTrack(TrackId),
    SelectTrack(TrackId),
    AddClipToTrack(TrackId),
    ClipFileSelected(TrackId, Option<PathBuf>),
    ClipAudioDecoded(TrackId, ClipId, Arc<DecodedAudio>, String, MediaSourceRef),
    ClipDecodeError(TrackId, String),
    RemoveClip(TrackId, ClipId),

    // Track controls
    SetTrackGain(TrackId, f32),
    SetTrackPan(TrackId, f32),
    SetTrackMute(TrackId),
    SetTrackSolo(TrackId),

    // Per-track metering
    EngineTrackMeter {
        track_id: TrackId,
        peak_l: f32,
        peak_r: f32,
    },

    // Effects
    AddEffect(TrackId, EffectType),
    RemoveEffect(TrackId, EffectId),
    SetEffectParam(TrackId, EffectId, usize, f32),
    ToggleEffectBypass(TrackId, EffectId),
    MoveEffectUp(TrackId, EffectId),
    MoveEffectDown(TrackId, EffectId),

    // Instrument tracks
    AddInstrumentTrack,
    SetInstrumentParam(TrackId, usize, f32),

    // Sampler
    LoadSamplerSample(TrackId),
    SamplerFileSelected(TrackId, Option<PathBuf>),
    SamplerSampleDecoded(TrackId, Arc<DecodedAudio>, String, MediaSourceRef),
    SamplerDecodeError(TrackId, String),
    LoadDrumRackPadSample(TrackId, usize),
    DrumRackPadFileSelected(TrackId, usize, Option<PathBuf>),
    DrumRackPadSampleDecoded(TrackId, usize, Arc<DecodedAudio>, String, MediaSourceRef),
    DrumRackPadDecodeError(TrackId, usize, String),
    ClearDrumRackPad(TrackId, usize),
    SelectDrumRackPad(TrackId, usize),

    // Zoom / scroll
    ZoomIn,
    ZoomOut,
    SetZoom(f32),
    ZoomToFit,
    ScrollArrangement(f64),

    // Snap grid
    SetSnapGrid(SnapGrid),

    // Clip looping
    ToggleClipLoop(TrackId, ClipId),
    SetClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start: u64,
        loop_end: u64,
    },
    ToggleNoteClipLoop(TrackId, ClipId),
    SetNoteClipLoopRegion {
        track_id: TrackId,
        clip_id: ClipId,
        loop_start_beats: f64,
        loop_end_beats: f64,
    },

    // Piano roll / note clips
    AddNoteClipToTrack(TrackId),
    SelectNoteClip(TrackId, ClipId),
    AddNote {
        track_id: TrackId,
        clip_id: ClipId,
        pitch: u8,
        start_beat: f64,
        duration_beats: f64,
    },
    RemoveNote(TrackId, ClipId, usize),
    EditNote(TrackId, ClipId, usize, MidiNote),
    SelectNote(TrackId, ClipId, Option<usize>, bool),
    SelectAllNotes(TrackId, ClipId),
    RemoveSelectedNotes(TrackId, ClipId),
    NudgeSelectedNotes {
        track_id: TrackId,
        clip_id: ClipId,
        delta_beats: f64,
        delta_semitones: i8,
    },
    /// Batch-move notes to absolute positions (used by multi-note drag on release).
    MoveNotesAbsolute {
        track_id: TrackId,
        clip_id: ClipId,
        /// (note_index, new_start_beat, new_pitch)
        moves: Vec<(usize, f64, u8)>,
    },

    // Clip operations
    DuplicateNoteClip(TrackId, ClipId),
    DoubleNoteClip(TrackId, ClipId),
    CropNoteClip(TrackId, ClipId),

    // Piano roll scroll
    PianoRollScrollY(f32),

    // Arrangement clip interaction
    SelectArrangementClip {
        selection: ArrangementSelection,
        shift_held: bool,
    },
    MoveAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_position: u64,
    },
    MoveNoteClipPosition {
        track_id: TrackId,
        clip_id: ClipId,
        new_position_beats: f64,
    },
    ResizeAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        new_duration: u64,
    },
    ResizeNoteClipDuration {
        track_id: TrackId,
        clip_id: ClipId,
        new_duration_beats: f64,
    },
    MoveClipToTrack {
        source_track: TrackId,
        target_track: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
    SplitAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_position: u64,
    },
    SplitNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
        split_beat: f64,
    },
    DeleteSelectedClip,
    DuplicateSelectedClip,
    SplitSelectedAtPlayhead,
    JoinSelectedClips,

    // Detail panel tabs
    SwitchDetailTab(DetailPanelTab),

    // Arrangement loop
    ToggleArrangementLoop,
    SetArrangementLoopRegion {
        start_beats: f64,
        end_beats: f64,
    },

    // Time selection + context menu
    SetTimeSelection {
        start_beats: f64,
        end_beats: f64,
    },
    SetSelectionAsLoop,
    SetTimeSelectionActive(bool),
    ShowContextMenu {
        x: f32,
        y: f32,
        target: ContextMenuTarget,
    },
    DismissContextMenu,
    DeleteClipsInRegion {
        start_beats: f64,
        end_beats: f64,
    },
    SplitClipsAtRegion {
        start_beats: f64,
        end_beats: f64,
    },

    // Clip creation from region
    CreateClipFromSelection,
    CreateNoteClipFromSelection(TrackId),

    // Track reordering
    MoveTrackUp(TrackId),
    MoveTrackDown(TrackId),
    MoveSelectedTrackUp,
    MoveSelectedTrackDown,

    // Renaming
    RenameTrack(TrackId, String),
    RenameClip(TrackId, ClipId, String),
    StartEditingTrackName(TrackId),
    StartEditingClipName(TrackId, ClipId),
    EditNameText(String),
    FinishEditing,
    CancelEditing,

    // MIDI track (no auto-synth)
    AddMidiTrack,

    // Instrument attach/detach
    SetTrackInstrument(TrackId, InstrumentKind),
    RemoveTrackInstrument(TrackId),

    // Pattern halve
    HalveNoteClip(TrackId, ClipId),

    // Edit mode
    TogglePianoRollEditMode,

    // Device context menu
    ShowDeviceContextMenu {
        x: f32,
        y: f32,
        track_id: TrackId,
    },
    DismissDeviceContextMenu,
    SetDeviceMenuCategory(DeviceMenuCategory),
    DeviceMenuSearch(String),

    // Cursor tracking
    CursorMoved(f32, f32),
    MouseReleased,

    // File menu
    NewProject,
    NewProjectFromTemplate(&'static str),
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ToggleFileMenu,
    DismissFileMenu,
    ProjectOpenPathSelected(Option<PathBuf>),
    ProjectSavePathSelected(Option<PathBuf>),
    ProjectLoaded(Result<ProjectLoadResult, String>),
    ProjectSaved(Result<PathBuf, String>),
    ToggleSampleBrowser,
    AddSampleLibraryRoot,
    SampleLibraryRootSelected(Option<PathBuf>),
    RemoveSampleLibraryRoot(PathBuf),
    RescanSampleLibrary,
    SampleLibraryScanned(Result<SampleLibraryScanResult, String>),
    SampleBrowserSearchChanged(String),
    SelectSampleBrowserRoot(Option<PathBuf>),
    SelectSampleBrowserEntry(MediaSourceRef),
    ImportSelectedBrowserSampleToArrangement,
    LoadSelectedBrowserSampleToDevice,
    BrowserSampleDecoded(
        BrowserImportTarget,
        Arc<DecodedAudio>,
        String,
        MediaSourceRef,
    ),
    BrowserSampleDecodeError(String),

    // Settings
    OpenSettings,
    CloseSettings,
    SelectSettingsTab(SettingsTab),
    SetBufferSize(u32),

    // Plugin scanning
    ScanPlugins,
    ScanPluginsComplete(Vec<PluginInfo>),
    AddPluginScanPath,
    PluginScanPathSelected(Option<PathBuf>),
    RemovePluginScanPath(usize),
    ToggleScanDefaultPaths,

    // Plugin loading (via device menu)
    AddPluginToTrack(TrackId, PluginId),
    PluginLoadError(String),

    // Plugin GUI windows
    OpenPluginGui(PluginGuiKey),
    ClosePluginGui(PluginGuiKey),

    // Bounce / resample
    BounceSelectionToAudio,
    BounceClipToAudio {
        track_id: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
    BounceComplete(Result<BounceOutcome, String>),

    // Phrase variation
    GenerateVariations {
        track_id: TrackId,
        clip_id: ClipId,
    },

    // Sample browser mode
    SetSampleBrowserMode(crate::state::SampleBrowserMode),

    // Dropbox
    SetDropboxAppKey(String),
    SaveDropboxAppKey,
    ConnectDropbox,
    DropboxConnected(Result<DropboxConnectOutcome, String>),
    DisconnectDropbox,
    DropboxExpandFolder(String),
    DropboxCollapseFolder(String),
    DropboxFolderListed {
        path: String,
        result: Result<Vec<DropboxEntry>, String>,
    },
    DropboxSelectEntry(DropboxEntry),
    DropboxPreview(DropboxEntry),
    DropboxPreviewReady(Result<Arc<DecodedAudio>, String>),
    DropboxImportToArrangement(DropboxEntry),
    DropboxImportToDevice(DropboxEntry),
}
