//! Production Project Format V1 SQLite container.
//!
//! Project documents and playback-critical Project Media commit together.
//! The container preserves unchanged media rows during ordinary saves and can
//! create a self-contained Save As copy without returning to Source Storage.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vibez_core::track::{InstrumentStateInfo, MediaProvenance, MediaSourceRef, TrackInfo};

use crate::Project;

pub const FORMAT_VERSION: u32 = 1;
/// ASCII `VZP1`, stored in SQLite's application-id header field.
pub const APPLICATION_ID: u32 = 0x565a_5031;
static STAGING_NONCE: AtomicU64 = AtomicU64::new(0);

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_name: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectFileFormat {
    V1,
    LegacyJson,
}

#[derive(Debug, Clone)]
pub struct SaveResult {
    pub project: Project,
    pub observation: SaveObservation,
}

#[derive(Debug)]
pub enum ProjectFormatError {
    Io(std::io::Error),
    Sql(rusqlite::Error),
    Json(serde_json::Error),
    InvalidContainer(String),
    InvalidProvenance(String),
    MissingMedia(String),
}

impl std::fmt::Display for ProjectFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Sql(error) => write!(f, "SQLite error: {error}"),
            Self::Json(error) => write!(f, "JSON error: {error}"),
            Self::InvalidContainer(message) => {
                write!(f, "invalid Project Format V1 container: {message}")
            }
            Self::InvalidProvenance(message) => write!(f, "invalid media provenance: {message}"),
            Self::MissingMedia(id) => write!(f, "project media {id:?} was not found"),
        }
    }
}

impl std::error::Error for ProjectFormatError {}

impl From<std::io::Error> for ProjectFormatError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ProjectFormatError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sql(value)
    }
}

impl From<serde_json::Error> for ProjectFormatError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub struct ProjectContainer {
    path: PathBuf,
    connection: Connection,
}

impl ProjectContainer {
    /// First save: transactionally builds a new container under a private
    /// sibling path and atomically publishes it after commit, replacing any
    /// existing destination the caller has already confirmed overwriting. A
    /// failed save never leaves a partial container at `path`, so retries
    /// stay possible. Staging copies are content-addressed and shared, so
    /// commit leaves them in place; the staging sweep owns their cleanup.
    pub fn create_from_staged(
        path: impl AsRef<Path>,
        document: &mut ProjectDocumentV1,
        staged_media: &[StagedMedia],
    ) -> Result<(Self, SaveObservation), ProjectFormatError> {
        let path = path.as_ref();
        let started = Instant::now();
        let temporary = sibling_temp_path(path);
        let mut build = || -> Result<(usize, u64), ProjectFormatError> {
            let mut connection = Connection::open(&temporary)?;
            configure_connection(&connection)?;
            initialize_schema_markers(&connection)?;
            connection.execute_batch(SCHEMA)?;

            let transaction = connection.transaction()?;
            let mut media_bytes_written = 0_u64;
            let mut media_rows_written = 0_usize;
            for staged in staged_media {
                let content = fs::read(&staged.path)?;
                let entry = media_entry(staged, &content);
                if insert_media(&transaction, &entry, &content)? {
                    media_rows_written += 1;
                    media_bytes_written += content.len() as u64;
                }
                if !document
                    .project_media
                    .iter()
                    .any(|item| item.id == entry.id)
                {
                    document.project_media.push(entry);
                }
            }
            write_document(&transaction, document)?;
            transaction.commit()?;
            Ok((media_rows_written, media_bytes_written))
        };
        let published = build().and_then(|counts| {
            fs::rename(&temporary, path)?;
            Ok(counts)
        });
        let (media_rows_written, media_bytes_written) = match published {
            Ok(counts) => counts,
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                return Err(error);
            }
        };

