pub mod chunker;

use common::{
    Checkpoint, Chunk, ChunkId, ChunkMetadata, FileManifest, SessionId, SessionInfo, SessionState,
    TransferProgress, TransferRequest, TransferResponse,
};
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

pub trait TransferSessionCoordinator {
    fn create_session(&mut self, request: TransferRequest) -> Result<SessionInfo>;
    fn respond_to_transfer(&mut self, response: TransferResponse) -> Result<SessionInfo>;
    fn transition_session(
        &mut self,
        session_id: &SessionId,
        state: SessionState,
    ) -> Result<SessionInfo>;
}

pub trait TransferSessionObserver {
    fn session_updated(&mut self, session: &SessionInfo);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPlan {
    pub manifest: FileManifest,
    pub chunks: Vec<ChunkMetadata>,
}
