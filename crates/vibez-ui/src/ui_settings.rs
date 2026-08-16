use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vibez_audio_io::audio_host::AudioBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiSettings {
    /// Most recently opened or successfully saved projects, newest first.
    #[serde(default)]
    pub recent_project_paths: Vec<PathBuf>,
    #[serde(default)]
    pub perform_input_mapping: crate::domains::perform::PerformInputMapping,
    #[serde(default = "default_fixed_computer_velocity")]
    pub fixed_computer_velocity: u8,
    #[serde(default = "default_track_mute_quantization")]
    pub track_mute_quantization: vibez_core::perform::TrackMuteQuantization,
    #[serde(default)]
    pub sample_library_roots: Vec<PathBuf>,
    #[serde(default = "default_sample_browser_open")]
    pub sample_browser_open: bool,
    #[serde(default = "default_sample_browser_width")]
    pub sample_browser_width: f32,
    #[serde(default = "default_perform_surface_width")]
    pub perform_surface_width: f32,
    #[serde(default = "default_detail_panel_height")]
    pub detail_panel_height: f32,
    #[serde(default = "default_audition_enabled")]
    pub audition_enabled: bool,
    #[serde(default = "default_audition_gain")]
    pub audition_gain: f32,
    /// Automatically detect each dropped sample's BPM and warp it to
    /// the project tempo on import. Off by default; users opt in from
    /// Settings → Warping.
    #[serde(default)]
    pub auto_warp_on_import: bool,
    /// Minimum BPM-detector confidence below which import-time auto-
    /// warp refuses to stretch. 0.0 warps everything (even bad
    /// guesses); 1.0 means only stretch when the detector is very
    /// sure. Default is a moderate gate.
    #[serde(default = "default_warp_confidence_threshold")]
    pub warp_confidence_threshold: f32,
    /// Name of the external MIDI input port to auto-connect on
    /// startup. `None` means auto-pick the first visible port.
    #[serde(default)]
    pub preferred_midi_input: Option<String>,
    /// `None` follows the platform's System Default Audio Input.
    #[serde(default)]
    pub preferred_audio_input: Option<String>,
    /// Native audio API used to enumerate and open both input and output.
    #[serde(default)]
    pub audio_backend: AudioBackend,
    /// `None` follows the platform's System Default Audio Output.
    #[serde(default)]
    pub preferred_audio_output: Option<String>,
    /// `None` uses the output device's default until the producer chooses one.
    #[serde(default)]
    pub audio_sample_rate: Option<u32>,
    #[serde(default = "default_audio_buffer_size")]
    pub audio_buffer_size: u32,
    /// Selected theme name (built-in or user `.vzt`); `None` means
    /// the default Charcoal.
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default = "default_media_cache_budget_bytes")]
    pub media_cache_budget_bytes: u64,
    #[serde(default = "default_media_cache_automatic_eviction")]
    pub media_cache_automatic_eviction: bool,
    /// Ask before deleting a Project Track and all of its Arrange and
    /// Section content. Off by default because deletion is undoable.
    #[serde(default)]
    pub confirm_project_track_deletion: bool,
    /// Save named projects shortly after editing stops. Untitled projects
    /// still wait for an explicit save so autosave never opens a file picker.
    #[serde(default = "default_auto_save_enabled")]
    pub auto_save_enabled: bool,
    /// Multiplier applied to the whole logical coordinate space, so
    /// every panel, control and font grows or shrinks together. This is
    /// not timeline zoom: it changes how big the interface is drawn,
    /// never how much musical time a lane shows. Always read through
    /// [`UiSettings::clamped_interface_scale`], since the value comes
    /// off disk and an out-of-range one is unrecoverable from inside
    /// the app.
    #[serde(default = "default_interface_scale")]
    pub interface_scale: f32,
    /// Ask GitHub at startup whether a newer release exists. On by
    /// default: the check is one request a day and the result is a
    /// dismissible chip, never a dialog. Off means no network call.
    #[serde(default = "default_check_for_updates")]
    pub check_for_updates: bool,
    /// Unix seconds of the last release check, so the once-a-day
    /// throttle survives a restart. `None` until the first attempt.
    #[serde(default)]
    pub last_update_check_unix: Option<u64>,
}

