use common::{Checkpoint, ResumeMetadata, SessionId, SessionInfo, TransferId};

pub trait CheckpointStore {
    fn save_checkpoint(&mut self, checkpoint: &Checkpoint) -> std::io::Result<()>;
    fn load_checkpoint(&self, transfer_id: &TransferId) -> std::io::Result<Option<Checkpoint>>;
}

pub trait SessionStore {
    fn save_session(&mut self, session: &SessionInfo) -> std::io::Result<()>;
    fn load_session(&self, session_id: &SessionId) -> std::io::Result<Option<SessionInfo>>;
}

pub trait ResumeMetadataStore {
    fn save_resume_metadata(&mut self, metadata: &ResumeMetadata) -> std::io::Result<()>;
    fn load_resume_metadata(
        &self,
        transfer_id: &TransferId,
    ) -> std::io::Result<Option<ResumeMetadata>>;
}
