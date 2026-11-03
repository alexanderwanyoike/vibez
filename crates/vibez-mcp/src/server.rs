//! MCP tool surface: translates protocol calls into Vibez project operations.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, Json, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bounce::{BounceRange, BounceReport};
use crate::device_params::{self, AppliedParameter, Device, ParameterInfo};
use crate::project_file::SaveReport;
use crate::sample_library::{SampleHit, SampleInfo, SampleLibrary, SampleSource};
use crate::session::{NewAudioClip, NewNoteClip, NewTrack, ProjectSummary, TrackSession};

const INSTRUCTIONS: &str = "Builds Vibez projects. Typical flow: search_samples and \
inspect_sample to choose material (fetch_sample for Dropbox hits), create_project, add_track \
with a native instrument or none for audio, list_device_parameters then \
set_instrument_parameters / add_effect, place add_audio_clip and add_note_clip content, \
describe_project to check, bounce to hear it as a WAV and save_project for a .vzp the app \
opens. Timing is in beats from the project start (4 beats per bar); track 0 is the master.";

type ToolResult<T> = Result<Json<T>, String>;

const NO_PROJECT: &str = "no project is open; call create_project or open_project first";

#[derive(Clone)]
pub struct VibezServer {
    tool_router: ToolRouter<Self>,
    session: Arc<Mutex<Option<TrackSession>>>,
    library: Arc<SampleLibrary>,
}

impl Default for VibezServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl VibezServer {
    pub fn new() -> Self {
        Self::with_library(SampleLibrary::from_environment())
    }

    pub fn with_library(library: SampleLibrary) -> Self {
        Self {
            tool_router: Self::tool_router(),
            session: Arc::new(Mutex::new(None)),
            library: Arc::new(library),
        }
    }

    #[tool(
        description = "Search local sample folders and the Dropbox catalog. Every word \
must appear in the file's path. Dropbox hits without a local_path need fetch_sample first."
    )]
    async fn search_samples(&self, Parameters(r): Parameters<SearchRequest>) -> ToolResult<Hits> {
        let hits = self
            .library
            .search(&r.query, r.source, r.limit)
            .map_err(|e| e.to_string())?;
        Ok(Json(Hits { hits }))
    }

    #[tool(
        description = "Download a Dropbox sample from search results into Vibez's media \
cache and return its local path."
    )]
    async fn fetch_sample(
        &self,
        Parameters(r): Parameters<DropboxPathRequest>,
    ) -> ToolResult<Fetched> {
        let local_path = self
            .library
            .fetch(&r.path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(Fetched { local_path }))
    }

    #[tool(
        description = "Report a sample's length, channels, sample rate, peak and Vibez's \
tempo estimate."
    )]
    async fn inspect_sample(
        &self,
        Parameters(r): Parameters<PathRequest>,
    ) -> ToolResult<SampleInfo> {
        tokio::task::spawn_blocking(move || crate::sample_library::inspect(&r.path))
            .await
            .map_err(|e| e.to_string())?
            .map(Json)
            .map_err(|e| e.to_string())
    }

    #[tool(
        description = "List a native device's parameters with their ranges and defaults. \
Use these names when setting instrument or effect parameters."
    )]
    async fn list_device_parameters(
        &self,
        Parameters(r): Parameters<DeviceRequest>,
    ) -> Json<DeviceParameters> {
        Json(DeviceParameters {
            device: r.device,
            parameters: device_params::parameter_info(r.device),
        })
    }

    #[tool(description = "Start a new empty project, replacing the current one.")]
    async fn create_project(
        &self,
        Parameters(r): Parameters<CreateProjectRequest>,
    ) -> ToolResult<ProjectSummary> {
        let session = TrackSession::new(&r.name, r.bpm).map_err(|e| e.to_string())?;
        let summary = session.summary();
        *self.lock()? = Some(session);
        Ok(Json(summary))
    }

    #[tool(
        description = "Open an existing .vzp (or legacy JSON) project for editing, \
replacing the current one."
    )]
    async fn open_project(
        &self,
        Parameters(r): Parameters<PathRequest>,
    ) -> ToolResult<ProjectSummary> {
        self.blocking(move |slot| {
            let session = crate::project_file::open(&r.path).map_err(|e| e.to_string())?;
            let summary = session.summary();
            *slot = Some(session);
            Ok(Json(summary))
        })
        .await
    }

    #[tool(
        description = "Summarise the current project: tempo, length, tracks, devices and \
clip counts."
    )]
    async fn describe_project(&self) -> ToolResult<ProjectSummary> {
        self.edit(|session| Ok(Json(session.summary())))
    }

    #[tool(description = "Change the project tempo.")]
    async fn set_tempo(
        &self,
        Parameters(r): Parameters<TempoRequest>,
    ) -> ToolResult<ProjectSummary> {
        self.edit(|session| {
            session.set_tempo(r.bpm).map_err(|e| e.to_string())?;
            Ok(Json(session.summary()))
        })
    }

    #[tool(description = "Add a track. Give an instrument (subtractive_synth, \