/// Smallest supported interface scale. Below this, hit targets in the
/// mixer and timeline shrink past the point where they can be aimed at.
pub const INTERFACE_SCALE_MIN: f32 = 0.75;

/// Largest supported interface scale. Above this, the arrangement and
/// mixer stop fitting inside the 900x600 minimum window, which would
/// hide controls with no way to reach them.
pub const INTERFACE_SCALE_MAX: f32 = 1.5;

/// Unscaled interface: what every existing installation gets, and the
/// fallback for a value that cannot be interpreted.
pub const INTERFACE_SCALE_DEFAULT: f32 = 1.0;

/// Force a scale from disk or from the slider into the supported range.
///
/// NaN is handled separately because `f32::clamp` returns NaN unchanged,
/// and handing iced a NaN scale factor collapses the layout into a
/// window the user cannot click their way out of. Since the settings
/// file is plain JSON that people do hand-edit, an uninterpretable
/// value falls back to unscaled rather than propagating.
pub fn clamp_interface_scale(scale: f32) -> f32 {
    if scale.is_nan() {
        return INTERFACE_SCALE_DEFAULT;
    }
    scale.clamp(INTERFACE_SCALE_MIN, INTERFACE_SCALE_MAX)
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            recent_project_paths: Vec::new(),
            perform_input_mapping: crate::domains::perform::PerformInputMapping::default(),
            fixed_computer_velocity: default_fixed_computer_velocity(),
            track_mute_quantization: default_track_mute_quantization(),
            sample_library_roots: Vec::new(),
            sample_browser_open: default_sample_browser_open(),
            sample_browser_width: default_sample_browser_width(),
            perform_surface_width: default_perform_surface_width(),
            detail_panel_height: default_detail_panel_height(),
            audition_enabled: default_audition_enabled(),
            audition_gain: default_audition_gain(),
            auto_warp_on_import: false,
            warp_confidence_threshold: default_warp_confidence_threshold(),
            preferred_midi_input: None,
            preferred_audio_input: None,
            audio_backend: AudioBackend::default(),
            preferred_audio_output: None,
            audio_sample_rate: None,
            audio_buffer_size: default_audio_buffer_size(),
            theme: None,
            media_cache_budget_bytes: default_media_cache_budget_bytes(),
            media_cache_automatic_eviction: default_media_cache_automatic_eviction(),
            confirm_project_track_deletion: false,
            auto_save_enabled: default_auto_save_enabled(),
            interface_scale: default_interface_scale(),
            check_for_updates: default_check_for_updates(),
            last_update_check_unix: None,
        }
    }
}

pub const RECENT_PROJECT_LIMIT: usize = 6;

pub fn normalize_recent_projects(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut normalized = Vec::new();
    for path in paths {
        if !normalized.contains(&path) {
            normalized.push(path);
        }
        if normalized.len() == RECENT_PROJECT_LIMIT {
            break;
        }
    }
    normalized
}

pub fn remember_recent_project(paths: &mut Vec<PathBuf>, path: PathBuf) {
    paths.retain(|existing| existing != &path);
    paths.insert(0, path);
    paths.truncate(RECENT_PROJECT_LIMIT);
}

pub fn forget_recent_project(paths: &mut Vec<PathBuf>, path: &Path) -> bool {
    let previous_len = paths.len();
    paths.retain(|existing| existing != path);
    paths.len() != previous_len
}

impl UiSettings {
    pub fn settings_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vibez")
            .join("ui.json")
    }

    pub fn load() -> Self {
        let path = Self::settings_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// The persisted interface scale, made safe to render with.
    pub fn clamped_interface_scale(&self) -> f32 {
        clamp_interface_scale(self.interface_scale)
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::settings_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }
}

fn default_sample_browser_open() -> bool {
    true
}

/// Track Mutes default to Immediate for back-compat with settings written
/// before quantization existed; the shared enum's own Default is the Section
/// launch default (OneBar).
const fn default_track_mute_quantization() -> vibez_core::perform::TrackMuteQuantization {
    vibez_core::perform::TrackMuteQuantization::Immediate
}

