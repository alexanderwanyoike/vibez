//! Prepared media and project results carried by application messages.

use crate::state::{AuditionImportInput, SampleBrowserEntry, SampleBrowserFolder};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use vibez_core::{
    audio_buffer::DecodedAudio,
    id::{ClipId, SectionId, TrackId},
    track::{ClipInfo, ClipTranspose, DrumPadState, MediaSourceRef},
};
use vibez_dropbox::{AccountInfo, Tokens as DropboxTokens};
use vibez_project::{project_format_v1::SaveObservation, Project, TimelineLocation};

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
pub struct ClipTransientDetection {
    pub location: TimelineLocation,
    pub track_id: TrackId,
    pub clip_id: ClipId,
    pub expected_audio: Arc<DecodedAudio>,
    pub source_frames: Vec<u64>,
    pub record_undo: bool,
}

#[derive(Debug, Clone)]
pub struct ClipTransposeSuccess {
    pub audio: Arc<DecodedAudio>,
    pub source_audio: Arc<DecodedAudio>,
    pub transpose: ClipTranspose,
    pub expected_warped: bool,
    pub expected_audio: Arc<DecodedAudio>,
    pub expected_geometry: Option<crate::domains::arrangement::ClipRenderedGeometry>,
    pub geometry: Option<crate::domains::arrangement::ClipRenderedGeometry>,
    pub warning: Option<String>,
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
pub struct PreparedDrumRackAudio {
    pub source: MediaSourceRef,
    pub audio: Arc<DecodedAudio>,
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
}

#[derive(Debug, Clone)]
pub struct AudioRecordingOutcome {
    pub track_id: TrackId,
    pub start_position_samples: u64,
    pub clip_name: String,
    pub audio: Arc<DecodedAudio>,
    pub source: MediaSourceRef,
    pub completion_label: String,
    pub quality_warning: Option<String>,
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

#[derive(Debug, Clone)]
pub struct AnalysedBrowserAudio {
    pub audio: Arc<DecodedAudio>,
    pub loop_fit: Option<crate::warp::LoopGridFit>,
}

#[derive(Debug)]
pub struct RemoteCatalogStartupData {
    pub catalog: crate::remote_provider::RemoteCatalogSnapshot,
    pub catalog_children: HashMap<String, Vec<usize>>,
    pub availability: HashMap<String, crate::state::RemoteAvailability>,
    pub cache_usage: vibez_dropbox::CacheUsage,
    pub load_error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum RemoteCatalogRefreshContinuation {
    FetchNext { checkpoint: String },
    Complete,
    Failed(crate::remote_provider::RemoteProviderError),
}

#[derive(Debug)]
pub struct RemoteCatalogRefreshData {
    pub generation: u64,
    pub pages: usize,
    pub catalog: Arc<crate::remote_provider::RemoteCatalogSnapshot>,
    pub catalog_children: Option<HashMap<String, Vec<usize>>>,
    pub availability: Option<HashMap<String, crate::state::RemoteAvailability>>,
    pub base_catalog: Arc<crate::remote_provider::RemoteCatalogSnapshot>,
    pub base_availability: HashMap<String, crate::state::RemoteAvailability>,
    pub base_runtime_revision: u64,
    pub continuation: RemoteCatalogRefreshContinuation,
}

#[derive(Clone)]
pub struct RemoteCatalogRefreshResult {
    generation: u64,
    result: Arc<std::sync::Mutex<Option<Result<RemoteCatalogRefreshData, String>>>>,
}

impl RemoteCatalogRefreshResult {
    pub fn new(generation: u64, result: Result<RemoteCatalogRefreshData, String>) -> Self {
        Self {
            generation,
            result: Arc::new(std::sync::Mutex::new(Some(result))),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn take(&self) -> Option<Result<RemoteCatalogRefreshData, String>> {
        self.result.lock().ok()?.take()
    }
}

impl std::fmt::Debug for RemoteCatalogRefreshResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RemoteCatalogRefreshResult")
    }
}

#[derive(Clone)]
pub struct RemoteCatalogStartupResult(
    Arc<std::sync::Mutex<Option<Result<RemoteCatalogStartupData, String>>>>,
);

impl RemoteCatalogStartupResult {
    pub fn new(result: Result<RemoteCatalogStartupData, String>) -> Self {
        Self(Arc::new(std::sync::Mutex::new(Some(result))))
    }

    pub fn take(&self) -> Option<Result<RemoteCatalogStartupData, String>> {
        self.0.lock().ok()?.take()
    }
}

impl std::fmt::Debug for RemoteCatalogStartupResult {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RemoteCatalogStartupResult")
    }
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
    pub new_start_marker: u64,
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
    LauncherClipAt {
        track_id: TrackId,
        row: u32,
    },
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
pub enum ProjectLoadError {
    MissingProject(String),
    Other(String),
}

impl ProjectLoadError {
    pub fn missing_project(error: impl Into<String>) -> Self {
        Self::MissingProject(error.into())
    }

    pub fn other(error: impl Into<String>) -> Self {
        Self::Other(error.into())
    }

    pub fn is_missing_project(&self) -> bool {
        matches!(self, Self::MissingProject(_))
    }
}

impl std::fmt::Display for ProjectLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingProject(error) | Self::Other(error) => formatter.write_str(error),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectSaveResult {
    pub path: PathBuf,
    pub project: Project,
    pub observation: Option<SaveObservation>,
}

/// Identity of the document revision captured by an asynchronous save.
/// The completion uses this to avoid marking newer edits as saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectSaveToken {
    pub document_id: u64,
    pub revision: u64,
    pub automatic: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectSaveCompleted {
    pub token: ProjectSaveToken,
    pub result: Result<ProjectSaveResult, String>,
}