wavetable_synth, fm_synth, or sampler with a sample path) for a MIDI track, or none for an \
audio track.")]
    async fn add_track(&self, Parameters(r): Parameters<NewTrack>) -> ToolResult<TrackCreated> {
        self.edit(|session| {
            let track_id = session.add_track(r).map_err(|e| e.to_string())?;
            Ok(Json(TrackCreated { track_id }))
        })
    }

    #[tool(description = "Set a track's volume in dB, pan from -1 to 1, or mute.")]
    async fn set_track_mix(
        &self,
        Parameters(r): Parameters<TrackMixRequest>,
    ) -> ToolResult<ProjectSummary> {
        self.edit(|session| {
            session
                .set_track_mix(r.track_id, r.gain_db, r.pan, r.mute)
                .map_err(|e| e.to_string())?;
            Ok(Json(session.summary()))
        })
    }

    #[tool(
        description = "Set named parameters on a track's instrument; values are clamped \
to the parameter's range."
    )]
    async fn set_instrument_parameters(
        &self,
        Parameters(r): Parameters<ParametersRequest>,
    ) -> ToolResult<Applied> {
        self.edit(|session| {
            let applied = session
                .set_instrument_parameters(r.track_id, &r.values)
                .map_err(|e| e.to_string())?;
            Ok(Json(Applied { applied }))
        })
    }

    #[tool(
        description = "Append a native effect to a track's chain (track 0 is the master) \
with optional named parameter values."
    )]
    async fn add_effect(
        &self,
        Parameters(r): Parameters<AddEffectRequest>,
    ) -> ToolResult<EffectCreated> {
        self.edit(|session| {
            let (effect_id, applied) = session
                .add_effect(r.track_id, r.device, &r.values)
                .map_err(|e| e.to_string())?;
            Ok(Json(EffectCreated { effect_id, applied }))
        })
    }

    #[tool(description = "Set named parameters on an effect already on a track.")]
    async fn set_effect_parameters(
        &self,
        Parameters(r): Parameters<EffectParametersRequest>,
    ) -> ToolResult<Applied> {
        self.edit(|session| {
            let applied = session
                .set_effect_parameters(r.track_id, r.effect_id, &r.values)
                .map_err(|e| e.to_string())?;
            Ok(Json(Applied { applied }))
        })
    }

    #[tool(
        description = "Place a sample on an audio track. loop_beats repeats the first \
part of the sample at the project tempo for length_beats."
    )]
    async fn add_audio_clip(
        &self,
        Parameters(r): Parameters<NewAudioClip>,
    ) -> ToolResult<ClipCreated> {
        self.edit(|session| {
            let clip_id = session.add_audio_clip(r).map_err(|e| e.to_string())?;
            Ok(Json(ClipCreated { clip_id }))
        })
    }

    #[tool(
        description = "Place a clip of MIDI notes on an instrument track. Note start \
beats are relative to the clip."
    )]
    async fn add_note_clip(
        &self,
        Parameters(r): Parameters<NewNoteClip>,
    ) -> ToolResult<ClipCreated> {
        self.edit(|session| {
            let clip_id = session.add_note_clip(r).map_err(|e| e.to_string())?;
            Ok(Json(ClipCreated { clip_id }))
        })
    }

    #[tool(
        description = "Save the project as a self-contained .vzp that Vibez opens, with \
every sample embedded."
    )]
    async fn save_project(&self, Parameters(r): Parameters<PathRequest>) -> ToolResult<SaveReport> {
        self.blocking(move |slot| {
            let session = slot.as_mut().ok_or(NO_PROJECT)?;
            crate::project_file::save(session, &r.path)
                .map(Json)
                .map_err(|e| e.to_string())
        })
        .await
    }

    #[tool(
        description = "Render the project to a WAV through the Vibez engine, like the \
app's Export. Optionally limit it to a beat range and set the tail length."
    )]
    async fn bounce(
        &self,
        Parameters(r): Parameters<BounceToolRequest>,
    ) -> ToolResult<BounceReport> {
        self.blocking(move |slot| {
            let session = slot.as_ref().ok_or(NO_PROJECT)?;
            let range = BounceRange {
                start_beat: r.start_beat,
                end_beat: r.end_beat,
                tail_seconds: r.tail_seconds,
            };
            crate::bounce::bounce(session, &r.path, &range)
                .map(Json)
                .map_err(|e| e.to_string())
        })
        .await
    }
}

