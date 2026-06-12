use common::{Checkpoint, ResumeMetadata, TransferId};

pub trait CheckpointRepository {
    fn save_checkpoint(&mut self, checkpoint: &Checkpoint) -> std::io::Result<()>;
    fn load_checkpoint(&self, transfer_id: &TransferId) -> std::io::Result<Option<Checkpoint>>;
}

pub trait ResumeMetadataRepository {
    fn save_resume_metadata(&mut self, metadata: &ResumeMetadata) -> std::io::Result<()>;
    fn load_resume_metadata(
        &self,
        transfer_id: &TransferId,
    ) -> std::io::Result<Option<ResumeMetadata>>;
}
