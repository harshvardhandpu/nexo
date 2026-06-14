use common::{
    Checkpoint, ChunkId, FileManifest, PeerId, ResumeMetadata, SessionId, SessionInfo,
    SessionState, TransferId,
};
use rusqlite::{Connection, OptionalExtension, params};
use std::io::{Error, ErrorKind, Result};
use std::path::Path;
use std::time::Duration;

const SCHEMA_VERSION: i64 = 1;

pub trait CheckpointStore {
    fn save_checkpoint(&mut self, checkpoint: &Checkpoint) -> Result<()>;
    fn load_checkpoint(&self, transfer_id: &TransferId) -> Result<Option<Checkpoint>>;
}

pub trait SessionStore {
    fn save_session(&mut self, session: &SessionInfo) -> Result<()>;
    fn load_session(&self, session_id: &SessionId) -> Result<Option<SessionInfo>>;
}

pub trait ResumeMetadataStore {
    fn save_resume_metadata(&mut self, metadata: &ResumeMetadata) -> Result<()>;
    fn load_resume_metadata(&self, transfer_id: &TransferId) -> Result<Option<ResumeMetadata>>;
}

pub trait StorageBackend {
    fn migrate(&mut self) -> Result<()>;
    fn save_checkpoint_record(&mut self, record: &CheckpointRecord) -> Result<()>;
    fn load_checkpoint_record(&self, transfer_id: &TransferId) -> Result<Option<CheckpointRecord>>;
    fn save_session_record(&mut self, record: &SessionRecord) -> Result<()>;
    fn load_session_record(&self, session_id: &SessionId) -> Result<Option<SessionRecord>>;
    fn save_resume_metadata_record(&mut self, record: &ResumeMetadataRecord) -> Result<()>;
    fn load_resume_metadata_record(
        &self,
        transfer_id: &TransferId,
    ) -> Result<Option<ResumeMetadataRecord>>;
}

#[derive(Debug)]
pub struct Storage<B> {
    backend: B,
}

impl<B: StorageBackend> Storage<B> {
    pub fn new(mut backend: B) -> Result<Self> {
        backend.migrate()?;
        Ok(Self { backend })
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
}

impl<B: StorageBackend> CheckpointStore for Storage<B> {
    fn save_checkpoint(&mut self, checkpoint: &Checkpoint) -> Result<()> {
        self.backend
            .save_checkpoint_record(&CheckpointRecord::from(checkpoint))
    }

    fn load_checkpoint(&self, transfer_id: &TransferId) -> Result<Option<Checkpoint>> {
        self.backend
            .load_checkpoint_record(transfer_id)
            .map(|record| record.map(Checkpoint::from))
    }
}

impl<B: StorageBackend> SessionStore for Storage<B> {
    fn save_session(&mut self, session: &SessionInfo) -> Result<()> {
        self.backend
            .save_session_record(&SessionRecord::from(session))
    }

    fn load_session(&self, session_id: &SessionId) -> Result<Option<SessionInfo>> {
        self.backend
            .load_session_record(session_id)
            .map(|record| record.map(SessionInfo::from))
    }
}

impl<B: StorageBackend> ResumeMetadataStore for Storage<B> {
    fn save_resume_metadata(&mut self, metadata: &ResumeMetadata) -> Result<()> {
        self.backend
            .save_resume_metadata_record(&ResumeMetadataRecord::from(metadata))
    }

