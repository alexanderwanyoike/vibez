//! Bounded Project Format V1 container proof.
//!
//! This module deliberately does not replace [`crate::Project`]'s production
//! JSON save/load path. It exists to measure and validate the SQLite-backed
//! container proposed for Project Format V1 before production persistence is
//! built on it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Project;

pub const FORMAT_VERSION: u32 = 1;
/// ASCII `VZP1`, stored in SQLite's application-id header field.
pub const APPLICATION_ID: u32 = 0x565a_5031;

const SCHEMA: &str = r#"
CREATE TABLE project_document (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    json BLOB NOT NULL
);
CREATE TABLE project_media (
    id TEXT PRIMARY KEY,
    file_name TEXT NOT NULL,
    byte_len INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    provenance_json BLOB NOT NULL,
    content BLOB NOT NULL
);
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    Local {
        source_path: PathBuf,
    },
    Remote {
        provider: String,
        connection_id: String,
        source_id: String,
        source_path: String,
        revision: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMediaEntry {
    pub id: String,
    pub file_name: String,
    pub byte_len: u64,
    pub sha256: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDocumentV1 {
    pub format_version: u32,
    pub project: Project,
    pub project_media: Vec<ProjectMediaEntry>,
}

impl ProjectDocumentV1 {
    pub fn new(project: Project) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            project,
            project_media: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StagedMedia {
    pub id: String,
    pub file_name: String,
    pub path: PathBuf,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy)]
pub struct SaveObservation {
    pub elapsed: Duration,
    pub container_bytes: u64,
    pub media_rows_written: usize,
    pub media_bytes_written: u64,
}

#[derive(Debug)]
pub enum ProofError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    InvalidContainer(String),
    MissingMedia(String),
    ExistingDestination(PathBuf),
}

impl std::fmt::Display for ProofError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Sql(error) => write!(f, "SQLite error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::InvalidContainer(message) => {
                write!(f, "invalid Project Format V1 proof container: {message}")
            }
            Self::MissingMedia(id) => write!(f, "project media {id:?} was not found"),
            Self::ExistingDestination(path) => {
                write!(f, "Save As destination already exists: {}", path.display())
            }
        }
    }
}

impl std::error::Error for ProofError {}

impl From<std::io::Error> for ProofError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ProofError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

impl From<serde_json::Error> for ProofError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub struct ProofContainer {
    path: PathBuf,
    connection: Connection,
}