impl VibezServer {
    fn lock(&self) -> Result<MutexGuard<'_, Option<TrackSession>>, String> {
        lock_session(&self.session)
    }

    fn edit<T>(
        &self,
        change: impl FnOnce(&mut TrackSession) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut slot = self.lock()?;
        change(slot.as_mut().ok_or(NO_PROJECT)?)
    }

    /// Runs disk and render work off the async executor.
    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Option<TrackSession>) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let session = Arc::clone(&self.session);
        tokio::task::spawn_blocking(move || {
            let mut slot = lock_session(&session)?;
            work(&mut slot)
        })
        .await
        .map_err(|error| format!("the tool task failed: {error}"))?
    }
}

fn lock_session(
    session: &Mutex<Option<TrackSession>>,
) -> Result<MutexGuard<'_, Option<TrackSession>>, String> {
    session
        .lock()
        .map_err(|_| "an earlier tool call panicked while editing the project".to_string())
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for VibezServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vibez-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

fn default_limit() -> usize {
    25
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub source: SampleSource,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Hits {
    pub hits: Vec<SampleHit>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DropboxPathRequest {
    /// The Dropbox path exactly as search_samples returned it.
    pub path: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Fetched {
    pub local_path: PathBuf,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathRequest {
    pub path: PathBuf,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeviceRequest {
    pub device: Device,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeviceParameters {
    pub device: Device,
    pub parameters: Vec<ParameterInfo>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateProjectRequest {
    pub name: String,
    pub bpm: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TempoRequest {
    pub bpm: f64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TrackCreated {
    pub track_id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TrackMixRequest {
    pub track_id: u64,
    #[serde(default)]
    pub gain_db: Option<f32>,
    /// -1 is hard left, 0 centre, 1 hard right.
    #[serde(default)]
    pub pan: Option<f32>,
    #[serde(default)]
    pub mute: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ParametersRequest {
    pub track_id: u64,
    /// Parameter names from list_device_parameters, mapped to values.
    pub values: BTreeMap<String, f32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Applied {
    pub applied: Vec<AppliedParameter>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddEffectRequest {
    pub track_id: u64,
    pub device: Device,
    #[serde(default)]
    pub values: BTreeMap<String, f32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EffectCreated {
    pub effect_id: u64,
    pub applied: Vec<AppliedParameter>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EffectParametersRequest {
    pub track_id: u64,
    pub effect_id: u64,
    pub values: BTreeMap<String, f32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ClipCreated {
    pub clip_id: u64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BounceToolRequest {
    pub path: PathBuf,
    #[serde(default)]
    pub start_beat: Option<f64>,
    #[serde(default)]
    pub end_beat: Option<f64>,
    #[serde(default)]
    pub tail_seconds: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Note;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_dropbox::DropboxCache;

    fn failure<T>(result: ToolResult<T>) -> String {
        match result {
            Ok(_) => panic!("expected the tool to fail"),
            Err(error) => error,
        }
    }

    fn server(dir: &std::path::Path) -> VibezServer {
        let samples = dir.join("samples");
        std::fs::create_dir_all(&samples).unwrap();
        let wave: Vec<f32> = (0..44_100).map(|i| 0.5 * (i as f32 * 0.05).sin()).collect();
        vibez_audio_io::file_io::write_wav_file(
            &samples.join("Dark Stab Ebm.wav"),
            &DecodedAudio {
                channels: vec![wave.clone(), wave],
                sample_rate: 44_100,
            },
        )
        .unwrap();
        VibezServer::with_library(SampleLibrary::new(
            vec![samples],
            None,
            DropboxCache::with_root(dir.join("cache")),
        ))
    }

    #[test]
    fn every_tool_is_listed_with_a_description() {
        let dir = tempfile::tempdir().unwrap();
        let tools = server(dir.path()).tool_router.list_all();
        let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
        for expected in [
            "search_samples",
            "fetch_sample",
            "inspect_sample",
            "list_device_parameters",
            "create_project",
            "open_project",
            "describe_project",
            "set_tempo",
            "add_track",
            "set_track_mix",
            "set_instrument_parameters",
            "add_effect",
            "set_effect_parameters",
            "add_audio_clip",
            "add_note_clip",
            "save_project",
            "bounce",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
        assert!(tools.iter().all(|t| t.description.is_some()));
    }

    #[tokio::test]
    async fn editing_without_a_project_explains_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let error = failure(server(dir.path()).describe_project().await);
        assert!(error.contains("create_project"), "{error}");
    }

    #[tokio::test]
    async fn a_track_is_built_saved_and_bounced_through_the_tools() {
        let dir = tempfile::tempdir().unwrap();
        let server = server(dir.path());
        let hit = server
            .search_samples(Parameters(SearchRequest {
                query: "dark stab".into(),
                source: SampleSource::Any,
                limit: 5,
            }))
            .await
            .unwrap()
            .0
            .hits
            .remove(0);
        let sample = hit.local_path.unwrap();

        server
            .create_project(Parameters(CreateProjectRequest {
                name: "tools".into(),
                bpm: 90.0,
            }))
            .await
            .unwrap();
        let keys = server
            .add_track(Parameters(NewTrack {
                name: "keys".into(),
                instrument: Some(Device::FmSynth),
                sample: None,
                gain_db: Some(-3.0),
                pan: None,
            }))
            .await
            .unwrap()
            .0
            .track_id;
        let stab = server
            .add_track(Parameters(NewTrack {
                name: "stab".into(),
                instrument: None,
                sample: None,
                gain_db: None,
                pan: None,
            }))
            .await
            .unwrap()
            .0
            .track_id;
        server
            .add_effect(Parameters(AddEffectRequest {
                track_id: keys,
                device: Device::Reverb,
                values: [("Mix".to_string(), 0.3)].into(),
            }))
            .await
            .unwrap();
        server
            .add_note_clip(Parameters(NewNoteClip {
                track_id: keys,
                start_beat: 0.0,
                length_beats: 4.0,
                notes: vec![Note {
                    pitch: 51,
                    velocity: 90,
                    start_beat: 0.0,
                    length_beats: 3.0,
                }],
            }))
            .await
            .unwrap();
        server
            .add_audio_clip(Parameters(NewAudioClip {
                track_id: stab,
                sample,
                start_beat: 4.0,
                length_beats: None,
                loop_beats: None,
                gain_db: None,
            }))
            .await
            .unwrap();

        let summary = server.describe_project().await.unwrap().0;
        assert_eq!(summary.tracks.len(), 2);
        assert_eq!(summary.length_beats, 4.0 + 1.5);

        let saved = server
            .save_project(Parameters(PathRequest {
                path: dir.path().join("tools.vzp"),
            }))
            .await
            .unwrap()
            .0;
        assert_eq!(saved.embedded_media, 1);

        let report = server
            .bounce(Parameters(BounceToolRequest {
                path: dir.path().join("tools.wav"),
                start_beat: None,
                end_beat: None,
                tail_seconds: Some(1.0),
            }))
            .await
            .unwrap()
            .0;
        assert!(report.peak_dbfs > -30.0, "peak {}", report.peak_dbfs);

        let reopened = server
            .open_project(Parameters(PathRequest {
                path: dir.path().join("tools.vzp"),
            }))
            .await
            .unwrap()
            .0;
        assert_eq!(reopened.tracks.len(), 2);
    }

    #[tokio::test]
    async fn tool_errors_carry_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let server = server(dir.path());
        server
            .create_project(Parameters(CreateProjectRequest {
                name: "errors".into(),
                bpm: 120.0,
            }))
            .await
            .unwrap();
        let error = failure(
            server
                .set_instrument_parameters(Parameters(ParametersRequest {
                    track_id: 999,
                    values: BTreeMap::new(),
                }))
                .await,
        );
        assert!(error.contains("999"), "{error}");
        let error = failure(
            server
                .create_project(Parameters(CreateProjectRequest {
                    name: "slow".into(),
                    bpm: 1.0,
                }))
                .await,
        );
        assert!(error.contains("tempo"), "{error}");
    }
}