    fn load_resume_metadata(&self, transfer_id: &TransferId) -> Result<Option<ResumeMetadata>> {
        self.backend
            .load_resume_metadata_record(transfer_id)
            .map(|record| record.map(ResumeMetadata::from))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRecord {
    pub transfer_id: TransferId,
    pub completed_chunks: Vec<ChunkId>,
}

impl From<&Checkpoint> for CheckpointRecord {
    fn from(checkpoint: &Checkpoint) -> Self {
        Self {
            transfer_id: checkpoint.transfer_id.clone(),
            completed_chunks: checkpoint.completed_chunks.clone(),
        }
    }
}

impl From<CheckpointRecord> for Checkpoint {
    fn from(record: CheckpointRecord) -> Self {
        Self {
            transfer_id: record.transfer_id,
            completed_chunks: record.completed_chunks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
    pub local_peer: PeerId,
    pub remote_peer: PeerId,
    pub state: SessionState,
}

impl From<&SessionInfo> for SessionRecord {
    fn from(session: &SessionInfo) -> Self {
        Self {
            session_id: session.session_id.clone(),
            transfer_id: session.transfer_id.clone(),
            local_peer: session.local_peer.clone(),
            remote_peer: session.remote_peer.clone(),
            state: session.state,
        }
    }
}

impl From<SessionRecord> for SessionInfo {
    fn from(record: SessionRecord) -> Self {
        Self {
            session_id: record.session_id,
            transfer_id: record.transfer_id,
            local_peer: record.local_peer,
            remote_peer: record.remote_peer,
            state: record.state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeMetadataRecord {
    pub transfer_id: TransferId,
    pub manifest: FileManifest,
    pub checkpoint: CheckpointRecord,
}

impl From<&ResumeMetadata> for ResumeMetadataRecord {
    fn from(metadata: &ResumeMetadata) -> Self {
        Self {
            transfer_id: metadata.transfer_id.clone(),
            manifest: metadata.manifest.clone(),
            checkpoint: CheckpointRecord::from(&metadata.checkpoint),
        }
    }
}

impl From<ResumeMetadataRecord> for ResumeMetadata {
    fn from(record: ResumeMetadataRecord) -> Self {
        Self {
            transfer_id: record.transfer_id,
            manifest: record.manifest,
            checkpoint: Checkpoint::from(record.checkpoint),
        }
    }
}

#[derive(Debug)]
pub struct SqliteStorageBackend {
    connection: Connection,
}

impl SqliteStorageBackend {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let connection = Connection::open(path).map_err(sqlite_error)?;
        Self::configure_connection(&connection)?;
        Ok(Self { connection })
    }

    pub fn in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory().map_err(sqlite_error)?;
        Self::configure_connection(&connection)?;
        Ok(Self { connection })
    }

    /// Configures a connection so that concurrent writers (for example the
    /// sender and receiver processes sharing one state directory) do not crash
    /// with "database is locked" during a large transfer.
    ///
    /// - WAL journaling lets a writer commit without blocking readers and keeps
    ///   write transactions short, instead of taking a database-wide exclusive
    ///   lock for the duration of every per-chunk checkpoint write.
    /// - `synchronous = NORMAL` is the safe, fast pairing for WAL: commits no
    ///   longer fsync the whole database, so the write lock is held briefly.
    /// - An explicit busy timeout makes a momentarily blocked writer wait for
    ///   the lock instead of failing immediately.
    fn configure_connection(connection: &Connection) -> Result<()> {
        connection
            .busy_timeout(Duration::from_secs(30))
            .map_err(sqlite_error)?;
        // `PRAGMA journal_mode` returns the resulting mode, so read it back. A
        // file database becomes "wal"; an in-memory database stays "memory".
        connection
            .query_row("PRAGMA journal_mode = WAL", [], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sqlite_error)?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(sqlite_error)?;
        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    fn save_checkpoint_with_connection(
        connection: &Connection,
        record: &CheckpointRecord,
    ) -> Result<()> {
        connection
            .execute(
                "INSERT INTO checkpoints (transfer_id)
                 VALUES (?1)
                 ON CONFLICT(transfer_id) DO UPDATE SET transfer_id = excluded.transfer_id",
                params![record.transfer_id.0],
            )
            .map_err(sqlite_error)?;

        connection
            .execute(
                "DELETE FROM checkpoint_chunks WHERE transfer_id = ?1",
                params![record.transfer_id.0],
            )
            .map_err(sqlite_error)?;

        for chunk in &record.completed_chunks {
            let chunk_id = i64::try_from(chunk.0).map_err(integer_error)?;
            connection
                .execute(
                    "INSERT INTO checkpoint_chunks (transfer_id, chunk_id)
                     VALUES (?1, ?2)",
                    params![record.transfer_id.0, chunk_id],
                )
                .map_err(sqlite_error)?;
        }

        Ok(())
    }

    fn load_checkpoint_with_connection(
        connection: &Connection,
        transfer_id: &TransferId,
    ) -> Result<Option<CheckpointRecord>> {
        let exists = connection
            .query_row(
                "SELECT 1 FROM checkpoints WHERE transfer_id = ?1",
                params![transfer_id.0],
                |_| Ok(()),
            )
            .optional()
            .map_err(sqlite_error)?
            .is_some();

        if !exists {
            return Ok(None);
        }

        let mut statement = connection
            .prepare(
                "SELECT chunk_id
                 FROM checkpoint_chunks
                 WHERE transfer_id = ?1
                 ORDER BY chunk_id ASC",
            )
            .map_err(sqlite_error)?;

        let chunks = statement
            .query_map(params![transfer_id.0], |row| row.get::<_, i64>(0))
            .map_err(sqlite_error)?
            .map(|chunk| {
                let chunk = chunk.map_err(sqlite_error)?;
                let chunk = u64::try_from(chunk).map_err(integer_error)?;
                Ok(ChunkId(chunk))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Some(CheckpointRecord {
            transfer_id: transfer_id.clone(),
            completed_chunks: chunks,
        }))
    }
}

impl StorageBackend for SqliteStorageBackend {
    fn migrate(&mut self) -> Result<()> {
        self.connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS storage_migrations (
                    version INTEGER PRIMARY KEY
                );

                CREATE TABLE IF NOT EXISTS checkpoints (
                    transfer_id TEXT PRIMARY KEY NOT NULL
                );

                CREATE TABLE IF NOT EXISTS checkpoint_chunks (
                    transfer_id TEXT NOT NULL,
                    chunk_id INTEGER NOT NULL,
                    PRIMARY KEY (transfer_id, chunk_id),
                    FOREIGN KEY (transfer_id)
                        REFERENCES checkpoints(transfer_id)
                        ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS sessions (
                    session_id TEXT PRIMARY KEY NOT NULL,
                    transfer_id TEXT NOT NULL,
                    local_peer TEXT NOT NULL,
                    remote_peer TEXT NOT NULL,
                    state TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS resume_metadata (
                    transfer_id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    size INTEGER NOT NULL,
                    chunk_size INTEGER NOT NULL,
                    total_chunks INTEGER NOT NULL,
                    sha256 TEXT NOT NULL,
                    FOREIGN KEY (transfer_id)
                        REFERENCES checkpoints(transfer_id)
                        ON DELETE CASCADE
                );
                ",
            )
            .map_err(sqlite_error)?;

        self.connection
            .execute(
                "INSERT OR IGNORE INTO storage_migrations (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )
            .map_err(sqlite_error)?;

        Ok(())
    }

    fn save_checkpoint_record(&mut self, record: &CheckpointRecord) -> Result<()> {
        let transaction = self.connection.transaction().map_err(sqlite_error)?;
        Self::save_checkpoint_with_connection(&transaction, record)?;
        transaction.commit().map_err(sqlite_error)
    }

    fn load_checkpoint_record(&self, transfer_id: &TransferId) -> Result<Option<CheckpointRecord>> {
        Self::load_checkpoint_with_connection(&self.connection, transfer_id)
    }

    fn save_session_record(&mut self, record: &SessionRecord) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO sessions (
                    session_id,
                    transfer_id,
                    local_peer,
                    remote_peer,
                    state
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(session_id) DO UPDATE SET
                    transfer_id = excluded.transfer_id,
                    local_peer = excluded.local_peer,
                    remote_peer = excluded.remote_peer,
                    state = excluded.state",
                params![
                    record.session_id.0,
                    record.transfer_id.0,
                    record.local_peer.0,
                    record.remote_peer.0,
                    session_state_to_str(record.state),
                ],
            )
            .map_err(sqlite_error)?;

        Ok(())
    }

    fn load_session_record(&self, session_id: &SessionId) -> Result<Option<SessionRecord>> {
        self.connection
            .query_row(
                "SELECT transfer_id, local_peer, remote_peer, state
                 FROM sessions
                 WHERE session_id = ?1",
                params![session_id.0],
                |row| {
                    let state: String = row.get(3)?;
                    Ok(SessionRecord {
                        session_id: session_id.clone(),
                        transfer_id: TransferId(row.get(0)?),
                        local_peer: PeerId(row.get(1)?),
                        remote_peer: PeerId(row.get(2)?),
                        state: session_state_from_str(&state).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)
    }

    fn save_resume_metadata_record(&mut self, record: &ResumeMetadataRecord) -> Result<()> {
        let transaction = self.connection.transaction().map_err(sqlite_error)?;
        Self::save_checkpoint_with_connection(&transaction, &record.checkpoint)?;
        transaction
            .execute(
                "INSERT INTO resume_metadata (
                    transfer_id,
                    name,
                    size,
                    chunk_size,
                    total_chunks,
                    sha256
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(transfer_id) DO UPDATE SET
                    name = excluded.name,
                    size = excluded.size,
                    chunk_size = excluded.chunk_size,
                    total_chunks = excluded.total_chunks,
                    sha256 = excluded.sha256",
                params![
                    record.transfer_id.0,
                    record.manifest.name,
                    i64::try_from(record.manifest.size).map_err(integer_error)?,
                    i64::try_from(record.manifest.chunk_size).map_err(integer_error)?,
                    i64::try_from(record.manifest.total_chunks).map_err(integer_error)?,
                    record.manifest.sha256,
                ],
            )
            .map_err(sqlite_error)?;
        transaction.commit().map_err(sqlite_error)
    }

    fn load_resume_metadata_record(
        &self,
        transfer_id: &TransferId,
    ) -> Result<Option<ResumeMetadataRecord>> {
        let manifest = self
            .connection
            .query_row(
                "SELECT name, size, chunk_size, total_chunks, sha256
                 FROM resume_metadata
                 WHERE transfer_id = ?1",
                params![transfer_id.0],
                |row| {
                    let size: i64 = row.get(1)?;
                    let chunk_size: i64 = row.get(2)?;
                    let total_chunks: i64 = row.get(3)?;

                    Ok(FileManifest {
                        name: row.get(0)?,
                        size: u64::try_from(size).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                1,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })?,
                        chunk_size: u64::try_from(chunk_size).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                2,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })?,
                        total_chunks: u64::try_from(total_chunks).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                3,
                                rusqlite::types::Type::Integer,
                                Box::new(error),
                            )
                        })?,
                        sha256: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(sqlite_error)?;

        let Some(manifest) = manifest else {
            return Ok(None);
        };

        let checkpoint = Self::load_checkpoint_with_connection(&self.connection, transfer_id)?
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "resume checkpoint is missing"))?;

        Ok(Some(ResumeMetadataRecord {
            transfer_id: transfer_id.clone(),
            manifest,
            checkpoint,
        }))
    }
}

fn session_state_to_str(state: SessionState) -> &'static str {
    match state {
        SessionState::Created => "created",
        SessionState::Connecting => "connecting",
        SessionState::PendingAcceptance => "pending_acceptance",
        SessionState::Accepted => "accepted",
        SessionState::Transferring => "transferring",
        SessionState::Paused => "paused",
        SessionState::Verifying => "verifying",
        SessionState::Completed => "completed",
        SessionState::Failed => "failed",
        SessionState::Cancelled => "cancelled",
    }
}

fn session_state_from_str(value: &str) -> std::result::Result<SessionState, InvalidSessionState> {
    match value {
        "created" => Ok(SessionState::Created),
        "connecting" => Ok(SessionState::Connecting),
        "pending_acceptance" => Ok(SessionState::PendingAcceptance),
        "accepted" => Ok(SessionState::Accepted),
        "transferring" => Ok(SessionState::Transferring),
        "paused" => Ok(SessionState::Paused),
        "verifying" => Ok(SessionState::Verifying),
        "completed" => Ok(SessionState::Completed),
        "failed" => Ok(SessionState::Failed),
        "cancelled" => Ok(SessionState::Cancelled),
        _ => Err(InvalidSessionState(value.to_owned())),
    }
}

#[derive(Debug)]
struct InvalidSessionState(String);

impl std::fmt::Display for InvalidSessionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid session state: {}", self.0)
    }
}

impl std::error::Error for InvalidSessionState {}

fn sqlite_error(error: rusqlite::Error) -> Error {
    Error::other(error)
}

fn integer_error(error: std::num::TryFromIntError) -> Error {
    Error::new(ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn sqlite_storage_creates_schema() {
        let storage = sqlite_storage();

        let version: i64 = storage
            .backend()
            .connection()
            .query_row(
                "SELECT version FROM storage_migrations WHERE version = ?1",
                params![SCHEMA_VERSION],
                |row| row.get(0),
            )
            .expect("schema version");

        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn checkpoint_persistence_round_trips_completed_chunks() {
        let mut storage = sqlite_storage();
        let checkpoint = test_checkpoint(vec![ChunkId(0), ChunkId(2), ChunkId(5)]);

        storage
            .save_checkpoint(&checkpoint)
            .expect("save checkpoint");
        let loaded = storage
            .load_checkpoint(&checkpoint.transfer_id)
            .expect("load checkpoint");

        assert_eq!(loaded, Some(checkpoint));
    }

    #[test]
    fn checkpoint_persistence_updates_existing_rows() {
        let mut storage = sqlite_storage();
        let transfer_id = TransferId("transfer-1".to_owned());

        storage
            .save_checkpoint(&Checkpoint {
                transfer_id: transfer_id.clone(),
                completed_chunks: vec![ChunkId(0), ChunkId(1)],
            })
            .expect("initial checkpoint");
        storage
            .save_checkpoint(&Checkpoint {
                transfer_id: transfer_id.clone(),
                completed_chunks: vec![ChunkId(2)],
            })
            .expect("updated checkpoint");

        let loaded = storage
            .load_checkpoint(&transfer_id)
            .expect("load checkpoint")
            .expect("checkpoint exists");

        assert_eq!(loaded.completed_chunks, vec![ChunkId(2)]);
    }

    #[test]
    fn missing_checkpoint_returns_none() {
        let storage = sqlite_storage();

        let loaded = storage
            .load_checkpoint(&TransferId("missing".to_owned()))
            .expect("load missing checkpoint");

        assert_eq!(loaded, None);
    }

    #[test]
    fn session_persistence_round_trips_session_info() {
        let mut storage = sqlite_storage();
        let session = test_session(SessionState::Transferring);

        storage.save_session(&session).expect("save session");
        let loaded = storage
            .load_session(&session.session_id)
            .expect("load session");

        assert_eq!(loaded, Some(session));
    }

    #[test]
    fn session_persistence_updates_existing_rows() {
        let mut storage = sqlite_storage();
        let mut session = test_session(SessionState::Created);

        storage.save_session(&session).expect("save session");
        session.state = SessionState::Paused;
        storage.save_session(&session).expect("update session");

        let loaded = storage
            .load_session(&session.session_id)
            .expect("load session")
            .expect("session exists");

        assert_eq!(loaded.state, SessionState::Paused);
    }

    #[test]
    fn resume_metadata_persistence_round_trips_manifest_and_checkpoint() {
        let mut storage = sqlite_storage();
        let metadata = test_resume_metadata(vec![ChunkId(1), ChunkId(3)]);

        storage
            .save_resume_metadata(&metadata)
            .expect("save resume metadata");
        let loaded = storage
            .load_resume_metadata(&metadata.transfer_id)
            .expect("load resume metadata");

        assert_eq!(loaded, Some(metadata));
    }

    #[test]
    fn resume_metadata_persistence_updates_checkpoint() {
        let mut storage = sqlite_storage();
        let transfer_id = TransferId("transfer-1".to_owned());
        let mut metadata = test_resume_metadata(vec![ChunkId(1)]);
        metadata.transfer_id = transfer_id.clone();
        metadata.checkpoint.transfer_id = transfer_id.clone();

        storage
            .save_resume_metadata(&metadata)
            .expect("save resume metadata");
        metadata.checkpoint.completed_chunks = vec![ChunkId(2), ChunkId(4)];
        storage
            .save_resume_metadata(&metadata)
            .expect("update resume metadata");

        let loaded = storage
            .load_resume_metadata(&transfer_id)
            .expect("load resume metadata")
            .expect("metadata exists");

        assert_eq!(
            loaded.checkpoint.completed_chunks,
            vec![ChunkId(2), ChunkId(4)]
        );
    }

    #[test]
    fn persisted_checkpoint_survives_restart() {
        let path = temp_db_path("checkpoint-restart");
        let checkpoint = test_checkpoint(vec![ChunkId(0), ChunkId(9)]);

        {
            let mut storage = file_storage(&path);
            storage
                .save_checkpoint(&checkpoint)
                .expect("save checkpoint");
        }

        {
            let storage = file_storage(&path);
            let loaded = storage
                .load_checkpoint(&checkpoint.transfer_id)
                .expect("load checkpoint after restart");
            assert_eq!(loaded, Some(checkpoint));
        }

        fs::remove_file(path).ok();
    }

    #[test]
    fn persisted_session_survives_restart() {
        let path = temp_db_path("session-restart");
        let session = test_session(SessionState::Accepted);

        {
            let mut storage = file_storage(&path);
            storage.save_session(&session).expect("save session");
        }

        {
            let storage = file_storage(&path);
            let loaded = storage
                .load_session(&session.session_id)
                .expect("load session after restart");
            assert_eq!(loaded, Some(session));
        }

        fs::remove_file(path).ok();
    }

    #[test]
    fn persisted_resume_metadata_survives_restart() {
        let path = temp_db_path("resume-restart");
        let metadata = test_resume_metadata(vec![ChunkId(2), ChunkId(7)]);

        {
            let mut storage = file_storage(&path);
            storage
                .save_resume_metadata(&metadata)
                .expect("save resume metadata");
        }

        {
            let storage = file_storage(&path);
            let loaded = storage
                .load_resume_metadata(&metadata.transfer_id)
                .expect("load resume metadata after restart");
            assert_eq!(loaded, Some(metadata));
        }

        fs::remove_file(path).ok();
    }

    #[test]
    fn open_configures_wal_and_busy_timeout() {
        let path = temp_db_path("wal-config");
        {
            let storage = file_storage(&path);
            let connection = storage.backend().connection();
            let journal_mode: String = connection
                .query_row("PRAGMA journal_mode", [], |row| row.get(0))
                .expect("journal_mode");
            let busy_timeout: i64 = connection
                .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
                .expect("busy_timeout");

            assert_eq!(journal_mode.to_lowercase(), "wal");
            assert!(
                busy_timeout >= 1000,
                "busy_timeout must be configured, got {busy_timeout}"
            );
        }
        remove_database(&path);
    }

    #[test]
    fn concurrent_writers_on_shared_database_do_not_lock() {
        // Reproduces the receiver crash during large transfers: the sender and
        // receiver processes share one state directory, so two independent
        // connections write per-chunk checkpoints to the same database file at
        // the same time. Each per-chunk write rewrites a growing checkpoint
        // (DELETE + N inserts) plus resume metadata, holding the write lock long
        // enough that, without WAL journaling and a busy timeout, the loser of
        // the lock race fails with "database is locked".
        let path = temp_db_path("concurrent-writers");
        let chunks_per_writer = 400u64;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

        let spawn_writer = |label: &'static str| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut storage = file_storage(&path);
                let transfer_id = TransferId(format!("transfer-{label}"));
                barrier.wait();
                for chunk in 0..chunks_per_writer {
                    let checkpoint = Checkpoint {
                        transfer_id: transfer_id.clone(),
                        completed_chunks: (0..=chunk).map(ChunkId).collect(),
                    };
                    storage
                        .save_resume_metadata(&ResumeMetadata {
                            transfer_id: transfer_id.clone(),
                            manifest: FileManifest {
                                name: format!("{label}.bin"),
                                size: 4 * 1024 * 1024 * chunks_per_writer,
                                chunk_size: 4 * 1024 * 1024,
                                total_chunks: chunks_per_writer,
                                sha256: "sha256".to_owned(),
                            },
                            checkpoint: checkpoint.clone(),
                        })
                        .unwrap_or_else(|error| panic!("writer {label} resume failed: {error}"));
                    storage
                        .save_checkpoint(&checkpoint)
                        .unwrap_or_else(|error| {
                            panic!("writer {label} checkpoint failed: {error}")
                        });
                }
            })
        };

        let receiver = spawn_writer("receiver");
        let sender = spawn_writer("sender");
        receiver.join().expect("receiver writer thread");
        sender.join().expect("sender writer thread");

        remove_database(&path);
    }

    fn sqlite_storage() -> Storage<SqliteStorageBackend> {
        Storage::new(SqliteStorageBackend::in_memory().expect("sqlite backend"))
            .expect("sqlite storage")
    }

    fn file_storage(path: &Path) -> Storage<SqliteStorageBackend> {
        Storage::new(SqliteStorageBackend::open(path).expect("sqlite backend"))
            .expect("sqlite storage")
    }

    fn test_checkpoint(completed_chunks: Vec<ChunkId>) -> Checkpoint {
        Checkpoint {
            transfer_id: TransferId("transfer-1".to_owned()),
            completed_chunks,
        }
    }

    fn test_session(state: SessionState) -> SessionInfo {
        SessionInfo {
            session_id: SessionId("session-1".to_owned()),
            transfer_id: TransferId("transfer-1".to_owned()),
            local_peer: PeerId("peer-a".to_owned()),
            remote_peer: PeerId("peer-b".to_owned()),
            state,
        }
    }

    fn test_resume_metadata(completed_chunks: Vec<ChunkId>) -> ResumeMetadata {
        let transfer_id = TransferId("transfer-1".to_owned());

        ResumeMetadata {
            transfer_id: transfer_id.clone(),
            manifest: FileManifest {
                name: "file.bin".to_owned(),
                size: 10,
                chunk_size: 4,
                total_chunks: 3,
                sha256: "sha256".to_owned(),
            },
            checkpoint: Checkpoint {
                transfer_id,
                completed_chunks,
            },
        }
    }

    fn temp_db_path(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "nexo-storage-{label}-{}-{unique}.sqlite",
            std::process::id()
        ))
    }

    fn remove_database(path: &Path) {
        fs::remove_file(path).ok();
        // WAL journaling leaves -wal/-shm sidecar files; remove them too.
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = path.as_os_str().to_owned();
            sidecar.push(suffix);
            fs::remove_file(std::path::PathBuf::from(sidecar)).ok();
        }
    }
}
