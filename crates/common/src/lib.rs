#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRequest {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
    pub from_peer: PeerId,
    pub to_peer: PeerId,
    pub manifest: FileManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferAcceptance {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRejection {
    pub session_id: SessionId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferResponse {
    Accepted(TransferAcceptance),
    Rejected(TransferRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Created,
    Connecting,
    PendingAcceptance,
    Accepted,
    Transferring,
    Paused,
    Verifying,
    Completed,
    Failed,
    Cancelled,
}

impl SessionState {
    pub fn can_transition_to(self, next: SessionState) -> bool {
        use SessionState::{
            Accepted, Cancelled, Completed, Connecting, Created, Failed, Paused, PendingAcceptance,
            Transferring, Verifying,
        };

        matches!(
            (self, next),
            (Created, Connecting | Cancelled | Failed)
                | (Connecting, PendingAcceptance | Cancelled | Failed)
                | (PendingAcceptance, Accepted | Cancelled | Failed)
                | (Accepted, Transferring | Cancelled | Failed)
                | (Transferring, Paused | Verifying | Cancelled | Failed)
                | (Paused, Transferring | Cancelled | Failed)
                | (Verifying, Completed | Failed)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionState::Completed | SessionState::Failed | SessionState::Cancelled
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
    pub local_peer: PeerId,
    pub remote_peer: PeerId,
    pub state: SessionState,
}

impl SessionInfo {
    pub fn new(
        session_id: SessionId,
        transfer_id: TransferId,
        local_peer: PeerId,
        remote_peer: PeerId,
    ) -> Self {
        Self {
            session_id,
            transfer_id,
            local_peer,
            remote_peer,
            state: SessionState::Created,
        }
    }

    pub fn transition_to(&mut self, next: SessionState) -> Result<(), SessionTransitionError> {
        if self.state.can_transition_to(next) {
            self.state = next;
            return Ok(());
        }

        Err(SessionTransitionError {
            from: self.state,
            to: next,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTransitionError {
    pub from: SessionState,
    pub to: SessionState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_state_accepts_valid_transfer_lifecycle() {
        let mut session = test_session();

        session
            .transition_to(SessionState::Connecting)
            .expect("connecting");
        session
            .transition_to(SessionState::PendingAcceptance)
            .expect("pending acceptance");
        session
            .transition_to(SessionState::Accepted)
            .expect("accepted");
        session
            .transition_to(SessionState::Transferring)
            .expect("transferring");
        session
            .transition_to(SessionState::Verifying)
            .expect("verifying");
        session
            .transition_to(SessionState::Completed)
            .expect("completed");

        assert_eq!(session.state, SessionState::Completed);
        assert!(session.state.is_terminal());
    }

    #[test]
    fn session_state_supports_pause_and_resume() {
        let mut session = accepted_session();

        session
            .transition_to(SessionState::Transferring)
            .expect("transferring");
        session.transition_to(SessionState::Paused).expect("paused");
        session
            .transition_to(SessionState::Transferring)
            .expect("resumed");

        assert_eq!(session.state, SessionState::Transferring);
    }

    #[test]
    fn session_state_rejects_invalid_shortcut_to_transferring() {
        let mut session = test_session();

        let error = session
            .transition_to(SessionState::Transferring)
            .expect_err("invalid transition");

        assert_eq!(
            error,
            SessionTransitionError {
                from: SessionState::Created,
                to: SessionState::Transferring,
            }
        );
        assert_eq!(session.state, SessionState::Created);
    }

    #[test]
    fn session_state_rejects_transitions_from_terminal_states() {
        let mut session = accepted_session();
        session
            .transition_to(SessionState::Transferring)
            .expect("transferring");
        session
            .transition_to(SessionState::Verifying)
            .expect("verifying");
        session
            .transition_to(SessionState::Completed)
            .expect("completed");

        let error = session
            .transition_to(SessionState::Transferring)
            .expect_err("terminal transition");

        assert_eq!(
            error,
            SessionTransitionError {
                from: SessionState::Completed,
                to: SessionState::Transferring,
            }
        );
    }

    #[test]
    fn transfer_response_models_acceptance_flow() {
        let request = TransferRequest {
            session_id: SessionId("session-1".to_owned()),
            transfer_id: TransferId("transfer-1".to_owned()),
            from_peer: PeerId("peer-a".to_owned()),
            to_peer: PeerId("peer-b".to_owned()),
            manifest: FileManifest {
                name: "file.bin".to_owned(),
                size: 12,
                chunk_size: 4,
                total_chunks: 3,
                sha256: "hash".to_owned(),
            },
        };

        let response = TransferResponse::Accepted(TransferAcceptance {
            session_id: request.session_id.clone(),
            transfer_id: request.transfer_id.clone(),
        });

        assert_eq!(
            response,
            TransferResponse::Accepted(TransferAcceptance {
                session_id: SessionId("session-1".to_owned()),
                transfer_id: TransferId("transfer-1".to_owned()),
            })
        );
    }

    fn accepted_session() -> SessionInfo {
        let mut session = test_session();
        session
            .transition_to(SessionState::Connecting)
            .expect("connecting");
        session
            .transition_to(SessionState::PendingAcceptance)
            .expect("pending");
        session
            .transition_to(SessionState::Accepted)
            .expect("accepted");
        session
    }

    fn test_session() -> SessionInfo {
        SessionInfo::new(
            SessionId("session-1".to_owned()),
            TransferId("transfer-1".to_owned()),
            PeerId("peer-a".to_owned()),
            PeerId("peer-b".to_owned()),
        )
    }
}
