pub mod chunker;

use common::{Checkpoint, Chunk, ChunkId, ChunkMetadata, FileManifest, TransferProgress};
use std::io::Result;

pub trait ChunkSource {
    fn read_chunk(&mut self, metadata: &ChunkMetadata) -> Result<Chunk>;
}

pub trait CheckpointSink {
    fn save_checkpoint(&mut self, checkpoint: &Checkpoint) -> Result<()>;
}

pub trait TransferProgressSink {
    fn report_progress(&mut self, progress: &TransferProgress);
}

pub trait ChunkScheduler {
    fn next_chunk(&mut self) -> Option<ChunkId>;
    fn mark_completed(&mut self, chunk_id: ChunkId);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPlan {
    pub manifest: FileManifest,
    pub chunks: Vec<ChunkMetadata>,
}
