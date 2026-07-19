use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, SectionId, TrackId};
use vibez_core::midi::InstrumentKind;
use vibez_core::track::{ClipInfo, DrumPadState, MediaSourceRef};
use vibez_dropbox::{AccountInfo, DropboxEntry, Tokens as DropboxTokens};
use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::PluginId;
use vibez_project::project_format_v1::SaveObservation;
use vibez_project::{Project, TimelineLocation};

/// Cloneable UI-message holder for one prepared, uniquely-owned Section.
/// The router takes the box exactly once before sending it to the engine.
#[derive(Clone)]
pub struct ResidentSection(
    Arc<
        std::sync::Mutex<Option<Box<vibez_engine::playback_source::PreparedSectionPlaybackSource>>>,
    >,
);

impl ResidentSection {
    pub fn new(
        prepared: Box<vibez_engine::playback_source::PreparedSectionPlaybackSource>,
    ) -> Self {
        Self(Arc::new(std::sync::Mutex::new(Some(prepared))))
    }

    pub fn take(
        &self,
    ) -> Option<Box<vibez_engine::playback_source::PreparedSectionPlaybackSource>> {
        self.0.lock().ok()?.take()
    }
}

impl std::fmt::Debug for ResidentSection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResidentSection")
    }
}

use crate::state::{
    AuditionImportInput, AuditionMode, SampleBrowserEntry, SampleBrowserFolder, SettingsTab,
    UndoGestureId,
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
pub struct LoadedTimelineClip {
    pub location: TimelineLocation,
    pub clip: LoadedClipData,
}

impl std::ops::Deref for LoadedTimelineClip {
    type Target = LoadedClipData;

    fn deref(&self) -> &Self::Target {
        &self.clip
    }
}

#[derive(Debug, Clone)]
pub struct UnresolvedTimelineClip {
    pub location: TimelineLocation,
    pub info: ClipInfo,
}

impl std::ops::Deref for UnresolvedTimelineClip {
    type Target = ClipInfo;

    fn deref(&self) -> &Self::Target {
        &self.info
    }
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
    pub folders: Vec<SampleBrowserFolder>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum LocalRootWatchEvent {
    Changed(Vec<PathBuf>),
    Watching(Vec<PathBuf>),
    Failed {
        roots: Vec<PathBuf>,
        message: String,
    },
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
pub struct RemoteMaterializedSample {
    pub audio: Arc<DecodedAudio>,
    pub name: String,
    pub source: MediaSourceRef,
    pub lease: vibez_dropbox::CacheLease,
    pub metadata: vibez_dropbox::DerivedMetadata,
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
    SectionClipAt {
        section_id: SectionId,
        track_id: TrackId,
        position_samples: u64,
    },
    ArrangementNewTrackAt {
        position_samples: u64,
    },
    Sampler(TrackId),
    DrumRackPad {
        track_id: TrackId,
        pad_index: usize,
    },
}

#[derive(Debug, Clone)]
pub struct PreparedBrowserImport {
    pub treatment: AuditionImportInput,
    pub audio: Arc<DecodedAudio>,
    pub original_audio: Option<Arc<DecodedAudio>>,
    pub name: String,
    pub source: MediaSourceRef,
}

#[derive(Debug, Clone)]
pub struct ProjectLoadResult {
    pub path: PathBuf,
    pub project: Project,
    pub clips: Vec<LoadedTimelineClip>,
    /// Clips whose media could not be hydrated this session. They stay out
    /// of the arrangement but must survive the next save for relinking.
    pub unresolved_clips: Vec<UnresolvedTimelineClip>,
    pub sampler_samples: Vec<LoadedSamplerData>,
    pub drum_rack_pad_samples: Vec<LoadedDrumRackPadData>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectSaveResult {
    pub path: PathBuf,
    pub project: Project,
    pub observation: Option<SaveObservation>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// One incremental update within a continuous pointer edit. Messages from
    /// the same gesture share one pre-edit undo snapshot.
    UndoGesture {
        id: UndoGestureId,
        edit: Box<Message>,
    },
    /// Transport domain (playback, tempo, arrangement loop).
    Transport(crate::domains::transport::TransportMsg),
    /// Devices domain (effect chain, instruments, drum pads, menu).
    Devices(crate::domains::devices::DevicesMsg),
    /// Arrangement domain (tracks, selection; clips arriving next).
    Arrangement(crate::domains::arrangement::ArrangementMsg),
    PianoRoll(crate::domains::piano_roll::PianoRollMsg),
    Browser(crate::domains::browser::BrowserMsg),
    Project(crate::domains::project::ProjectMsg),
    Automation(crate::domains::automation::AutomationMsg),
    Perform(crate::domains::perform::PerformMsg),
    SectionResidencyReady {
        request_id: u64,
        section_id: SectionId,
        quantization: vibez_core::perform::SectionLaunchQuantization,
        resident: ResidentSection,
    },
    KeyboardInput {
        event: iced::keyboard::Event,
        occurred_at: std::time::Instant,
    },
    View(crate::domains::view::ViewMsg),

    // Workspace

    // Engine events
    Tick,
    EngineMetering {
        peak_l: f32,
        peak_r: f32,
    },

    // Multi-track
    AddClipToTrack(TrackId),
    ClipFileSelected(TrackId, Option<PathBuf>),
    ClipAudioDecoded(TrackId, ClipId, Arc<DecodedAudio>, String, MediaSourceRef),
    ClipDecodeError(TrackId, String),

    // Track controls

    // Per-track metering

    // Effects

    // Instrument tracks

    // Sampler
    LoadSamplerSample(TrackId),
    SamplerFileSelected(TrackId, Option<PathBuf>),
    SamplerSampleDecoded(TrackId, Arc<DecodedAudio>, String, MediaSourceRef),
    SamplerDecodeError(TrackId, String),
    LoadDrumRackPadSample(TrackId, usize),
    DrumRackPadFileSelected(TrackId, usize, Option<PathBuf>),
    DrumRackPadSampleDecoded(TrackId, usize, Arc<DecodedAudio>, String, MediaSourceRef),
    DrumRackPadDecodeError(TrackId, usize, String),

    // Zoom / scroll

    // Snap grid

    // Detail panel tabs

    // Arrangement loop

    // Time selection + context menu

    // Track reordering

    // Renaming

    // MIDI track (no auto-synth)

    // Instrument attach/detach

    // Device context menu

    // Cursor tracking
    DeleteKeyPressed,

    // File menu
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ProjectOpenPathSelected(Option<PathBuf>),
    ProjectSavePathSelected(Option<PathBuf>),
    ProjectLoaded(Box<Result<ProjectLoadResult, String>>),
    ProjectSaved(Box<Result<ProjectSaveResult, String>>),
    /// Settings: toggle auto-warp-on-import.
    ToggleAutoWarpOnImport,
    /// Settings: set warp detection confidence threshold.
    SetWarpConfidenceThreshold(f32),
    /// Settings: ask before deleting a Project Track everywhere.
    ToggleProjectTrackDeleteConfirmation,
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
    /// Appearance: activate a theme by name (built-in or user).
    SelectTheme(String),
    /// Appearance: rescan the themes directory for `.vzt` files.
    RescanThemes,
    /// Appearance: live edit of the save-as-theme name field.
    ThemeSaveNameChanged(String),
    /// Appearance: save the current palette as a user `.vzt`.
    SaveCurrentTheme,
    AddSampleLibraryRoot,
    SampleLibraryRootSelected(Option<PathBuf>),
    RescanSampleLibrary,
    /// Select a Local source; starts RAW Audition when selection-follow is on.
    ClickLocalBrowserEntry(MediaSourceRef),
    /// Mouse-down creates a Pending Drag at the globally tracked cursor.
    BeginPendingBrowserDrag(MediaSourceRef, String),
    /// Explicit Play in the persistent Audition footer.
    PreviewLocalEntry(MediaSourceRef),
    StopBrowserPreview,
    ToggleAuditionEnabled,
    SetAuditionGain(f32),
    SetAuditionMode(AuditionMode),
    SetAuditionSync(vibez_engine::commands::AuditionSync),
    ToggleAuditionLoop,
    AuditionBpmEditChanged(String),
    ConfirmAuditionBpm,
    EscapePressed,
    /// The `u64` is the audition request generation minted at spawn
    /// time; stale completions (stopped or superseded requests) are
    /// dropped instead of starting playback.
    LocalSamplePreviewReady(MediaSourceRef, u64, Result<Arc<DecodedAudio>, String>),
    BrowserWaveformReady(MediaSourceRef, Result<Arc<DecodedAudio>, String>),
    BrowserBpmDetected(MediaSourceRef, Option<(f64, f32)>),
    BrowserAuditionWarpReady {
        source: MediaSourceRef,
        generation: u64,
        source_bpm: f64,
        project_bpm: f64,
        result: Result<Arc<DecodedAudio>, String>,
    },
    DropSampleOnArrangement {
        track_id: TrackId,
        position_samples: u64,
    },
    DropSampleOnEmptyArrangement,
    DropSampleOnDrumPad {
        track_id: TrackId,
        pad_index: usize,
    },
    DropSampleOnSampler {
        track_id: TrackId,
    },
    ImportSelectedBrowserSampleToArrangement,
    SelectAdjacentBrowserResult(i8),
    LoadSelectedBrowserSampleToDevice,
    BrowserSampleDecoded(
        BrowserImportTarget,
        AuditionImportInput,
        Arc<DecodedAudio>,
        String,
        MediaSourceRef,
    ),
    BrowserImportPrepared {
        target: BrowserImportTarget,
        /// Import generation at spawn time; a New Project or cancelled
        /// import bumps the app counter so the prepared clip is dropped
        /// instead of landing in a reset project.
        generation: u64,
        payload: PreparedBrowserImport,
    },
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
    /// Commit a manually-entered nominal BPM for the clip.
    /// Parse the in-progress `clip_bpm_edit` text and commit it as the
    /// clip's nominal BPM (wired to the BPM text input's Enter key).
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

    // Export
    ExportProject,
    ExportPathSelected(Option<PathBuf>),
    ExportComplete(Result<PathBuf, String>),

    // Dropbox
    SaveDropboxAppKey,
    ConnectDropbox,
    DropboxConnected(Result<DropboxConnectOutcome, String>),
    DisconnectDropbox,
    RefreshRemoteConnection,
    RemoteCatalogPageFetched {
        generation: u64,
        completed_pages: usize,
        result:
            Result<crate::remote_provider::RemotePage, crate::remote_provider::RemoteProviderError>,
    },
    RemoteCatalogSaved {
        generation: u64,
        /// `Some` continues pagination from this checkpoint after a
        /// successful progress save.
        next_checkpoint: Option<String>,
        result: Result<(), String>,
    },
    SetMediaCacheBudgetGiB(f32),
    ToggleMediaCacheAutomaticEviction,
    ClearMediaCache,
    MediaCacheMaintenanceComplete(Result<vibez_dropbox::CacheUsage, String>),
    MediaCacheCleared(Result<(vibez_dropbox::CacheClearReport, vibez_dropbox::CacheUsage), String>),
    ClickRemoteBrowserEntry(crate::remote_provider::RemoteCatalogEntry),
    RemoteAuditionReady {
        request_id: u64,
        /// Audition request generation minted at spawn time (see
        /// [`Message::LocalSamplePreviewReady`]).
        generation: u64,
        source: MediaSourceRef,
        result: Result<RemoteMaterializedSample, String>,
    },
    RemoteImportReady {
        request_id: u64,
        target: BrowserImportTarget,
        treatment: AuditionImportInput,
        result: Result<(Arc<DecodedAudio>, String, MediaSourceRef), String>,
    },
    DropboxPreview(DropboxEntry),
    DropboxImportToArrangement(DropboxEntry),
    DropboxImportToDevice(DropboxEntry),
}

/// Arity-preserving constructor helpers so call sites read cleanly
/// and mechanical migrations stay parenthesis-balanced.
impl Message {
    pub fn in_undo_gesture(self, id: UndoGestureId) -> Self {
        Self::UndoGesture {
            id,
            edit: Box::new(self),
        }
    }

    pub fn add_effect(t: TrackId, e: EffectType) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::AddEffect(t, e))
    }
    pub fn remove_effect(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::RemoveEffect(t, e))
    }
    pub fn set_effect_param(t: TrackId, e: EffectId, i: usize, v: f32) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetEffectParam(
            t, e, i, v,
        ))
    }
    pub fn set_effect_params(t: TrackId, e: EffectId, updates: Vec<(usize, f32)>) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetEffectParams(
            t, e, updates,
        ))
    }
    pub fn toggle_effect_bypass(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::ToggleEffectBypass(
            t, e,
        ))
    }
    pub fn move_effect_up(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::MoveEffectUp(t, e))
    }
    pub fn move_effect_down(t: TrackId, e: EffectId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::MoveEffectDown(t, e))
    }
    pub fn set_track_instrument(t: TrackId, k: InstrumentKind) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetTrackInstrument(
            t, k,
        ))
    }
    pub fn remove_track_instrument(t: TrackId) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::RemoveTrackInstrument(
            t,
        ))
    }
    pub fn set_instrument_param(t: TrackId, i: usize, v: f32) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetInstrumentParam(
            t, i, v,
        ))
    }
    pub fn select_drum_rack_pad(t: TrackId, p: usize) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SelectDrumRackPad(t, p))
    }
    pub fn clear_drum_rack_pad(t: TrackId, p: usize) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::ClearDrumRackPad(t, p))
    }
    pub fn audition_note(track_id: TrackId, pitch: u8, on: bool) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::AuditionNote {
            track_id,
            pitch,
            on,
        })
    }
    pub fn dismiss_device_menu() -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::DismissContextMenu)
    }
    pub fn set_device_menu_category(c: crate::state::DeviceMenuCategory) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::SetMenuCategory(c))
    }
    pub fn device_menu_search(q: String) -> Self {
        Self::Devices(crate::domains::devices::DevicesMsg::MenuSearch(q))
    }
}

