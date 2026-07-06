use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::track::{ClipInfo, DrumPadState, MediaSourceRef};
use vibez_dropbox::{AccountInfo, DropboxEntry, Tokens as DropboxTokens};
use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::PluginId;
use vibez_project::Project;

use crate::state::{
    ArrangementSelection, ContextMenuTarget, DetailPanelTab, DeviceMenuCategory,
    SampleBrowserEntry, SettingsTab, SnapGrid, Workspace,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumPadParam {
    Gain,
    Pan,
    Start,
    End,
    CoarseTune,
    FineTune,
}

#[derive(Debug, Clone)]
pub struct LoadedClipData {
    pub info: ClipInfo,
    /// Audio the clip's geometry fields refer to. For warped clips
    /// this is the re-stretched buffer, not the raw file contents.
    pub audio: Arc<DecodedAudio>,
    /// Raw un-warped audio, retained when `info.warped` so later
    /// re-warps stretch from the original.
    pub original_audio: Option<Arc<DecodedAudio>>,
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

/// Successful background result from `quantize_audio_clip_async`.
#[derive(Debug, Clone)]
pub struct AudioQuantizeSuccess {
    pub new_clip_id: ClipId,
    pub new_audio: Arc<DecodedAudio>,
    pub new_name: String,
    pub new_position: u64,
    pub new_duration: u64,
    pub slice_count: usize,
    pub grid_label: String,
}

/// Successful background result from `warp_clip_async`. The UI
/// installs the new audio via `EngineCommand::ReplaceClipAudio` and
/// updates per-clip warp metadata.
#[derive(Debug, Clone)]
pub struct ClipWarpSuccess {
    pub audio: Arc<DecodedAudio>,
    /// Original un-warped audio captured for later re-warp / undo.
    pub original_audio: Arc<DecodedAudio>,
    pub new_duration: u64,
    pub new_source_offset: u64,
    pub new_loop_start: u64,
    pub new_loop_end: u64,
    pub detected_bpm: f64,
    pub warped_to_bpm: f64,
}

/// Outcome of an auto-warp-on-import pass.
#[derive(Debug, Clone)]
pub enum AutoWarpOutcome {
    /// The detector refused to commit to a BPM (silence, sparse pad,
    /// too short). Nothing to apply.
    NotDetected,
    /// Detected a BPM but confidence fell below the user's threshold;
    /// record it for manual use but do not warp automatically.
    DetectedOnly { bpm: f64, confidence: f32 },
    /// Detected and warped.
    Warped {
        confidence: f32,
        success: ClipWarpSuccess,
    },
}

#[derive(Debug, Clone)]
pub enum BrowserImportTarget {
    ArrangementClip(Option<TrackId>),
    /// Drop a sample as an arrangement clip at a specific sample position.
    ArrangementClipAt {
        track_id: TrackId,
        position_samples: u64,
    },
    Sampler(TrackId),
    DrumRackPad {
        track_id: TrackId,
        pad_index: usize,
    },
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
    /// Transport domain (playback, tempo, arrangement loop).
    Transport(crate::domains::transport::TransportMsg),

    // Workspace
    SwitchWorkspace(Workspace),

    // Engine events
    Tick,
    EngineMetering {
        peak_l: f32,
        peak_r: f32,
    },

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
    SetDrumPadParam {
        track_id: TrackId,
        pad_index: usize,
        param: DrumPadParam,
        value: f32,
    },
    SetDrumPadOneShot {
        track_id: TrackId,
        pad_index: usize,
        one_shot: bool,
    },
    SetDrumPadChokeGroup {
        track_id: TrackId,
        pad_index: usize,
        choke_group: Option<u8>,
    },

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

    // Time selection + context menu
    SetTimeSelection {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
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
        track_id: Option<TrackId>,
    },
    SplitClipsAtRegion {
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
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
    DeleteKeyPressed,
    /// Preview a pitch on a track's instrument (piano-roll key press
    /// or drum pad click). `on: false` releases it.
    AuditionNote {
        track_id: TrackId,
        pitch: u8,
        on: bool,
    },
    WindowResized(f32, f32),
    MouseReleased,

    // File menu
    NewProject,
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
    /// Settings: toggle auto-warp-on-import.
    ToggleAutoWarpOnImport,
    /// Settings: set warp detection confidence threshold.
    SetWarpConfidenceThreshold(f32),
    /// Settings: re-warp every warped clip to the current project
    /// tempo. Uses each clip's retained `original_audio` when
    /// available.
    RewarpAllClips,
    /// Settings: refresh the list of visible MIDI input ports.
    RescanMidiInputs,
    /// Settings: open a MIDI input port by name.
    OpenMidiInput(String),
    /// Settings: close the currently-open MIDI input port.
    CloseMidiInput,
    AddSampleLibraryRoot,
    SampleLibraryRootSelected(Option<PathBuf>),
    RemoveSampleLibraryRoot(PathBuf),
    RescanSampleLibrary,
    SampleLibraryScanned(Result<SampleLibraryScanResult, String>),
    SampleBrowserSearchChanged(String),
    SelectSampleBrowserRoot(Option<PathBuf>),
    SelectSampleBrowserEntry(MediaSourceRef),
    /// Click in the Local sample browser: select only, no preview.
    ClickLocalBrowserEntry(MediaSourceRef),
    /// Speaker-icon click on a Local browser row: audition.
    PreviewLocalEntry(MediaSourceRef),
    LocalSamplePreviewReady(Result<Arc<DecodedAudio>, String>),

    // Drag-and-drop from sample browser
    StartDragSample {
        source: MediaSourceRef,
        label: String,
    },
    EndDragSample,
    /// Fired by a clip canvas whenever the cursor moves while a sample
    /// drag is in flight and the cursor is inside that lane.
    DragHoverTrack {
        track_id: TrackId,
        beat: f64,
    },
    DropSampleOnArrangement {
        track_id: TrackId,
        position_samples: u64,
    },
    DropSampleOnDrumPad {
        track_id: TrackId,
        pad_index: usize,
    },
    DropSampleOnSampler {
        track_id: TrackId,
    },
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
    ScanPluginsComplete(vibez_plugin_host::ScanReport),
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

    // Quantize
    QuantizeNoteClip {
        track_id: TrackId,
        clip_id: ClipId,
    },
    QuantizeAudioClip {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Quantize an audio clip with an explicit grid, bypassing the
    /// piano-roll snap setting.
    QuantizeAudioClipAt {
        track_id: TrackId,
        clip_id: ClipId,
        grid: crate::state::SnapGrid,
    },
    /// Background audio-quantize computation finished.
    AudioQuantizeReady {
        track_id: TrackId,
        old_clip_id: ClipId,
        result: Result<AudioQuantizeSuccess, String>,
    },

    // -- Warping (manual + auto) --
    /// Kick off a background BPM detection for the given clip.
    DetectClipBpm {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Background BPM detection result. `bpm` is `None` when the
    /// detector refused to commit (silence, sparse pad, too short).
    ClipBpmDetected {
        track_id: TrackId,
        clip_id: ClipId,
        bpm: Option<f64>,
        confidence: f32,
    },
    /// Manual BPM text field input change (ephemeral, before commit).
    ClipBpmInputChanged {
        track_id: TrackId,
        clip_id: ClipId,
        text: String,
    },
    /// Commit a manually-entered nominal BPM for the clip.
    SetClipNominalBpm {
        track_id: TrackId,
        clip_id: ClipId,
        bpm: f64,
    },
    /// Parse the in-progress `clip_bpm_edit` text and commit it as the
    /// clip's nominal BPM (wired to the BPM text input's Enter key).
    SubmitClipBpm {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Kick off a background warp-to-project-tempo for the clip.
    WarpClipToProject {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Background warp result.
    ClipWarpReady {
        track_id: TrackId,
        clip_id: ClipId,
        result: Result<ClipWarpSuccess, String>,
    },
    /// Revert the clip's audio to the un-warped `original_audio` and
    /// clear warp metadata.
    ClearClipWarp {
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Auto-warp pass completed for a freshly-imported clip. The
    /// outcome bundles three cases: the detector refused
    /// (`NotDetected`), detected but confidence below the user's
    /// threshold (`DetectedOnly`), or detected and warped
    /// (`Warped`).
    ClipAutoWarpReady {
        track_id: TrackId,
        clip_id: ClipId,
        outcome: AutoWarpOutcome,
    },

    // Undo / redo
    Undo,
    Redo,

    // Export
    ExportProject,
    ExportPathSelected(Option<PathBuf>),
    ExportComplete(Result<PathBuf, String>),

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