const fn default_fixed_computer_velocity() -> u8 {
    100
}

const fn default_audio_buffer_size() -> u32 {
    512
}

fn default_media_cache_budget_bytes() -> u64 {
    vibez_dropbox::DEFAULT_MEDIA_CACHE_BUDGET_BYTES
}

fn default_media_cache_automatic_eviction() -> bool {
    true
}

const fn default_auto_save_enabled() -> bool {
    true
}

fn default_sample_browser_width() -> f32 {
    crate::state::BROWSER_DOCK_DEFAULT_WIDTH
}

fn default_perform_surface_width() -> f32 {
    crate::state::PERFORM_SURFACE_DEFAULT_WIDTH
}

fn default_detail_panel_height() -> f32 {
    crate::state::DETAIL_PANEL_DEFAULT_HEIGHT
}

fn default_audition_enabled() -> bool {
    true
}

fn default_audition_gain() -> f32 {
    1.0
}

fn default_warp_confidence_threshold() -> f32 {
    0.6
}

fn default_interface_scale() -> f32 {
    INTERFACE_SCALE_DEFAULT
}

const fn default_check_for_updates() -> bool {
    true
}

#[cfg(test)]
impl UiSettings {
    fn input_mapping_key(
        &self,
        position: crate::domains::perform::PadPosition,
    ) -> crate::domains::perform::ComputerKey {
        self.perform_input_mapping.key_for(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_projects_are_deduplicated_capped_and_promoted() {
        let mut paths = normalize_recent_projects([
            "/projects/a.vzp".into(),
            "/projects/b.vzp".into(),
            "/projects/a.vzp".into(),
            "/projects/c.vzp".into(),
            "/projects/d.vzp".into(),
            "/projects/e.vzp".into(),
            "/projects/f.vzp".into(),
            "/projects/g.vzp".into(),
        ]);
        assert_eq!(paths.len(), RECENT_PROJECT_LIMIT);
        assert_eq!(paths[0], PathBuf::from("/projects/a.vzp"));

        remember_recent_project(&mut paths, "/projects/d.vzp".into());
        assert_eq!(paths[0], PathBuf::from("/projects/d.vzp"));
        assert_eq!(paths.len(), RECENT_PROJECT_LIMIT);
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.as_path() == std::path::Path::new("/projects/d.vzp"))
                .count(),
            1
        );
    }

