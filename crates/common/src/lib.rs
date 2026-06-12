#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkId(pub u64);

#[derive(Debug, Clone)]
pub struct FileManifest {
    pub name: String,
    pub size: u64,
    pub chunk_size: u64,
    pub total_chunks: u64,
    pub sha256: String,
}

#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub transfer_id: TransferId,
    pub completed_chunks: Vec<ChunkId>,
}