        let container = Self::open(path)?;
        let observation = SaveObservation {
            elapsed: started.elapsed(),
            container_bytes: fs::metadata(path)?.len(),
            media_rows_written,
            media_bytes_written,
        };
        Ok((container, observation))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, ProjectFormatError> {
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

    pub fn load_document(&self) -> Result<ProjectDocumentV1, ProjectFormatError> {
        let json: Vec<u8> = self.connection.query_row(
            "SELECT json FROM project_document WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        let document: ProjectDocumentV1 = serde_json::from_slice(&json)?;
        if document.format_version != FORMAT_VERSION {
            return Err(ProjectFormatError::InvalidContainer(format!(
                "document version is {}, expected {FORMAT_VERSION}",
                document.format_version
            )));
        }
        Ok(document)
    }

    pub fn read_media(&self, id: &str) -> Result<Vec<u8>, ProjectFormatError> {
        self.connection
            .query_row(
                "SELECT content FROM project_media WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| ProjectFormatError::MissingMedia(id.to_owned()))
    }

    pub fn media_fingerprint(&self, id: &str) -> Result<(i64, String, u64), ProjectFormatError> {
        self.connection
            .query_row(
                "SELECT rowid, sha256, byte_len FROM project_media WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? as u64)),
            )
            .optional()?
            .ok_or_else(|| ProjectFormatError::MissingMedia(id.to_owned()))
    }

    /// Incrementally updates only the versioned project document. The media
    /// table is not touched; the observation makes that explicit.
    pub fn save_document(
        &mut self,
        document: &ProjectDocumentV1,
    ) -> Result<SaveObservation, ProjectFormatError> {
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

    /// Commits newly staged Project Media and the document in one transaction.
    /// Existing content-addressed rows are retained without rewriting.
    pub fn save_with_staged(
        &mut self,
        document: &mut ProjectDocumentV1,
        staged_media: &[StagedMedia],
    ) -> Result<SaveObservation, ProjectFormatError> {
        let started = Instant::now();
        let transaction = self.connection.transaction()?;
        let mut media_rows_written = 0_usize;
        let mut media_bytes_written = 0_u64;
        for staged in staged_media {
            let content = fs::read(&staged.path)?;
            let entry = media_entry(staged, &content);
            if insert_media(&transaction, &entry, &content)? {
                media_rows_written += 1;
                media_bytes_written += content.len() as u64;
            }
            if !document
                .project_media
                .iter()
                .any(|item| item.id == entry.id)
            {
                document.project_media.push(entry);
            }
        }
        write_document(&transaction, document)?;
        transaction.commit()?;
        Ok(SaveObservation {
            elapsed: started.elapsed(),
            container_bytes: fs::metadata(&self.path)?.len(),
            media_rows_written,
            media_bytes_written,
        })
    }

    /// Creates a self-contained snapshot without changing the source path.
    /// The copy is built under a private sibling path, validated, and
    /// atomically renamed into place, replacing any existing destination the
    /// caller has already confirmed overwriting.
    pub fn save_as(
        &self,
        destination: impl AsRef<Path>,
    ) -> Result<SaveObservation, ProjectFormatError> {
        let destination = destination.as_ref();
        let temporary = sibling_temp_path(destination);
        let temporary_text = temporary.to_str().ok_or_else(|| {
            ProjectFormatError::InvalidContainer("Save As destination is not valid UTF-8".into())
        })?;
        let started = Instant::now();
        let published = self
            .connection
            .execute("VACUUM INTO ?1", [temporary_text])
            .map_err(ProjectFormatError::from)
            .and_then(|_| {
                let copied = Self::open(&temporary)?;
                let document = copied.load_document()?;
                if document.format_version != FORMAT_VERSION {
                    return Err(ProjectFormatError::InvalidContainer(
                        "Save As lost format marker".into(),
                    ));
                }
                drop(copied);
                fs::rename(&temporary, destination)?;
                Ok(())
            });
        if let Err(error) = published {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(SaveObservation {
            elapsed: started.elapsed(),
            container_bytes: fs::metadata(destination)?.len(),
            media_rows_written: 0,
            media_bytes_written: 0,
        })
    }
}

pub fn detect_project_format(path: &Path) -> Result<ProjectFileFormat, ProjectFormatError> {
    let mut header = [0_u8; 16];
    let read = fs::File::open(path)?.read(&mut header)?;
    if read == header.len() && &header == b"SQLite format 3\0" {
        let container = ProjectContainer::open(path)?;
        container.load_document()?;
        Ok(ProjectFileFormat::V1)
    } else {
        Ok(ProjectFileFormat::LegacyJson)
    }
}

/// Copies Local Source Storage into a Vibez-owned staging location before an
/// unsaved project begins depending on it. The content hash is the eventual
/// Project Media identity, so repeated imports reuse the same staged bytes.
pub fn stage_local_file(path: &Path) -> Result<MediaSourceRef, ProjectFormatError> {
    let content = fs::read(path)?;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project-media".to_string());
    stage_local_content(path, &file_name, &content)
}

/// Stages Vibez-derived bytes while retaining the authoritative Local source
/// as provenance. Used when a device import deliberately bakes the heard WARP
/// treatment into project-owned media without touching Source Storage.
pub fn stage_local_content(
    source_path: &Path,
    file_name: &str,
    content: &[u8],
) -> Result<MediaSourceRef, ProjectFormatError> {
    let id = hex_sha256(content);
    let staging_path = write_staging_copy(&id, file_name, content)?;
    Ok(MediaSourceRef::StagedProjectMedia {
        id,
        file_name: file_name.to_string(),
        staging_path,
        source_path: source_path.to_path_buf(),
    })
}

/// Vibez-owned staging directory for content-addressed Project Media
/// copies. Entries may be referenced by any unsaved project in any running
/// instance, so nothing deletes them eagerly; [`sweep_stale_staging`]
/// reclaims entries old enough that no live session can still depend on
/// them.
pub fn staging_root() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("vibez")
        .join("staged-project-media")
}

/// Removes staging entries (including abandoned `.partial` temporaries)
/// older than `max_age` from the default staging root. Returns the number
/// of files removed.
pub fn sweep_stale_staging(max_age: Duration) -> usize {
    sweep_staging_root(&staging_root(), max_age)
}

/// Age-based sweep of a staging directory. Errors are non-fatal: a file in
/// use elsewhere simply survives until a later sweep.
pub fn sweep_staging_root(root: &Path, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let stale = metadata.is_file()
            && metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age > max_age);
        if stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn sibling_temp_path(path: &Path) -> PathBuf {
    let nonce = STAGING_NONCE.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "container".to_string());
    path.with_file_name(format!(".{name}.{}-{nonce}.saving", std::process::id()))
}

fn write_staging_copy(
    id: &str,
    file_name: &str,
    content: &[u8],
) -> Result<PathBuf, ProjectFormatError> {
    let root = staging_root();
    fs::create_dir_all(&root)?;
    let safe_name: String = file_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let staging_path = root.join(format!("{id}-{safe_name}"));
    // Always rewrite: identical content-addressed bytes, but the fresh
    // modification time keeps actively referenced entries clear of the
    // age-based staging sweep.
    let nonce = STAGING_NONCE.fetch_add(1, Ordering::Relaxed);
    let temporary = root.join(format!(
        ".{id}-{safe_name}-{}-{nonce}.partial",
        std::process::id()
    ));
    fs::write(&temporary, content)?;
    fs::rename(temporary, &staging_path)?;
    Ok(staging_path)
}

/// Copies a complete Remote materialization into project-owned staging while
/// retaining only credential-free Source Location identity.
pub fn stage_remote_file(
    path: &Path,
    file_name: &str,
    provenance: MediaProvenance,
) -> Result<MediaSourceRef, ProjectFormatError> {
    let content = fs::read(path)?;
    stage_remote_content(file_name, &content, provenance)
}

/// Stages Vibez-derived Remote bytes, used when a WARP device import bakes the
/// heard treatment before the first project save.
pub fn stage_remote_content(
    file_name: &str,
    content: &[u8],
    provenance: MediaProvenance,
) -> Result<MediaSourceRef, ProjectFormatError> {
    if !matches!(provenance, MediaProvenance::Remote { .. }) {
        return Err(ProjectFormatError::InvalidProvenance(
            "Remote staging requires Remote provenance".into(),
        ));
    }
    let id = hex_sha256(content);
    let staging_path = write_staging_copy(&id, file_name, content)?;
    Ok(MediaSourceRef::StagedRemoteProjectMedia {
        id,
        file_name: file_name.to_string(),
        staging_path,
        provenance: Box::new(provenance),
    })
}

/// Saves a production Project Format V1 container. `source_container` is the
/// currently open V1 path, when any; a different destination performs Save As
/// by copying that self-contained container before committing the new document.
pub fn save_project_v1(
    destination: &Path,
    source_container: Option<&Path>,
    mut project: Project,
) -> Result<SaveResult, ProjectFormatError> {
    let same_container = source_container.is_some_and(|source| source == destination);
    let mut staged_media = Vec::new();
    prepare_project_media(&mut project, &mut staged_media)?;

    let (mut container, mut document) = if same_container {
        let container = ProjectContainer::open(destination)?;
        let document = container.load_document()?;
        (container, document)
    } else if let Some(source) = source_container.filter(|source| source.exists()) {
        let source = ProjectContainer::open(source)?;
        source.save_as(destination)?;
        let copied = ProjectContainer::open(destination)?;
        let document = copied.load_document()?;
        (copied, document)
    } else {
        // No source container to copy media rows from: any Project Media
        // reference not satisfied by staged bytes would commit as a
        // dangling row and silently drop the clip on reopen. Fail loudly
        // instead (the open V1 container vanished or moved).
        if let Some(id) = first_dangling_project_media(&mut project, &staged_media) {
            return Err(ProjectFormatError::MissingMedia(id));
        }
        let mut document = ProjectDocumentV1::new(project.clone());
        let (_container, observation) =
            ProjectContainer::create_from_staged(destination, &mut document, &staged_media)?;
        return Ok(SaveResult {
            project,
            observation,
        });
    };

    document.project = project.clone();
    let observation = container.save_with_staged(&mut document, &staged_media)?;
    Ok(SaveResult {
        project,
        observation,
    })
}

/// Visits every media source reference in a project: audio clips, sampler
/// sources, and drum rack pads across tracks, master, and buses.
fn visit_sources_mut(project: &mut Project, visit: &mut dyn FnMut(&mut MediaSourceRef)) {
    for clip in &mut project.clips {
        if let Some(source) = &mut clip.source {
            visit(source);
        }
    }
    let tracks = project
        .tracks
        .iter_mut()
        .chain(project.master.iter_mut())
        .chain(project.buses.iter_mut());
    for track in tracks {
        match &mut track.native_instrument {
            Some(InstrumentStateInfo::Sampler {
                source: Some(source),
                ..
            }) => visit(source),
            Some(InstrumentStateInfo::DrumRack { pads }) => {
                for pad in pads {
                    if let Some(source) = &mut pad.source {
                        visit(source);
                    }
                }
            }
            _ => {}
        }
    }
}

/// First Project Media reference that neither an existing container nor the
/// staged bytes can satisfy.
fn first_dangling_project_media(project: &mut Project, staged: &[StagedMedia]) -> Option<String> {
    let mut dangling = None;
    visit_sources_mut(project, &mut |source| {
        if let MediaSourceRef::ProjectMedia { id, .. } = source {
            if dangling.is_none() && !staged.iter().any(|item| item.id == *id) {
                dangling = Some(id.clone());
            }
        }
    });
    dangling
}

/// Rewrites transient staged references back to durable Source Storage
/// identity for legacy JSON documents, which have no Project Media table. A
/// serialized staging path would dangle as soon as the staging cache is
/// swept, silently dropping the clip on reopen.
pub fn strip_staged_sources(project: &mut Project) {
    visit_sources_mut(project, &mut |source| match source {
        MediaSourceRef::StagedProjectMedia { source_path, .. } => {
            *source = MediaSourceRef::LocalFile {
                path: source_path.clone(),
            };
        }
        MediaSourceRef::StagedRemoteProjectMedia { provenance, .. } => {
            if let MediaProvenance::Remote {
                source_id,
                source_path,
                revision,
                ..
            } = provenance.as_ref()
            {
                *source = MediaSourceRef::DropboxFile {
                    path_lower: source_id.clone(),
                    display_path: source_path.clone(),
                    rev: revision.clone(),
                };
            }
        }
        _ => {}
    });
}

fn prepare_project_media(
    project: &mut Project,
    staged_media: &mut Vec<StagedMedia>,
) -> Result<(), ProjectFormatError> {
    for clip in &mut project.clips {
        if let Some(source) = &mut clip.source {
            prepare_source(source, staged_media)?;
            clip.file_path = None;
        }
    }
    for track in &mut project.tracks {
        prepare_track_sources(track, staged_media)?;
    }
    if let Some(master) = &mut project.master {
        prepare_track_sources(master, staged_media)?;
    }
    for bus in &mut project.buses {
        prepare_track_sources(bus, staged_media)?;
    }
    Ok(())
}

fn prepare_track_sources(
    track: &mut TrackInfo,
    staged_media: &mut Vec<StagedMedia>,
) -> Result<(), ProjectFormatError> {
    match &mut track.native_instrument {
        Some(InstrumentStateInfo::Sampler {
            source: Some(source),
            ..
        }) => prepare_source(source, staged_media)?,
        Some(InstrumentStateInfo::DrumRack { pads }) => {
            for pad in pads {
                if let Some(source) = &mut pad.source {
                    prepare_source(source, staged_media)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn prepare_source(
    source: &mut MediaSourceRef,
    staged_media: &mut Vec<StagedMedia>,
) -> Result<(), ProjectFormatError> {
    let (path, file_name, provenance) = match source {
        MediaSourceRef::LocalFile { path } => {
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project-media".to_string());
            (
                path.clone(),
                file_name,
                Provenance::Local {
                    source_path: path.clone(),
                },
            )
        }
        MediaSourceRef::StagedProjectMedia {
            file_name,
            staging_path,
            source_path,
            ..
        } => (
            staging_path.clone(),
            file_name.clone(),
            Provenance::Local {
                source_path: source_path.clone(),
            },
        ),
        MediaSourceRef::StagedRemoteProjectMedia {
            file_name,
            staging_path,
            provenance,
            ..
        } => (
            staging_path.clone(),
            file_name.clone(),
            project_provenance(provenance),
        ),
        MediaSourceRef::ProjectMedia { .. } | MediaSourceRef::DropboxFile { .. } => return Ok(()),
    };
    let content = match fs::read(&path) {
        Ok(content) => content,
        // Unavailable Local Source Storage keeps its reference: the save
        // preserves the pointer for a later relink instead of aborting or
        // silently dropping the clip. Staged copies must still be readable,
        // because their bytes exist nowhere else.
        Err(_) if matches!(source, MediaSourceRef::LocalFile { .. }) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let id = hex_sha256(&content);
    let retained_provenance = core_provenance(&provenance);
    if !staged_media.iter().any(|item| item.id == id) {
        staged_media.push(StagedMedia {
            id: id.clone(),
            file_name: file_name.clone(),
            path,
            provenance,
        });
    }
    *source = MediaSourceRef::ProjectMedia {
        id,
        file_name,
        provenance: Some(Box::new(retained_provenance)),
    };
    Ok(())
}

fn project_provenance(provenance: &MediaProvenance) -> Provenance {
    match provenance {
        MediaProvenance::Local { source_path } => Provenance::Local {
            source_path: source_path.clone(),
        },
        MediaProvenance::Remote {
            provider,
            connection_id,
            connection_name,
            source_id,
            source_path,
            revision,
        } => Provenance::Remote {
            provider: provider.clone(),
            connection_id: connection_id.clone(),
            connection_name: connection_name.clone(),
            source_id: source_id.clone(),
            source_path: source_path.clone(),
            revision: revision.clone(),
        },
    }
}

fn core_provenance(provenance: &Provenance) -> MediaProvenance {
    match provenance {
        Provenance::Local { source_path } => MediaProvenance::Local {
            source_path: source_path.clone(),
        },
        Provenance::Remote {
            provider,
            connection_id,
            connection_name,
            source_id,
            source_path,
            revision,
        } => MediaProvenance::Remote {
            provider: provider.clone(),
            connection_id: connection_id.clone(),
            connection_name: connection_name.clone(),
            source_id: source_id.clone(),
            source_path: source_path.clone(),
            revision: revision.clone(),
        },
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

fn validate_container(connection: &Connection) -> Result<(), ProjectFormatError> {
    let application_id: u32 =
        connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let user_version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if application_id != APPLICATION_ID || user_version != FORMAT_VERSION {
        return Err(ProjectFormatError::InvalidContainer(format!(
            "application_id={application_id:#x}, user_version={user_version}"
        )));
    }
    Ok(())
}

fn write_document(
    transaction: &Transaction<'_>,
    document: &ProjectDocumentV1,
) -> Result<(), ProjectFormatError> {
    if document.format_version != FORMAT_VERSION {
        return Err(ProjectFormatError::InvalidContainer(format!(
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
) -> Result<bool, ProjectFormatError> {
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO project_media(id, file_name, byte_len, sha256, provenance_json, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.id,
            entry.file_name,
            entry.byte_len as i64,
            entry.sha256,
            serde_json::to_vec(&entry.provenance)?,
            content
        ],
    )?;
    Ok(inserted == 1)
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
