//! Sample-browser, audition, and remote-catalog UI state.
//! Split from state/mod.rs.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleBrowserMode {
    #[default]
    Local,
    Remote,
}

pub const BROWSER_DOCK_MIN_WIDTH: f32 = 300.0;
pub const BROWSER_DOCK_DEFAULT_WIDTH: f32 = 410.0;
pub const BROWSER_DOCK_MAX_WIDTH: f32 = 650.0;
pub const ARRANGE_MIN_WIDTH_WITH_BROWSER: f32 = 560.0;
pub const BROWSER_PLACES_MIN_WIDTH: f32 = 124.0;
pub const BROWSER_PLACES_MAX_WIDTH: f32 = 176.0;
pub const BROWSER_RESULTS_PAGE_SIZE: usize = 200;
/// Sane DAW range for a manually confirmed audition source BPM.
pub const AUDITION_BPM_MIN: f64 = 20.0;
pub const AUDITION_BPM_MAX: f64 = 999.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserSearchScope {
    #[default]
    SelectedFolder,
    Root,
    Everywhere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRootCatalogState {
    Indexing,
    Updating,
    Ready { warnings: Vec<String> },
    Stale { error: String },
}

impl LocalRootCatalogState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Indexing => "INDEXING",
            Self::Updating => "UPDATING",
            Self::Ready { warnings } if warnings.is_empty() => "READY",
            Self::Ready { .. } => "WARN",
            Self::Stale { .. } => "STALE",
        }
    }

    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Indexing | Self::Updating)
    }
}

/// Browser domain slice: local and remote sample browsing, and
/// drag-and-drop from the browser into the arrangement.
#[derive(Debug, Clone)]
pub struct BrowserState {
    pub open: bool,
    /// Remembered user width. The rendered width may temporarily yield to a
    /// narrow window without overwriting this preference.
    pub dock_width: f32,
    pub dock_resize_active: bool,
    pub search: String,
    pub roots: Vec<PathBuf>,
    pub entries: Vec<SampleBrowserEntry>,
    pub folders: Vec<SampleBrowserFolder>,
    /// Bumped whenever `entries`/`folders` change so memoized result
    /// lists (see [`Self::local_results`]) know to recompute.
    pub catalog_revision: u64,
    pub(crate) local_results_cache: std::cell::RefCell<LocalResults>,
    /// Absolute Local Source Storage folder currently shown in Results. `None`
    /// is the All Roots location.
    pub current_folder: Option<PathBuf>,
    pub expanded_local_folders: HashSet<PathBuf>,
    pub search_scope: BrowserSearchScope,
    pub results_visible_limit: usize,
    pub root_catalog_states: HashMap<PathBuf, LocalRootCatalogState>,
    pub root_refresh_revisions: HashMap<PathBuf, u64>,
    pub root_watch_errors: HashMap<PathBuf, String>,
    pub scan_warnings: Vec<String>,
    pub scan_error: Option<String>,
    pub selected_source: Option<MediaSourceRef>,
    /// Decoded audio used only for the selected Browser source's visual
    /// waveform. Audition still travels through the existing engine path.
    pub waveform_source: Option<MediaSourceRef>,
    pub waveform_audio: Option<Arc<DecodedAudio>>,
    pub waveform_loading: bool,
    pub waveform_error: Option<String>,
    pub audition_enabled: bool,
    pub audition_gain: f32,
    pub audition_loading: bool,
    pub audition_playing: bool,
    pub audition_queued: bool,
    /// Monotonic token minted by [`Self::begin_audition_load`] and
    /// invalidated by [`Self::cancel_audition_requests`]; async decode
    /// and WARP completions carry it so stale results never start
    /// playback after the user stopped or superseded the request.
    pub audition_generation: u64,
    /// UI-retained clone of the audio last handed to the engine's
    /// Audition Bus. The engine voice must never hold the final
    /// reference: dropping it inside the RT callback would free on
    /// the audio thread and violate the allocation-free invariant.
    pub audition_audio: Option<Arc<DecodedAudio>>,
    /// Ring of the most recently superseded audition buffers. The
    /// Audition Bus holds at most four voices (one active + three
    /// outgoing fades), so keeping the last four superseded Arcs
    /// alive UI-side guarantees a re-trigger never leaves the engine
    /// with the final reference either.
    pub audition_audio_retired: [Option<Arc<DecodedAudio>>; 4],
    pub audition_mode: AuditionMode,
    pub audition_sync: AuditionSync,
    pub audition_loop: bool,
    pub audition_bpm_source: Option<MediaSourceRef>,
    pub audition_bpm_suggestion: Option<f64>,
    pub audition_bpm_confidence: Option<f32>,
    pub audition_bpm_confirmed: Option<f64>,
    pub audition_bpm_edit: String,
    pub audition_bpm_detecting: bool,
    pub scan_in_progress: bool,
    pub mode: SampleBrowserMode,
    pub remote: RemoteUiState,
    pub pending_drag: Option<PendingMediaDrag>,
    pub drag_source: Option<MediaSourceRef>,
    pub drag_label: Option<String>,
    pub drag_target: Option<BrowserDropTarget>,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            open: true,
            dock_width: BROWSER_DOCK_DEFAULT_WIDTH,
            dock_resize_active: false,
            search: String::new(),
            roots: Vec::new(),
            entries: Vec::new(),
            folders: Vec::new(),
            catalog_revision: 0,
            local_results_cache: std::cell::RefCell::new(LocalResults::default()),
            current_folder: None,
            expanded_local_folders: HashSet::new(),
            search_scope: BrowserSearchScope::default(),
            results_visible_limit: BROWSER_RESULTS_PAGE_SIZE,
            root_catalog_states: HashMap::new(),
            root_refresh_revisions: HashMap::new(),
            root_watch_errors: HashMap::new(),
            scan_warnings: Vec::new(),
            scan_error: None,
            selected_source: None,
            waveform_source: None,
            waveform_audio: None,
            waveform_loading: false,
            waveform_error: None,
            audition_enabled: true,
            audition_gain: 1.0,
            audition_loading: false,
            audition_playing: false,
            audition_queued: false,
            audition_generation: 0,
            audition_audio: None,
            audition_audio_retired: [None, None, None, None],
            audition_mode: AuditionMode::default(),
            audition_sync: AuditionSync::Off,
            audition_loop: false,
            audition_bpm_source: None,
            audition_bpm_suggestion: None,
            audition_bpm_confidence: None,
            audition_bpm_confirmed: None,
            audition_bpm_edit: String::new(),
            audition_bpm_detecting: false,
            scan_in_progress: false,
            mode: SampleBrowserMode::default(),
            remote: RemoteUiState::default(),
            pending_drag: None,
            drag_source: None,
            drag_label: None,
            drag_target: None,
        }
    }
}

