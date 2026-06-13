use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransferId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PeerId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub u64);

pub const DEFAULT_CHUNK_SIZE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifest {
    pub name: String,
    pub size: u64,
    pub chunk_size: u64,
    pub total_chunks: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub transfer_id: TransferId,
    pub completed_chunks: Vec<ChunkId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeMetadata {
    pub transfer_id: TransferId,
    pub manifest: FileManifest,
    pub checkpoint: Checkpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingChunks {
    pub transfer_id: TransferId,
    pub chunks: Vec<ChunkId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferDirection {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: TransferId,
    pub completed_chunks: u64,
    pub total_chunks: u64,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: ChunkId,
    pub offset: u64,
    pub size: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub id: ChunkId,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRequest {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
    pub from_peer: PeerId,
    pub to_peer: PeerId,
    pub manifest: FileManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferAcceptance {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRejection {
    pub session_id: SessionId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferResponse {
    Accepted(TransferAcceptance),
    Rejected(TransferRejection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTransitionError {
    pub from: SessionState,
    pub to: SessionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageCategory {
    Session,
    Control,
    Chunk,
    Verification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferMessage {
    Session(TransferSessionMessage),
    Control(TransferControlMessage),
    Chunk(TransferChunkMessage),
    Verification(TransferVerificationMessage),
}

impl TransferMessage {
    pub fn category(&self) -> MessageCategory {
        match self {
            TransferMessage::Session(_) => MessageCategory::Session,
            TransferMessage::Control(_) => MessageCategory::Control,
            TransferMessage::Chunk(_) => MessageCategory::Chunk,
            TransferMessage::Verification(_) => MessageCategory::Verification,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferSessionMessage {
    Request(TransferRequest),
    Response(TransferResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferControlMessage {
    KeyExchange {
        transfer_id: TransferId,
        public_key: Vec<u8>,
    },
    Acknowledged {
        transfer_id: TransferId,
    },
    Pause {
        transfer_id: TransferId,
    },
    Resume {
        transfer_id: TransferId,
    },
    Cancel {
        transfer_id: TransferId,
        reason: String,
    },
    Checkpoint(Checkpoint),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferChunkMessage {
    Metadata(ChunkMetadata),
    Data(Chunk),
    Missing(MissingChunks),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransferVerificationMessage {
    ChunkVerified {
        chunk_id: ChunkId,
    },
    ChunkRejected {
        chunk_id: ChunkId,
        reason: String,
    },
    FileVerified {
        transfer_id: TransferId,
    },
    FileRejected {
        transfer_id: TransferId,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
    pub message: TransferMessage,
}

impl MessageEnvelope {
    pub fn category(&self) -> MessageCategory {
        self.message.category()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportError {
    ConnectionFailed {
        connection_id: Option<ConnectionId>,
        reason: String,
    },
    ConnectionClosed {
        connection_id: ConnectionId,
    },
    StreamFailed {
        connection_id: ConnectionId,
        stream_id: StreamId,
        reason: String,
    },
    MessageRejected {
        reason: String,
    },
    Timeout {
        reason: String,
    },
    Protocol {
        reason: String,
    },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::ConnectionFailed {
                connection_id,
                reason,
            } => match connection_id {
                Some(connection_id) => {
                    write!(
                        formatter,
                        "connection failed ({}): {reason}",
                        connection_id.0
                    )
                }
                None => write!(formatter, "connection failed: {reason}"),
            },
            TransportError::ConnectionClosed { connection_id } => {
                write!(formatter, "connection closed: {}", connection_id.0)
            }
            TransportError::StreamFailed {
                connection_id,
                stream_id,
                reason,
            } => write!(
                formatter,
                "stream failed (connection {}, stream {}): {reason}",
                connection_id.0, stream_id.0
            ),
            TransportError::MessageRejected { reason } => {
                write!(formatter, "message rejected: {reason}")
            }
            TransportError::Timeout { reason } => write!(formatter, "timeout: {reason}"),
            TransportError::Protocol { reason } => write!(formatter, "protocol error: {reason}"),
        }
    }
}

impl std::error::Error for TransportError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportEvent {
    Connecting {
        connection_id: ConnectionId,
        peer_id: PeerId,
    },
    Connected {
        connection_id: ConnectionId,
        peer_id: PeerId,
    },
    StreamOpened {
        connection_id: ConnectionId,
        stream_id: StreamId,
    },
    MessageSent {
        connection_id: ConnectionId,
        stream_id: StreamId,
        envelope: MessageEnvelope,
    },
    MessageReceived {
        connection_id: ConnectionId,
        stream_id: StreamId,
        envelope: MessageEnvelope,
    },
    StreamClosed {
        connection_id: ConnectionId,
        stream_id: StreamId,
    },
    Closed {
        connection_id: ConnectionId,
    },
    Failed {
        error: TransportError,
    },
}

impl TransportEvent {
    pub fn session_state_hint(&self) -> Option<SessionState> {
        match self {
            TransportEvent::Connecting { .. } => Some(SessionState::Connecting),
            TransportEvent::Connected { .. } => Some(SessionState::PendingAcceptance),
            TransportEvent::MessageReceived { envelope, .. }
            | TransportEvent::MessageSent { envelope, .. } => match &envelope.message {
                TransferMessage::Session(TransferSessionMessage::Response(
                    TransferResponse::Accepted(_),
                )) => Some(SessionState::Accepted),
                TransferMessage::Control(TransferControlMessage::Pause { .. }) => {
                    Some(SessionState::Paused)
                }
                TransferMessage::Verification(TransferVerificationMessage::FileVerified {
                    ..
                }) => Some(SessionState::Completed),
                _ => None,
            },
            TransportEvent::Failed { .. } => Some(SessionState::Failed),
            _ => None,
        }
    }
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

    #[test]
    fn transfer_messages_report_categories() {
        let transfer_id = TransferId("transfer-1".to_owned());

        assert_eq!(
            TransferMessage::Control(TransferControlMessage::Pause {
                transfer_id: transfer_id.clone(),
            })
            .category(),
            MessageCategory::Control
        );
        assert_eq!(
            TransferMessage::Chunk(TransferChunkMessage::Missing(MissingChunks {
                transfer_id: transfer_id.clone(),
                chunks: vec![ChunkId(1)],
            }))
            .category(),
            MessageCategory::Chunk
        );
        assert_eq!(
            TransferMessage::Verification(TransferVerificationMessage::FileVerified {
                transfer_id,
            })
            .category(),
            MessageCategory::Verification
        );
    }

    #[test]
    fn transport_event_flow_provides_session_state_hints() {
        let connection_id = ConnectionId("connection-1".to_owned());
        let peer_id = PeerId("peer-b".to_owned());
        let stream_id = StreamId("stream-1".to_owned());
        let envelope = accepted_envelope();

        let events = [
            TransportEvent::Connecting {
                connection_id: connection_id.clone(),
                peer_id: peer_id.clone(),
            },
            TransportEvent::Connected {
                connection_id: connection_id.clone(),
                peer_id,
            },
            TransportEvent::StreamOpened {
                connection_id: connection_id.clone(),
                stream_id: stream_id.clone(),
            },
            TransportEvent::MessageReceived {
                connection_id,
                stream_id,
                envelope,
            },
        ];

        assert_eq!(
            events
                .iter()
                .filter_map(TransportEvent::session_state_hint)
                .collect::<Vec<_>>(),
            vec![
                SessionState::Connecting,
                SessionState::PendingAcceptance,
                SessionState::Accepted,
            ]
        );
    }

    #[test]
    fn transport_events_can_drive_valid_session_transitions() {
        let mut session = test_session();
        let connection_id = ConnectionId("connection-1".to_owned());
        let peer_id = PeerId("peer-b".to_owned());
        let stream_id = StreamId("stream-1".to_owned());

        let events = [
            TransportEvent::Connecting {
                connection_id: connection_id.clone(),
                peer_id: peer_id.clone(),
            },
            TransportEvent::Connected {
                connection_id: connection_id.clone(),
                peer_id,
            },
            TransportEvent::MessageReceived {
                connection_id,
                stream_id,
                envelope: accepted_envelope(),
            },
        ];

        for event in events {
            if let Some(next) = event.session_state_hint() {
                session.transition_to(next).expect("valid transition");
            }
        }

        assert_eq!(session.state, SessionState::Accepted);
    }

    #[test]
    fn transport_event_invalid_session_transition_is_rejected() {
        let mut session = test_session();
        let event = TransportEvent::MessageReceived {
            connection_id: ConnectionId("connection-1".to_owned()),
            stream_id: StreamId("stream-1".to_owned()),
            envelope: accepted_envelope(),
        };

        let next = event.session_state_hint().expect("state hint");
        let error = session.transition_to(next).expect_err("invalid transition");

        assert_eq!(
            error,
            SessionTransitionError {
                from: SessionState::Created,
                to: SessionState::Accepted,
            }
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

    fn accepted_envelope() -> MessageEnvelope {
        MessageEnvelope {
            session_id: SessionId("session-1".to_owned()),
            transfer_id: TransferId("transfer-1".to_owned()),
            message: TransferMessage::Session(TransferSessionMessage::Response(
                TransferResponse::Accepted(TransferAcceptance {
                    session_id: SessionId("session-1".to_owned()),
                    transfer_id: TransferId("transfer-1".to_owned()),
                }),
            )),
        }
    }
}