impl Message {
    pub fn select_track(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SelectTrack(t))
    }
    pub fn remove_track(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RequestRemoveTrack(t))
    }
    pub fn rename_track(t: TrackId, n: String) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RenameTrack(
            t, n,
        ))
    }
    pub fn rename_clip(t: TrackId, c: ClipId, n: String) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RenameClip(
            t, c, n,
        ))
    }
    pub fn move_track_up(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::MoveTrackUp(t))
    }
    pub fn move_track_down(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::MoveTrackDown(
            t,
        ))
    }
    pub fn set_track_gain(t: TrackId, g: f32) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackGain(
            t, g,
        ))
    }
    pub fn set_track_pan(t: TrackId, p: f32) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackPan(
            t, p,
        ))
    }
    pub fn set_track_mute(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackMute(t))
    }
    pub fn set_track_solo(t: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTrackSolo(t))
    }
    pub fn add_bus() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::AddBus)
    }
    pub fn remove_bus(bus: TrackId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RemoveBus(bus))
    }
    pub fn set_send(t: TrackId, bus: TrackId, amount: f32) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetSend {
            track_id: t,
            bus_id: bus,
            amount,
        })
    }
}

impl Message {
    pub fn remove_clip(t: TrackId, c: ClipId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::RemoveClip(
            t, c,
        ))
    }
    pub fn toggle_clip_loop(t: TrackId, c: ClipId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::ToggleClipLoop(
            t, c,
        ))
    }
    pub fn set_time_selection_active(a: bool) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SetTimeSelectionActive(a))
    }
    pub fn duplicate_note_clip(t: TrackId, c: ClipId) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::DuplicateNoteClip(t, c))
    }
    pub fn split_audio_clip(t: TrackId, c: ClipId, split_position: u64) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::SplitAudioClip {
                track_id: t,
                clip_id: c,
                split_position,
            },
        )
    }
    pub fn split_note_clip(t: TrackId, c: ClipId, split_beat: f64) -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SplitNoteClip {
            track_id: t,
            clip_id: c,
            split_beat,
        })
    }
    pub fn split_selected_at_playhead() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::SplitSelectedAtPlayhead)
    }
    pub fn join_selected_clips() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::JoinSelectedClips)
    }
    pub fn delete_clips_in_region(
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::DeleteClipsInRegion {
                start_beats,
                end_beats,
                track_id,
            },
        )
    }
    pub fn split_clips_at_region(
        start_beats: f64,
        end_beats: f64,
        track_id: Option<TrackId>,
    ) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::SplitClipsAtRegion {
                start_beats,
                end_beats,
                track_id,
            },
        )
    }
    pub fn create_clip_from_selection() -> Self {
        Self::Arrangement(crate::domains::arrangement::ArrangementMsg::CreateClipFromSelection)
    }
    pub fn create_note_clip_from_selection(t: TrackId) -> Self {
        Self::Arrangement(
            crate::domains::arrangement::ArrangementMsg::CreateNoteClipFromSelection(t),
        )
    }
}