impl BrowserState {
    pub fn begin_root_scan(&mut self, root: &Path, from_watcher: bool) -> u64 {
        let revision = *self
            .root_refresh_revisions
            .entry(root.to_path_buf())
            .and_modify(|revision| *revision = revision.saturating_add(1))
            .or_insert(1);
        self.root_catalog_states.insert(
            root.to_path_buf(),
            if from_watcher {
                LocalRootCatalogState::Updating
            } else {
                LocalRootCatalogState::Indexing
            },
        );
        self.refresh_scan_diagnostics();
        revision
    }

    pub fn root_refresh_is_current(&self, root: &Path, revision: u64) -> bool {
        self.roots.iter().any(|configured| configured == root)
            && self.root_refresh_revisions.get(root).copied() == Some(revision)
    }

    pub fn root_catalog_label(&self, root: &Path) -> &'static str {
        if self.root_watch_errors.contains_key(root) {
            "WATCH ERR"
        } else {
            self.root_catalog_states
                .get(root)
                .map(LocalRootCatalogState::label)
                .unwrap_or("PENDING")
        }
    }

    pub fn root_catalog_message(&self, root: &Path) -> Option<String> {
        if let Some(error) = self.root_watch_errors.get(root) {
            return Some(format!("WATCH ERROR · {error}"));
        }
        match self.root_catalog_states.get(root) {
            Some(LocalRootCatalogState::Indexing) => Some("INDEXING LOCAL ROOT…".into()),
            Some(LocalRootCatalogState::Updating) => Some("UPDATING LOCAL ROOT…".into()),
            Some(LocalRootCatalogState::Ready { warnings }) if !warnings.is_empty() => {
                Some(format!("WARN {} · {}", warnings.len(), warnings[0]))
            }
            Some(LocalRootCatalogState::Stale { error }) => {
                Some(format!("STALE · {error} · RESCAN TO REPAIR"))
            }
            _ => None,
        }
    }

    pub fn refresh_scan_diagnostics(&mut self) {
        // Roll up per-root states in configured-root order so the
        // global label and surfaced error are deterministic (the
        // backing map iterates in arbitrary order).
        let states: Vec<&LocalRootCatalogState> = self
            .roots
            .iter()
            .filter_map(|root| self.root_catalog_states.get(root))
            .collect();
        self.scan_in_progress = states.iter().any(|state| state.is_busy());
        self.scan_warnings = states
            .iter()
            .filter_map(|state| match state {
                LocalRootCatalogState::Ready { warnings } => Some(warnings.as_slice()),
                _ => None,
            })
            .flatten()
            .cloned()
            .collect();
        self.scan_error = states.iter().find_map(|state| match state {
            LocalRootCatalogState::Stale { error } => Some(error.clone()),
            _ => None,
        });
    }

    pub fn reset_results_window(&mut self) {
        self.results_visible_limit = BROWSER_RESULTS_PAGE_SIZE;
    }

    /// Record that `entries`/`folders` changed, invalidating any
    /// memoized result lists.
    pub fn bump_catalog_revision(&mut self) {
        self.catalog_revision = self.catalog_revision.wrapping_add(1);
    }

    pub fn select_local_folder(&mut self, folder: Option<PathBuf>) {
        self.current_folder = folder;
        if let Some(folder) = &self.current_folder {
            self.expanded_local_folders.insert(folder.clone());
        }
        self.search_scope = BrowserSearchScope::SelectedFolder;
        self.reset_results_window();
    }

    pub fn current_local_root(&self) -> Option<&PathBuf> {
        let current = self.current_folder.as_ref()?;
        self.roots
            .iter()
            .filter(|root| current.starts_with(root))
            .max_by_key(|root| root.components().count())
    }

    pub fn search_scope_path(&self) -> Option<&std::path::Path> {
        match self.search_scope {
            BrowserSearchScope::SelectedFolder => self.current_folder.as_deref(),
            BrowserSearchScope::Root => self.current_local_root().map(PathBuf::as_path),
            BrowserSearchScope::Everywhere => None,
        }
    }

    pub fn search_scope_label(&self) -> &'static str {
        match self.search_scope {
            BrowserSearchScope::SelectedFolder if self.current_folder.is_none() => "EVERYWHERE",
            BrowserSearchScope::SelectedFolder
                if self.current_folder.as_ref() == self.current_local_root() =>
            {
                "THIS ROOT"
            }
            BrowserSearchScope::SelectedFolder => "THIS FOLDER",
            BrowserSearchScope::Root => "THIS ROOT",
            BrowserSearchScope::Everywhere => "EVERYWHERE",
        }
    }

    pub fn cycle_search_scope(&mut self) {
        if self.mode == SampleBrowserMode::Remote {
            self.search_scope = match self.search_scope {
                BrowserSearchScope::SelectedFolder if self.remote.current_path.is_empty() => {
                    BrowserSearchScope::Everywhere
                }
                BrowserSearchScope::SelectedFolder => BrowserSearchScope::Root,
                BrowserSearchScope::Root => BrowserSearchScope::Everywhere,
                BrowserSearchScope::Everywhere if self.remote.current_path.is_empty() => {
                    BrowserSearchScope::SelectedFolder
                }
                BrowserSearchScope::Everywhere => BrowserSearchScope::SelectedFolder,
            };
            self.reset_results_window();
            return;
        }
        self.search_scope = match self.search_scope {
            BrowserSearchScope::SelectedFolder
                if self.current_folder.is_some()
                    && self.current_folder.as_ref() != self.current_local_root() =>
            {
                BrowserSearchScope::Root
            }
            BrowserSearchScope::SelectedFolder | BrowserSearchScope::Root => {
                BrowserSearchScope::Everywhere
            }
            BrowserSearchScope::Everywhere if self.current_folder.is_some() => {
                BrowserSearchScope::SelectedFolder
            }
            BrowserSearchScope::Everywhere => BrowserSearchScope::Everywhere,
        };
        self.reset_results_window();
    }

    pub fn path_is_in_search_scope(&self, path: &std::path::Path) -> bool {
        self.search_scope_path()
            .is_none_or(|scope| path.starts_with(scope))
    }

    pub fn local_folder_is_result(
        &self,
        folder: &SampleBrowserFolder,
        normalized_query: &str,
    ) -> bool {
        if normalized_query.is_empty() {
            return self
                .current_folder
                .as_deref()
                .is_some_and(|current| folder.path.parent() == Some(current));
        }
        self.path_is_in_search_scope(&folder.path) && folder.search_text.contains(normalized_query)
    }

    pub fn local_entry_is_result(
        &self,
        entry: &SampleBrowserEntry,
        normalized_query: &str,
    ) -> bool {
        let path = entry.root_path.join(&entry.relative_path);
        if normalized_query.is_empty() {
            return self
                .current_folder
                .as_deref()
                .is_some_and(|current| path.parent() == Some(current));
        }
        self.path_is_in_search_scope(&path) && entry.search_text.contains(normalized_query)
    }

    pub fn visible_result_count(&self, total: usize) -> usize {
        total.min(self.results_visible_limit)
    }

    pub fn has_more_results(&self, total: usize) -> bool {
        self.results_visible_limit < total
    }

    pub fn select_source(&mut self, source: MediaSourceRef) -> bool {
        let changed = self.selected_source.as_ref() != Some(&source);
        self.selected_source = Some(source);
        if changed {
            self.clear_waveform();
            self.clear_audition_bpm();
        }
        changed
    }

    pub fn clear_selection(&mut self) {
        self.selected_source = None;
        self.clear_waveform();
        self.clear_audition_bpm();
    }

    pub fn begin_waveform_load(&mut self, source: &MediaSourceRef) {
        if self.selected_source.as_ref() == Some(source) {
            self.waveform_loading = true;
        }
    }

    /// Mint a fresh audition request token; any older in-flight decode
    /// or WARP preparation becomes stale.
    pub fn begin_audition_load(&mut self, source: &MediaSourceRef) -> u64 {
        self.begin_waveform_load(source);
        if self.selected_source.as_ref() == Some(source) {
            self.audition_loading = true;
        }
        self.audition_generation = self.audition_generation.wrapping_add(1);
        self.audition_generation
    }

    pub fn audition_request_is_current(&self, generation: u64) -> bool {
        self.audition_generation == generation
    }

    /// Explicit user cancellation: clears audition state and
    /// invalidates every in-flight audition request token.
    pub fn cancel_audition_requests(&mut self) {
        self.audition_generation = self.audition_generation.wrapping_add(1);
        self.stop_audition_state();
    }

    pub fn install_audition(
        &mut self,
        generation: u64,
        source: MediaSourceRef,
        audio: Arc<DecodedAudio>,
    ) -> bool {
        if !self.audition_request_is_current(generation) {
            return false;
        }
        if !self.install_waveform(source, audio) {
            return false;
        }
        self.audition_loading = false;
        self.audition_playing = true;
        true
    }

    pub fn stop_audition_state(&mut self) {
        self.audition_loading = false;
        self.audition_playing = false;
        self.audition_queued = false;
    }

    pub fn toggle_audition_enabled(&mut self) -> bool {
        self.audition_enabled = !self.audition_enabled;
        self.audition_enabled
    }

    pub fn set_audition_gain(&mut self, gain: f32) {
        self.audition_gain = gain.clamp(0.0, 2.0);
    }

    pub fn mark_audition_requested(&mut self, queued: bool) {
        self.audition_loading = false;
        self.audition_queued = queued;
        self.audition_playing = !queued;
    }

    pub fn begin_bpm_detection(&mut self, source: &MediaSourceRef) -> bool {
        if self.selected_source.as_ref() != Some(source)
            || self.audition_bpm_source.as_ref() == Some(source)
            || self.audition_bpm_detecting
        {
            return false;
        }
        self.audition_bpm_detecting = true;
        true
    }

    pub fn install_bpm_suggestion(
        &mut self,
        source: MediaSourceRef,
        estimate: Option<(f64, f32)>,
        auto_confirm_threshold: f32,
    ) -> bool {
        if self.selected_source.as_ref() != Some(&source) {
            return false;
        }
        self.audition_bpm_source = Some(source);
        self.audition_bpm_detecting = false;
        self.audition_bpm_suggestion = estimate.map(|value| value.0);
        self.audition_bpm_confidence = estimate.map(|value| value.1);
        // A BPM the user confirmed while detection was in flight wins
        // over the late estimate; only auto-confirm into an empty slot.
        if self.audition_bpm_confirmed.is_none() {
            self.audition_bpm_confirmed = estimate
                .filter(|(bpm, confidence)| {
                    bpm.is_finite()
                        && *bpm > 0.0
                        && *confidence >= auto_confirm_threshold.clamp(0.0, 1.0)
                })
                .map(|(bpm, _)| bpm);
        }
        if self.audition_bpm_edit.is_empty() {
            self.audition_bpm_edit = estimate
                .map(|value| format!("{:.1}", value.0))
                .unwrap_or_default();
        }
        true
    }

    pub fn confirm_audition_bpm(&mut self) -> Result<f64, &'static str> {
        let bpm = self
            .audition_bpm_edit
            .trim()
            .parse::<f64>()
            .map_err(|_| "Enter a source BPM between 20 and 999")?;
        // An unbounded BPM would request an effectively unbounded WARP
        // allocation; keep manual entry inside a sane DAW range.
        if !bpm.is_finite() || !(AUDITION_BPM_MIN..=AUDITION_BPM_MAX).contains(&bpm) {
            return Err("Enter a source BPM between 20 and 999");
        }
        self.audition_bpm_confirmed = Some(bpm);
        Ok(bpm)
    }

    pub fn clear_audition_bpm(&mut self) {
        self.audition_bpm_source = None;
        self.audition_bpm_suggestion = None;
        self.audition_bpm_confidence = None;
        self.audition_bpm_confirmed = None;
        self.audition_bpm_edit.clear();
        self.audition_bpm_detecting = false;
    }

    pub fn audition_import_input(&self) -> Option<AuditionImportInput> {
        match self.audition_mode {
            AuditionMode::Raw => Some(AuditionImportInput {
                mode: AuditionMode::Raw,
                source_bpm: None,
            }),
            AuditionMode::Warp => {
                self.audition_bpm_confirmed
                    .map(|source_bpm| AuditionImportInput {
                        mode: AuditionMode::Warp,
                        source_bpm: Some(source_bpm),
                    })
            }
        }
    }

    pub fn begin_pending_drag(
        &mut self,
        source: MediaSourceRef,
        label: String,
        origin_x: f32,
        origin_y: f32,
    ) {
        self.cancel_media_drag();
        self.pending_drag = Some(PendingMediaDrag {
            source,
            label,
            origin_x,
            origin_y,
        });
    }

    pub fn move_pending_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(pending) = self.pending_drag.as_ref() else {
            return false;
        };
        let dx = x - pending.origin_x;
        let dy = y - pending.origin_y;
        if dx * dx + dy * dy <= MEDIA_DRAG_THRESHOLD_PX * MEDIA_DRAG_THRESHOLD_PX {
            return false;
        }
        let pending = self.pending_drag.take().expect("pending drag exists");
        self.drag_source = Some(pending.source);
        self.drag_label = Some(pending.label);
        self.drag_target = None;
        true
    }

    pub fn cancel_pending_drag(&mut self) {
        self.pending_drag = None;
    }

    pub fn cancel_media_drag(&mut self) {
        self.pending_drag = None;
        self.drag_source = None;
        self.drag_label = None;
        self.drag_target = None;
    }

    pub fn drag_preview_beats(&self, project_bpm: f64) -> Option<f64> {
        let source = self.drag_source.as_ref()?;
        if self.waveform_source.as_ref()? != source {
            return None;
        }
        let audio = self.waveform_audio.as_ref()?;
        if audio.sample_rate == 0 || audio.num_frames() == 0 {
            return None;
        }
        let seconds = audio.num_frames() as f64 / audio.sample_rate as f64;
        match self.audition_import_input()? {
            AuditionImportInput {
                mode: AuditionMode::Raw,
                ..
            } => (project_bpm > 0.0).then_some(seconds * project_bpm / 60.0),
            AuditionImportInput {
                mode: AuditionMode::Warp,
                source_bpm: Some(source_bpm),
            } if project_bpm > 0.0 => {
                let target_frames = crate::warp::warp_target_frames(
                    audio.num_frames(),
                    audio.sample_rate as f64,
                    source_bpm,
                    project_bpm,
                );
                Some(target_frames as f64 * project_bpm / (audio.sample_rate as f64 * 60.0))
            }
            _ => None,
        }
    }

    pub fn install_waveform(&mut self, source: MediaSourceRef, audio: Arc<DecodedAudio>) -> bool {
        if self.selected_source.as_ref() != Some(&source) {
            return false;
        }
        let channels = audio.num_channels();
        let sample_rate = audio.sample_rate;
        let duration_seconds = if sample_rate > 0 {
            Some(audio.num_frames() as f64 / sample_rate as f64)
        } else {
            None
        };
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.source == source) {
            entry.duration_seconds = duration_seconds;
            entry.channels = Some(channels);
            entry.sample_rate = Some(sample_rate);
        }
        self.waveform_source = Some(source);
        self.waveform_audio = Some(audio);
        self.waveform_loading = false;
        self.waveform_error = None;
        true
    }

    pub fn fail_waveform_load(&mut self, source: &MediaSourceRef, error: String) {
        if self.selected_source.as_ref() == Some(source) {
            self.waveform_loading = false;
            self.waveform_error = Some(error);
        }
    }

    fn clear_waveform(&mut self) {
        self.waveform_source = None;
        self.waveform_audio = None;
        self.waveform_loading = false;
        self.waveform_error = None;
    }

    pub fn set_dock_width(&mut self, width: f32) {
        self.dock_width = width.clamp(BROWSER_DOCK_MIN_WIDTH, BROWSER_DOCK_MAX_WIDTH);
    }

    /// Width a live splitter drag should store: unlike a preference
    /// restored from settings, a drag also respects the window cap
    /// applied by [`Self::effective_dock_width`], so the handle keeps
    /// tracking the cursor instead of freezing at the yield point.
    pub fn dock_drag_width(&self, cursor_x: f32, window_width: f32) -> f32 {
        cursor_x.min(window_width - ARRANGE_MIN_WIDTH_WITH_BROWSER)
    }

    pub fn effective_dock_width(&self, window_width: f32) -> f32 {
        let available = (window_width - ARRANGE_MIN_WIDTH_WITH_BROWSER)
            .clamp(BROWSER_DOCK_MIN_WIDTH, BROWSER_DOCK_MAX_WIDTH);
        self.dock_width.min(available).max(BROWSER_DOCK_MIN_WIDTH)
    }

    pub fn places_pane_width(&self, window_width: f32) -> f32 {
        (self.effective_dock_width(window_width) * 0.36)
            .clamp(BROWSER_PLACES_MIN_WIDTH, BROWSER_PLACES_MAX_WIDTH)
    }

    /// The single Results table keeps Name and Status visible throughout the
    /// resize range, then promotes BPM and Length into dedicated columns once
    /// the Results pane has enough room to keep every column readable.
    pub fn results_use_wide_columns(&self, window_width: f32) -> bool {
        let results_width =
            self.effective_dock_width(window_width) - self.places_pane_width(window_width);
        results_width >= 400.0
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum RemoteCatalogState {
    #[default]
    Ready,
    Refreshing,
    Stale {
        error: String,
    },
    Partial {
        pages: usize,
        error: String,
    },
    AuthenticationRequired {
        error: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteAvailability {
    RemoteOnly,
    Cached,
    Fetching,
    ReconnectRequired,
    Unavailable { error: String },
}

impl RemoteAvailability {
    pub fn label(&self) -> &'static str {
        match self {
            Self::RemoteOnly => "REMOTE",
            Self::Cached => "CACHED",
            Self::Fetching => "FETCH",
            Self::ReconnectRequired => "RETRY",
            Self::Unavailable { .. } => "ERROR",
        }
    }
}

impl RemoteCatalogState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Refreshing => "REFRESHING",
            Self::Stale { .. } => "STALE",
            Self::Partial { .. } => "PARTIAL",
            Self::AuthenticationRequired { .. } => "SIGN IN",
        }
    }
}

