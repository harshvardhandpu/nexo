#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkId(pub u64);

pub const DEFAULT_CHUNK_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileManifest {
    pub name: String,
    pub size: u64,
    pub chunk_size: u64,
    pub total_chunks: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub transfer_id: TransferId,
    pub completed_chunks: Vec<ChunkId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeMetadata {
    pub transfer_id: TransferId,
    pub manifest: FileManifest,
    pub checkpoint: Checkpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingChunks {
    pub transfer_id: TransferId,
    pub chunks: Vec<ChunkId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferProgress {
    pub transfer_id: TransferId,
    pub completed_chunks: u64,
    pub total_chunks: u64,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub id: ChunkId,
    pub offset: u64,
    pub size: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMetadata {
    pub id: ChunkId,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
}