impl ProofContainer {
    /// First-save proof: transactionally creates a new container and takes
    /// ownership of staged bytes. Staging files are removed only after commit.
    pub fn create_from_staged(
        path: impl AsRef<Path>,
        document: &mut ProjectDocumentV1,
        staged_media: &[StagedMedia],
    ) -> Result<(Self, SaveObservation), ProofError> {
        let path = path.as_ref();
        if path.exists() {
            return Err(ProofError::ExistingDestination(path.to_path_buf()));
        }

        let started = Instant::now();
        let mut connection = Connection::open(path)?;
        configure_connection(&connection)?;
        initialize_schema_markers(&connection)?;
        connection.execute_batch(SCHEMA)?;

        let transaction = connection.transaction()?;
        let mut media_bytes_written = 0_u64;
        for staged in staged_media {
            let content = fs::read(&staged.path)?;
            let entry = media_entry(staged, &content);
            insert_media(&transaction, &entry, &content)?;
            media_bytes_written += content.len() as u64;
            document.project_media.push(entry);
        }
        write_document(&transaction, document)?;
        transaction.commit()?;

        // A committed container owns the media; stale staging copies are no
        // longer needed. Cleanup failure is intentionally non-fatal because
        // losing the newly committed Project Media would be worse.
        for staged in staged_media {
            let _ = fs::remove_file(&staged.path);
        }

        let observation = SaveObservation {
            elapsed: started.elapsed(),
            container_bytes: fs::metadata(path)?.len(),
            media_rows_written: staged_media.len(),
            media_bytes_written,
        };
        Ok((
            Self {
                path: path.to_path_buf(),
                connection,
            },
            observation,
        ))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProofError> {
        let path = path.as_ref();
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        validate_container(&connection)?;
        configure_connection(&connection)?;
        Ok(Self {
            path: path.to_path_buf(),
            connection,
        })
    }

    pub fn load_document(&self) -> Result<ProjectDocumentV1, ProofError> {
        let json: Vec<u8> = self.connection.query_row(
            "SELECT json FROM project_document WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        let document: ProjectDocumentV1 = serde_json::from_slice(&json)?;
        if document.format_version != FORMAT_VERSION {
            return Err(ProofError::InvalidContainer(format!(
                "document version is {}, expected {FORMAT_VERSION}",
                document.format_version
            )));
        }
        Ok(document)
    }

    pub fn read_media(&self, id: &str) -> Result<Vec<u8>, ProofError> {
        self.connection
            .query_row(
                "SELECT content FROM project_media WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| ProofError::MissingMedia(id.to_owned()))
    }

    pub fn media_fingerprint(&self, id: &str) -> Result<(i64, String, u64), ProofError> {
        self.connection
            .query_row(
                "SELECT rowid, sha256, byte_len FROM project_media WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as u64)),
            )
            .optional()?
            .ok_or_else(|| ProofError::MissingMedia(id.to_owned()))
    }

    /// Incrementally updates only the versioned project document. The media
    /// table is not touched; the observation makes that explicit.
    pub fn save_document(
        &mut self,
        document: &ProjectDocumentV1,
    ) -> Result<SaveObservation, ProofError> {
        let started = Instant::now();
        let transaction = self.connection.transaction()?;
        write_document(&transaction, document)?;
        transaction.commit()?;
        Ok(SaveObservation {
            elapsed: started.elapsed(),
            container_bytes: fs::metadata(&self.path)?.len(),
            media_rows_written: 0,
            media_bytes_written: 0,
        })
    }

    /// Creates a self-contained snapshot without changing the source path.
    pub fn save_as(&self, destination: impl AsRef<Path>) -> Result<SaveObservation, ProofError> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(ProofError::ExistingDestination(destination.to_path_buf()));
        }
        let destination_text = destination.to_str().ok_or_else(|| {
            ProofError::InvalidContainer("Save As destination is not valid UTF-8".into())
        })?;
        let started = Instant::now();
        self.connection
            .execute("VACUUM INTO ?1", [destination_text])?;
        let reopened = Self::open(destination)?;
        let document = reopened.load_document()?;
        if document.format_version != FORMAT_VERSION {
            return Err(ProofError::InvalidContainer(
                "Save As lost format marker".into(),
            ));
        }
        Ok(SaveObservation {
            elapsed: started.elapsed(),
            container_bytes: fs::metadata(destination)?.len(),
            media_rows_written: 0,
            media_bytes_written: 0,
        })
    }
}

/// Starts a real transaction, overwrites the document and a media row, then
/// terminates the process without running destructors or committing. This is
/// only for the proof's child-process interruption test.
pub fn abort_mid_save(path: impl AsRef<Path>) -> ! {
    let mut connection = Connection::open(path).expect("open proof container before abort");
    configure_connection(&connection).expect("configure proof container before abort");
    let transaction = connection
        .transaction()
        .expect("start interrupted transaction");
    transaction
        .execute(
            "UPDATE project_document SET json = ?1 WHERE singleton = 1",
            [br#"{"interrupted":true}"#.as_slice()],
        )
        .expect("overwrite document inside interrupted transaction");
    transaction
        .execute(
            "UPDATE project_media SET content = zeroblob(byte_len), sha256 = 'interrupted' WHERE rowid = (SELECT MIN(rowid) FROM project_media)",
            [],
        )
        .expect("overwrite media inside interrupted transaction");
    std::process::exit(86)
}

fn configure_connection(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch("PRAGMA journal_mode = DELETE; PRAGMA synchronous = FULL;")?;
    Ok(())
}

fn validate_container(connection: &Connection) -> Result<(), ProofError> {
    let application_id: u32 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let user_version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if application_id != APPLICATION_ID || user_version != FORMAT_VERSION {
        return Err(ProofError::InvalidContainer(format!(
            "application_id={application_id:#x}, user_version={user_version}"
        )));
    }
    Ok(())
}

fn write_document(
    transaction: &Transaction<'_>,
    document: &ProjectDocumentV1,
) -> Result<(), ProofError> {
    if document.format_version != FORMAT_VERSION {
        return Err(ProofError::InvalidContainer(format!(
            "cannot save document version {} as version {FORMAT_VERSION}",
            document.format_version
        )));
    }
    transaction.execute(
        "INSERT INTO project_document(singleton, json) VALUES (1, ?1) ON CONFLICT(singleton) DO UPDATE SET json = excluded.json",
        [serde_json::to_vec(document)?],
    )?;
    Ok(())
}

fn insert_media(
    transaction: &Transaction<'_>,
    entry: &ProjectMediaEntry,
    content: &[u8],
) -> Result<(), ProofError> {
    transaction.execute(
        "INSERT INTO project_media(id, file_name, byte_len, sha256, provenance_json, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.id,
            entry.file_name,
            entry.byte_len as i64,
            entry.sha256,
            serde_json::to_vec(&entry.provenance)?,
            content
        ],
    )?;
    Ok(())
}

fn media_entry(staged: &StagedMedia, content: &[u8]) -> ProjectMediaEntry {
    ProjectMediaEntry {
        id: staged.id.clone(),
        file_name: staged.file_name.clone(),
        byte_len: content.len() as u64,
        sha256: hex_sha256(content),
        provenance: staged.provenance.clone(),
    }
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Representative non-media document used by the repeatable proof and its
/// tests. It includes MIDI, automation, native-device state and opaque
/// third-party plugin state.
pub fn representative_document() -> ProjectDocumentV1 {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    use vibez_core::effect::{EffectInfo, EffectType, PluginDeviceInfo};
    use vibez_core::id::{ClipId, EffectId};
    use vibez_core::midi::{MidiNote, NoteClipInfo};
    use vibez_core::track::{InstrumentStateInfo, TrackInfo};

    let mut track = TrackInfo::new("Proof instrument");
    track.native_instrument = Some(InstrumentStateInfo::SubtractiveSynth {
        params: vec![0.05, 0.2, 0.8, 0.4],
    });
    track.effects.push(EffectInfo {
        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![1.0],
        plugin: Some(PluginDeviceInfo {
            format: "clap".into(),
            uid: "org.vibez.proof-plugin".into(),
            path: PathBuf::from("/plugins/proof.clap"),
            name: "Opaque Proof Plugin".into(),
            state_b64: Some("AAECA/7/UGx1Z2luU3RhdGU=".into()),
        }),
    });
    let mut automation = AutomationLane::new(AutomationTarget::TrackGain);
    automation.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.25,
        curve: 0.0,
    });
    automation.insert_point(AutomationPoint {
        beat: 16.0,
        value: 0.9,
        curve: 0.35,
    });
    track.automation.push(automation);

    let track_id = track.id;
    let project = Project {
        name: "Project Format V1 proof".into(),
        bpm: 126.0,
        sample_rate: 48_000,
        tracks: vec![track],
        clips: Vec::new(),
        note_clips: vec![NoteClipInfo {
            id: ClipId::new(),
            track_id,
            name: "Proof pattern".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: vec![
                MidiNote {
                    pitch: 36,
                    velocity: 112,
                    start_beat: 0.0,
                    duration_beats: 0.25,
                },
                MidiNote {
                    pitch: 42,
                    velocity: 96,
                    start_beat: 0.5,
                    duration_beats: 0.125,
                },
            ],
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
        }],
        master: Some(TrackInfo::new("Master")),
        buses: vec![TrackInfo::new("Return A")],
    };
    ProjectDocumentV1::new(project)
}

pub fn initialize_schema_markers(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.pragma_update(None, "application_id", i64::from(APPLICATION_ID))?;
    connection.pragma_update(None, "user_version", i64::from(FORMAT_VERSION))?;
    Ok(())
}