#[cfg(test)]
mod remote_catalog_state_tests {
    use super::RemoteCatalogState;

    #[test]
    fn refresh_auth_stale_and_partial_states_have_distinct_labels() {
        assert_eq!(RemoteCatalogState::Refreshing.label(), "REFRESHING");
        assert_eq!(
            RemoteCatalogState::AuthenticationRequired {
                error: "sign in".into()
            }
            .label(),
            "SIGN IN"
        );
        assert_eq!(
            RemoteCatalogState::Stale {
                error: "offline".into()
            }
            .label(),
            "STALE"
        );
        assert_eq!(
            RemoteCatalogState::Partial {
                pages: 1,
                error: "offline".into()
            }
            .label(),
            "PARTIAL"
        );
    }
}

/// Provider-neutral Browser state for Remote Connections. Dropbox V1 auth
/// fields remain here because Dropbox is the sole shipped adapter.
#[derive(Debug, Clone)]
pub struct RemoteUiState {
    pub connected: bool,
    pub account_email: Option<String>,
    /// App key entered in settings (may be empty until the user pastes one).
    pub app_key_input: String,
    /// Whether any source of app key is present (settings, env, build-time).
    pub has_app_key: bool,
    /// An OAuth flow is in progress; Connect button is disabled.
    pub auth_in_progress: bool,
    pub last_error: Option<String>,
    pub catalog: RemoteCatalogSnapshot,
    pub catalog_state: RemoteCatalogState,
    /// Pages and entries applied during the current progressive Catalog Refresh.
    pub refresh_pages: usize,
    pub refresh_items: usize,
    /// Current provider folder (`""` means the connection root).
    pub current_path: String,
    /// Whether the Remote place reveals its configured connections.
    pub place_expanded: bool,
    /// Whether the Dropbox connection reveals its folder tree.
    pub connection_expanded: bool,
    /// Paths expanded beneath the connection in Places.
    pub expanded: HashSet<String>,
    /// Derived catalog lookup used by Places and ordinary folder navigation.
    /// Values are indexes into `catalog.entries`, sorted folder-first by name.
    pub catalog_children: HashMap<String, Vec<usize>>,
    /// Provider item identity of the current Remote selection.
    pub selected_path: Option<String>,
    /// A preview fetch / playback is in flight.
    pub preview_in_progress: bool,
    pub availability: HashMap<String, RemoteAvailability>,
    pub cache_usage_bytes: u64,
    pub cache_entries: usize,
    pub cache_budget_bytes: u64,
    pub cache_automatic_eviction: bool,
    pub cache_error: Option<String>,
}