    #[test]
    fn failed_recent_project_is_pruned_without_touching_other_entries() {
        let mut paths = vec![
            PathBuf::from("/projects/a.vzp"),
            PathBuf::from("/projects/missing.vzp"),
            PathBuf::from("/projects/b.vzp"),
        ];

        assert!(forget_recent_project(
            &mut paths,
            Path::new("/projects/missing.vzp")
        ));
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/projects/a.vzp"),
                PathBuf::from("/projects/b.vzp")
            ]
        );
        assert!(!forget_recent_project(
            &mut paths,
            Path::new("/projects/unknown.vzp")
        ));
    }

    #[test]
    fn old_settings_start_with_no_recent_projects() {
        let loaded: UiSettings = serde_json::from_str("{}").unwrap();
        assert!(loaded.recent_project_paths.is_empty());
        assert_eq!(loaded.audio_buffer_size, 512);
        assert_eq!(loaded.audio_sample_rate, None);
        assert_eq!(loaded.preferred_audio_input, None);
        assert_eq!(loaded.preferred_audio_output, None);
        assert_eq!(loaded.audio_backend, AudioBackend::System);
    }

    #[test]
    fn audio_configuration_roundtrips_outside_project_state() {
        let settings = UiSettings {
            preferred_audio_input: Some("USB In".into()),
            audio_backend: AudioBackend::Asio,
            preferred_audio_output: Some("USB Out".into()),
            audio_sample_rate: Some(48_000),
            audio_buffer_size: 128,
            ..Default::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(loaded.preferred_audio_input.as_deref(), Some("USB In"));
        assert_eq!(loaded.audio_backend, AudioBackend::Asio);
        assert_eq!(loaded.preferred_audio_output.as_deref(), Some("USB Out"));
        assert_eq!(loaded.audio_sample_rate, Some(48_000));
        assert_eq!(loaded.audio_buffer_size, 128);
    }

    #[test]
    fn old_settings_receive_the_browser_width_default() {
        let loaded: UiSettings =
            serde_json::from_str(r#"{"sample_library_roots":[],"sample_browser_open":false}"#)
                .unwrap();
        assert!(!loaded.sample_browser_open);
        assert_eq!(
            loaded.sample_browser_width,
            crate::state::BROWSER_DOCK_DEFAULT_WIDTH
        );
    }

    #[test]
    fn browser_width_roundtrips() {
        let settings = UiSettings {
            sample_browser_width: 612.0,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let loaded: UiSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.sample_browser_width, 612.0);
    }

    #[test]
    fn perform_surface_width_defaults_and_roundtrips() {
        let old: UiSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.perform_surface_width,
            crate::state::PERFORM_SURFACE_DEFAULT_WIDTH
        );

        let settings = UiSettings {
            perform_surface_width: 704.0,
            ..UiSettings::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(loaded.perform_surface_width, 704.0);
    }

    #[test]
    fn detail_panel_height_defaults_and_roundtrips() {
        let old: UiSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.detail_panel_height,
            crate::state::DETAIL_PANEL_DEFAULT_HEIGHT
        );

        let settings = UiSettings {
            detail_panel_height: 412.0,
            ..UiSettings::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(loaded.detail_panel_height, 412.0);
    }

    #[test]
    fn old_settings_enable_audition_at_unity_by_default() {
        let loaded: UiSettings = serde_json::from_str(r#"{"sample_library_roots":[]}"#).unwrap();
        assert!(loaded.audition_enabled);
        assert_eq!(loaded.audition_gain, 1.0);
    }

    #[test]
    fn audition_preferences_roundtrip() {
        let settings = UiSettings {
            audition_enabled: false,
            audition_gain: 0.42,
            ..Default::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!loaded.audition_enabled);
        assert_eq!(loaded.audition_gain, 0.42);
    }

    #[test]
    fn project_track_deletion_confirmation_defaults_off_and_roundtrips() {
        let old: UiSettings = serde_json::from_str(r#"{"sample_library_roots":[]}"#).unwrap();
        assert!(!old.confirm_project_track_deletion);

        let settings = UiSettings {
            confirm_project_track_deletion: true,
            ..Default::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(loaded.confirm_project_track_deletion);
    }

    #[test]
    fn auto_save_defaults_on_and_roundtrips() {
        let old: UiSettings = serde_json::from_str("{}").unwrap();
        assert!(old.auto_save_enabled);

        let settings = UiSettings {
            auto_save_enabled: false,
            ..UiSettings::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!loaded.auto_save_enabled);
    }

    #[test]
    fn settings_written_before_interface_scale_render_unscaled() {
        let loaded: UiSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(loaded.interface_scale, INTERFACE_SCALE_DEFAULT);
        assert_eq!(loaded.clamped_interface_scale(), 1.0);
    }

    #[test]
    fn interface_scale_roundtrips() {
        let settings = UiSettings {
            interface_scale: 1.25,
            ..Default::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(loaded.interface_scale, 1.25);
        assert_eq!(loaded.clamped_interface_scale(), 1.25);
    }

    #[test]
    fn a_hand_edited_interface_scale_is_pulled_back_into_the_usable_range() {
        let tiny: UiSettings = serde_json::from_str(r#"{"interface_scale":0.05}"#).unwrap();
        assert_eq!(tiny.clamped_interface_scale(), INTERFACE_SCALE_MIN);

        let huge: UiSettings = serde_json::from_str(r#"{"interface_scale":40.0}"#).unwrap();
        assert_eq!(huge.clamped_interface_scale(), INTERFACE_SCALE_MAX);

        let negative: UiSettings = serde_json::from_str(r#"{"interface_scale":-2.0}"#).unwrap();
        assert_eq!(negative.clamped_interface_scale(), INTERFACE_SCALE_MIN);
    }

    #[test]
    fn an_uninterpretable_interface_scale_falls_back_to_unscaled() {
        assert_eq!(clamp_interface_scale(f32::NAN), INTERFACE_SCALE_DEFAULT);
        assert_eq!(clamp_interface_scale(f32::INFINITY), INTERFACE_SCALE_MAX);
        assert_eq!(
            clamp_interface_scale(f32::NEG_INFINITY),
            INTERFACE_SCALE_MIN
        );
    }

    #[test]
    fn old_settings_receive_the_twenty_gib_media_cache_policy() {
        let loaded: UiSettings = serde_json::from_str(r#"{"sample_library_roots":[]}"#).unwrap();
        assert_eq!(
            loaded.media_cache_budget_bytes,
            vibez_dropbox::DEFAULT_MEDIA_CACHE_BUDGET_BYTES
        );
        assert!(loaded.media_cache_automatic_eviction);
    }

    #[test]
    fn settings_predating_the_release_check_opt_into_it_with_no_history() {
        let old: UiSettings = serde_json::from_str("{}").unwrap();
        assert!(old.check_for_updates);
        assert_eq!(old.last_update_check_unix, None);
    }

    #[test]
    fn release_check_preference_and_timestamp_roundtrip() {
        let settings = UiSettings {
            check_for_updates: false,
            last_update_check_unix: Some(1_772_000_000),
            ..Default::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(!loaded.check_for_updates);
        assert_eq!(loaded.last_update_check_unix, Some(1_772_000_000));
    }

    #[test]
    fn multiple_local_roots_roundtrip_in_ui_configuration() {
        let roots = vec![
            PathBuf::from("/samples/drums"),
            PathBuf::from("/samples/field-recordings"),
        ];
        let settings = UiSettings {
            sample_library_roots: roots.clone(),
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: UiSettings = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.sample_library_roots, roots);
    }

    #[test]
    fn old_settings_receive_the_default_perform_input_mapping() {
        use crate::domains::perform::{ComputerKey, PadPosition};

        let loaded: UiSettings = serde_json::from_str(r#"{"sample_library_roots":[]}"#).unwrap();

        assert_eq!(
            loaded.input_mapping_key(PadPosition::ALL[0]),
            ComputerKey::Digit1
        );
        assert_eq!(
            loaded.input_mapping_key(PadPosition::ALL[15]),
            ComputerKey::V
        );
    }

    #[test]
    fn perform_input_mapping_roundtrips_in_global_settings() {
        use crate::domains::perform::{ComputerKey, PadPosition};

        let mut settings = UiSettings::default();
        settings
            .perform_input_mapping
            .rebind(PadPosition::ALL[0], ComputerKey::Y);
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();

        assert_eq!(
            loaded.input_mapping_key(PadPosition::ALL[0]),
            ComputerKey::Y
        );
    }

    #[test]
    fn fixed_computer_velocity_defaults_and_roundtrips_globally() {
        let old: UiSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.fixed_computer_velocity, 100);

        let settings = UiSettings {
            fixed_computer_velocity: 73,
            ..UiSettings::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(loaded.fixed_computer_velocity, 73);
    }

    #[test]
    fn track_mute_quantization_defaults_to_immediate_and_roundtrips_globally() {
        use vibez_core::perform::TrackMuteQuantization;

        let old: UiSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.track_mute_quantization,
            TrackMuteQuantization::Immediate
        );

        let settings = UiSettings {
            track_mute_quantization: TrackMuteQuantization::OneBar,
            ..UiSettings::default()
        };
        let loaded: UiSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(
            loaded.track_mute_quantization,
            TrackMuteQuantization::OneBar
        );
    }

    #[test]
    fn rebinding_global_input_does_not_change_project_bytes() {
        use crate::domains::perform::{ComputerKey, PadPosition};
        use vibez_core::perform::TrackMuteQuantization;

        let project = vibez_project::Project::default();
        let before = serde_json::to_vec(&project).unwrap();
        let mut settings = UiSettings::default();
        settings
            .perform_input_mapping
            .rebind(PadPosition::ALL[0], ComputerKey::Y);
        settings.track_mute_quantization = TrackMuteQuantization::OneBar;
        let after = serde_json::to_vec(&project).unwrap();

        assert_eq!(before, after);
        assert_ne!(
            settings.perform_input_mapping,
            crate::domains::perform::PerformInputMapping::default()
        );
    }
}
