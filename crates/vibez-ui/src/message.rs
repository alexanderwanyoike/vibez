//! UI messages and cloneable transient payload owners.

use std::path::PathBuf;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::EffectType;
use vibez_core::id::{ClipId, EffectId, SectionId, TrackId};
use vibez_core::midi::InstrumentKind;
use vibez_core::track::{AudioInputRoute, InputMonitoring, MediaSourceRef};
use vibez_dropbox::DropboxEntry;
use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::PluginId;
use vibez_project::TimelineLocation;

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

use crate::domains::audio_settings::{AudioDeviceChoice, AudioSampleRate};
use crate::state::{AuditionImportInput, AuditionMode, SettingsTab, UndoGestureId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumPadParam {
    Gain,
    Pan,
    Start,
    End,
    FadeIn,
    FadeOut,
    CoarseTune,
    FineTune,
}

/// Menus and dialogs whose lifecycle is owned by their overlay rather than
/// inferred from unrelated application messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOverlay {
    ArrangementContext,
    File,
    Edit,
    /// The About dialog joins the menus rather than the Settings modal so
    /// that Escape and a backdrop press close it, which Settings predates.
    About,
}

#[path = "message_results.rs"]
mod results;
pub use results::*;

#[derive(Debug, Clone)]
pub enum Message {
    /// A message emitted by one of the selected Section timeline canvases.
    /// The router focuses that editor before dispatching the enclosed action.
    SectionTimeline(Box<Message>),
    /// A menu item was chosen. The router dispatches the action, then closes
    /// only the overlay that produced it.
    MenuItemSelected(MenuOverlay, Box<Message>),
    /// Explicit dismissal from an overlay backdrop or another first-class
    /// lifecycle input such as Escape.
    DismissMenu(MenuOverlay),
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
    SetDrumRackSliceMarkers(crate::domains::arrangement::AudioSliceMarkers),
    ConfirmDrumRackSlice,
    CancelDrumRackSlice,
    OpenTransientAnalysisDialog {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
    },
    SetTransientAnalysisSensitivity(u8),
    TransientAnalysisSensitivityInputChanged(String),
    SubmitTransientAnalysisSensitivity,
    ConfirmTransientAnalysis,
    CancelTransientAnalysis,
    AudioClipDrumRackPrepared {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        markers: crate::domains::arrangement::AudioSliceMarkers,
        expected_clip: Box<crate::state::UiClip>,
        result: Result<PreparedDrumRackAudio, String>,
    },
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
    SectionRecordResidencyReady {
        request_id: u64,
        request: crate::domains::perform::SectionRecordStartRequest,
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
    ToggleAudioTrackArm(TrackId),
    SetAudioTrackInputRoute(TrackId, AudioInputRoute),
    SetAudioTrackMonitoring(TrackId, InputMonitoring),
    ToggleAudioRecording,
    AudioRecordingFinalized(Result<AudioRecordingOutcome, String>),

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
    /// Command+A. Context-resolved in `update`, like [`Self::DeleteKeyPressed`]:
    /// the open piano roll's notes first, then the active timeline's clips.
    SelectAllPressed,

    // File menu
    NewProject,
    SelectNewProjectLayout(vibez_project::PerformLayout),
    ConfirmNewProject,
    CancelNewProject,
    ImportLauncherClip {
        track_id: TrackId,
        row: u32,
    },
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ProjectOpenPathSelected(Option<PathBuf>),
    ProjectSavePathSelected(Option<PathBuf>),
    ProjectLoaded {
        path: PathBuf,
        result: Box<Result<ProjectLoadResult, ProjectLoadError>>,
    },
    ProjectSaved(Box<ProjectSaveCompleted>),

    // Window close protection. The window is configured not to exit on its
    // own close request, so every one of these arrives here first.
    /// The user asked to close the window (title bar button, Alt+F4, ...).
    WindowCloseRequested,
    /// Close dialog: save first, then quit. Falls through save-as when the
    /// project has no path yet.
    CloseConfirmSave,
    /// Close dialog: quit and lose the unsaved edits.
    CloseConfirmDiscard,
    /// Close dialog: stay in the project.
    CloseConfirmCancel,
    /// Settings: toggle auto-warp-on-import.
    ToggleAutoWarpOnImport,
    /// Settings: set warp detection confidence threshold.
    SetWarpConfidenceThreshold(f32),
    /// Settings: ask before deleting a Project Track everywhere.
    ToggleProjectTrackDeleteConfirmation,
    /// Settings: save named projects shortly after editing stops.
    ToggleAutoSave,
    /// Settings: resize the whole interface. Distinct from timeline
    /// zoom, which changes visible musical time instead.
    SetInterfaceScale(f32),
    /// Settings: opt in or out of the startup release check. Takes
    /// effect at the next launch; it never triggers a check itself.
    ToggleCheckForUpdates,
    /// User-initiated release check from Settings. Bypasses the once-a-day
    /// startup throttle: an explicit click is consent for one request even
    /// when the automatic startup check is switched off.
    CheckForUpdatesNow,
    /// The startup release check finished. `Some` carries the newest
    /// upstream tag; `None` means the request failed and is reported
    /// only by advancing the throttle.
    UpdateCheckCompleted(Option<String>),
    /// Hide the update notice for the rest of this session.
    DismissUpdateNotice,
    /// Open the releases page in the system browser.
    OpenReleasesPage,
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
    EscapePressed,
    /// The `u64` is the audition request generation minted at spawn
    /// time; stale completions (stopped or superseded requests) are
    /// dropped instead of starting playback.
    LocalSamplePreviewReady(MediaSourceRef, u64, Result<AnalysedBrowserAudio, String>),
    BrowserWaveformReady(MediaSourceRef, Result<AnalysedBrowserAudio, String>),
    BrowserAuditionWarpReady {
        source: MediaSourceRef,
        generation: u64,
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
        AnalysedBrowserAudio,
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

    // About
    OpenAbout,
    /// Hand a link to the system browser. Every URL reaching this variant is
    /// a compile-time constant, so there is nothing here to validate.
    OpenUrl(&'static str),
    UrlOpened(Result<(), String>),

    // Settings
    OpenSettings,
    CloseSettings,
    SelectSettingsTab(SettingsTab),
    SelectAudioBackend(vibez_audio_io::audio_host::AudioBackend),
    SetBufferSize(u32),
    SetAudioSampleRate(AudioSampleRate),
    SelectAudioInput(AudioDeviceChoice),
    SelectAudioOutput(AudioDeviceChoice),
    RescanAudioDevices,
    ReconnectAudioOutput,

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
        location: TimelineLocation,
        track_id: TrackId,
        old_clip_id: ClipId,
        result: Result<AudioQuantizeSuccess, String>,
    },

    // -- Warping (manual + auto) --
    /// Kick off a background BPM detection for the given clip.
    DetectClipBpm {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
    },
    DetectClipTransients {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        sensitivity: vibez_core::onset::TransientSensitivity,
    },
    ClipTransientsDetected(ClipTransientDetection),
    /// Background BPM detection result. `bpm` is `None` when the
    /// detector refused to commit (silence, sparse pad, too short).
    ClipBpmDetected {
        location: TimelineLocation,
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
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
    },
    /// Background warp result.
    ClipWarpReady {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        /// `false` when an earlier edit (for example Source BPM or Project
        /// tempo) already owns the Undo step for this refresh.
        record_undo: bool,
        result: Result<ClipWarpSuccess, String>,
    },
    /// Background duration-preserving Transpose render completed.
    ClipTransposeReady {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        result: Result<ClipTransposeSuccess, String>,
    },
    CommitAudioClipTransposeAfterDelay {
        location: TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        expected_semitones: i8,
        expected_revision: u64,
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
    RemoteCatalogStartupLoaded(RemoteCatalogStartupResult),
    RefreshRemoteConnection,
    RemoteCatalogPageFetched {
        generation: u64,
        completed_pages: usize,
        result:
            Result<crate::remote_provider::RemotePage, crate::remote_provider::RemoteProviderError>,
    },
    RemoteCatalogRefreshPrepared(RemoteCatalogRefreshResult),
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
        result: Result<(AnalysedBrowserAudio, String, MediaSourceRef), String>,
    },
    DropboxPreview(DropboxEntry),
    DropboxImportToArrangement(DropboxEntry),
    DropboxImportToDevice(DropboxEntry),
}

#[path = "message_constructors.rs"]
mod constructors;