impl Default for RemoteUiState {
    fn default() -> Self {
        Self {
            connected: false,
            account_email: None,
            app_key_input: String::new(),
            has_app_key: false,
            auth_in_progress: false,
            last_error: None,
            catalog: RemoteCatalogSnapshot::default(),
            catalog_state: RemoteCatalogState::default(),
            refresh_pages: 0,
            refresh_items: 0,
            current_path: String::new(),
            place_expanded: true,
            connection_expanded: true,
            expanded: HashSet::new(),
            catalog_children: HashMap::new(),
            selected_path: None,
            preview_in_progress: false,
            availability: HashMap::new(),
            cache_usage_bytes: 0,
            cache_entries: 0,
            cache_budget_bytes: vibez_dropbox::DEFAULT_MEDIA_CACHE_BUDGET_BYTES,
            cache_automatic_eviction: true,
            cache_error: None,
        }
    }
}

impl RemoteUiState {
    /// Rebuild the parent/children lookup after the catalog is loaded or
    /// reconciled. UI redraws can then navigate one folder without rescanning
    /// the complete provider catalog.
    pub fn rebuild_catalog_children(&mut self) {
        let mut children: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, entry) in self.catalog.entries.iter().enumerate() {
            children
                .entry(entry.parent_path.clone())
                .or_default()
                .push(index);
        }
        for indexes in children.values_mut() {
            indexes.sort_by(|left, right| {
                let left = &self.catalog.entries[*left];
                let right = &self.catalog.entries[*right];
                (
                    !left.is_folder,
                    left.name.to_ascii_lowercase(),
                    &left.provider_item_id,
                )
                    .cmp(&(
                        !right.is_folder,
                        right.name.to_ascii_lowercase(),
                        &right.provider_item_id,
                    ))
            });
        }
        self.catalog_children = children;
    }

    pub fn catalog_child_indices(&self, parent: &str) -> &[usize] {
        self.catalog_children
            .get(parent)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}
