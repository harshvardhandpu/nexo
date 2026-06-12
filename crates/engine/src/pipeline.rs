use crate::chunker::{
    chunk_file, generate_manifest, missing_chunks, read_chunk, sha256_file, verify_chunk,
};
use common::{
    Checkpoint, Chunk, ChunkId, ChunkMetadata, FileManifest, MessageEnvelope, MissingChunks,
    PeerId, ResumeMetadata, SessionId, SessionInfo, SessionState, TransferAcceptance,
    TransferChunkMessage, TransferId, TransferMessage, TransferRequest, TransferResponse,
    TransferSessionMessage, TransferVerificationMessage,
};
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub type PipelineResult<T> = std::result::Result<T, TransferPipelineError>;

#[derive(Debug)]
pub enum TransferPipelineError {
    Io(std::io::Error),
    InvalidMessage(&'static str),
    InvalidSessionTransition {
        from: SessionState,
        to: SessionState,
    },
    ChunkNotFound(ChunkId),
    ChunkVerificationFailed(ChunkId),
    FileVerificationFailed {
        expected: String,
        actual: String,
    },
    Cipher(String),
}

impl std::fmt::Display for TransferPipelineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransferPipelineError::Io(error) => write!(formatter, "io error: {error}"),
            TransferPipelineError::InvalidMessage(reason) => write!(formatter, "{reason}"),
            TransferPipelineError::InvalidSessionTransition { from, to } => {
                write!(formatter, "invalid session transition: {from:?} -> {to:?}")
            }
            TransferPipelineError::ChunkNotFound(chunk_id) => {
                write!(formatter, "chunk not found: {}", chunk_id.0)
            }
            TransferPipelineError::ChunkVerificationFailed(chunk_id) => {
                write!(formatter, "chunk verification failed: {}", chunk_id.0)
            }
            TransferPipelineError::FileVerificationFailed { expected, actual } => {
                write!(
                    formatter,
                    "file verification failed: expected {expected}, got {actual}"
                )
            }
            TransferPipelineError::Cipher(reason) => write!(formatter, "cipher error: {reason}"),
        }
    }
}

impl std::error::Error for TransferPipelineError {}

impl From<std::io::Error> for TransferPipelineError {
    fn from(error: std::io::Error) -> Self {
        TransferPipelineError::Io(error)
    }
}

pub trait TransferPayloadCipher {
    fn encrypt_chunk(&self, chunk_id: &ChunkId, plaintext: &[u8]) -> PipelineResult<Vec<u8>>;
    fn decrypt_chunk(&self, chunk_id: &ChunkId, ciphertext: &[u8]) -> PipelineResult<Vec<u8>>;
}

#[derive(Debug, Clone, Copy)]
pub struct PlaintextPayloadCipher;

impl TransferPayloadCipher for PlaintextPayloadCipher {
    fn encrypt_chunk(&self, _chunk_id: &ChunkId, plaintext: &[u8]) -> PipelineResult<Vec<u8>> {
        Ok(plaintext.to_vec())
    }

    fn decrypt_chunk(&self, _chunk_id: &ChunkId, ciphertext: &[u8]) -> PipelineResult<Vec<u8>> {
        Ok(ciphertext.to_vec())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPipelineConfig {
    pub session_id: SessionId,
    pub transfer_id: TransferId,
    pub sender_peer: PeerId,
    pub receiver_peer: PeerId,
    pub chunk_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferPipelinePlan {
    pub request: TransferRequest,
    pub manifest: FileManifest,
    pub chunks: Vec<ChunkMetadata>,
    pub session: SessionInfo,
    chunk_index: HashMap<ChunkId, ChunkMetadata>,
}

impl TransferPipelinePlan {
    pub fn metadata_for(&self, chunk_id: &ChunkId) -> Option<&ChunkMetadata> {
        self.chunk_index.get(chunk_id)
    }

    pub fn initial_checkpoint(&self) -> Checkpoint {
        Checkpoint {
            transfer_id: self.request.transfer_id.clone(),
            completed_chunks: Vec::new(),
        }
    }

    pub fn resume_metadata(&self, checkpoint: Checkpoint) -> ResumeMetadata {
        ResumeMetadata {
            transfer_id: self.request.transfer_id.clone(),
            manifest: self.manifest.clone(),
            checkpoint,
        }
    }

    pub fn missing_chunks(&self, checkpoint: &Checkpoint) -> MissingChunks {
        missing_chunks(self.request.transfer_id.clone(), &self.manifest, checkpoint)
    }
}

#[derive(Debug, Clone)]
pub struct TransferPipelineSender {
    source_path: PathBuf,
    config: TransferPipelineConfig,
    plan: TransferPipelinePlan,
}

impl TransferPipelineSender {
    pub fn prepare<P: AsRef<Path>>(
        source_path: P,
        config: TransferPipelineConfig,
    ) -> PipelineResult<Self> {
        let source_path = source_path.as_ref().to_path_buf();
        let manifest = generate_manifest(&source_path, config.chunk_size)?;
        let chunks = chunk_file(&source_path, config.chunk_size)?;
        let request = TransferRequest {
            session_id: config.session_id.clone(),
            transfer_id: config.transfer_id.clone(),
            from_peer: config.sender_peer.clone(),
            to_peer: config.receiver_peer.clone(),
            manifest: manifest.clone(),
        };
        let session = SessionInfo::new(
            config.session_id.clone(),
            config.transfer_id.clone(),
            config.sender_peer.clone(),
            config.receiver_peer.clone(),
        );
        let chunk_index = chunks
            .iter()
            .map(|metadata| (metadata.id.clone(), metadata.clone()))
            .collect();

        Ok(Self {
            source_path,
            config,
            plan: TransferPipelinePlan {
                request,
                manifest,
                chunks,
                session,
                chunk_index,
            },
        })
    }

    pub fn plan(&self) -> &TransferPipelinePlan {
        &self.plan
    }

    pub fn session_request_envelope(&self) -> MessageEnvelope {
        MessageEnvelope {
            session_id: self.config.session_id.clone(),
            transfer_id: self.config.transfer_id.clone(),
            message: TransferMessage::Session(TransferSessionMessage::Request(
                self.plan.request.clone(),
            )),
        }
    }

    pub fn chunk_envelopes<C: TransferPayloadCipher>(
        &self,
        checkpoint: &Checkpoint,
        cipher: &C,
    ) -> PipelineResult<Vec<MessageEnvelope>> {
        self.chunk_envelopes_with_limit(checkpoint, cipher, usize::MAX)
    }

    pub fn chunk_envelopes_with_limit<C: TransferPayloadCipher>(
        &self,
        checkpoint: &Checkpoint,
        cipher: &C,
        max_chunks: usize,
    ) -> PipelineResult<Vec<MessageEnvelope>> {
        let completed = checkpoint
            .completed_chunks
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let mut envelopes = Vec::new();

        for metadata in &self.plan.chunks {
            if envelopes.len() >= max_chunks {
                break;
            }

            if completed.contains(&metadata.id) {
                continue;
            }

            let plaintext = read_chunk(&self.source_path, metadata)?;
            if !verify_chunk(&plaintext, metadata) {
                return Err(TransferPipelineError::ChunkVerificationFailed(
                    metadata.id.clone(),
                ));
            }

            let encrypted = cipher.encrypt_chunk(&metadata.id, &plaintext.data)?;
            envelopes.push(MessageEnvelope {
                session_id: self.config.session_id.clone(),
                transfer_id: self.config.transfer_id.clone(),
                message: TransferMessage::Chunk(TransferChunkMessage::Data(Chunk {
                    id: metadata.id.clone(),
                    offset: metadata.offset,
                    size: encrypted.len() as u64,
                    data: encrypted,
                })),
            });
        }

        Ok(envelopes)
    }

    pub fn complete_session(&self, session: &mut SessionInfo) -> PipelineResult<()> {
        transition_session(session, SessionState::Verifying)?;
        transition_session(session, SessionState::Completed)
    }
}

#[derive(Debug)]
pub struct TransferPipelineReceiver {
    output_path: PathBuf,
    session: SessionInfo,
    manifest: Option<FileManifest>,
    chunk_index: HashMap<ChunkId, ChunkMetadata>,
    checkpoint: Checkpoint,
}

impl TransferPipelineReceiver {
    pub fn new<P: AsRef<Path>>(
        output_path: P,
        session_id: SessionId,
        transfer_id: TransferId,
        local_peer: PeerId,
        remote_peer: PeerId,
        checkpoint: Checkpoint,
    ) -> Self {
        Self {
            output_path: output_path.as_ref().to_path_buf(),
            session: SessionInfo::new(session_id, transfer_id, local_peer, remote_peer),
            manifest: None,
            chunk_index: HashMap::new(),
            checkpoint,
        }
    }

    pub fn session(&self) -> &SessionInfo {
        &self.session
    }

    pub fn checkpoint(&self) -> &Checkpoint {
        &self.checkpoint
    }

    pub fn accept_transfer(
        &mut self,
        request_envelope: MessageEnvelope,
        chunks: &[ChunkMetadata],
    ) -> PipelineResult<MessageEnvelope> {
        let request = match request_envelope.message {
            TransferMessage::Session(TransferSessionMessage::Request(request)) => request,
            _ => {
                return Err(TransferPipelineError::InvalidMessage(
                    "expected transfer request",
                ));
            }
        };

        if request.session_id != self.session.session_id
            || request.transfer_id != self.session.transfer_id
            || request.to_peer != self.session.local_peer
            || request.from_peer != self.session.remote_peer
        {
            return Err(TransferPipelineError::InvalidMessage(
                "transfer request does not match receiver session",
            ));
        }

        transition_session(&mut self.session, SessionState::Connecting)?;
        transition_session(&mut self.session, SessionState::PendingAcceptance)?;
        transition_session(&mut self.session, SessionState::Accepted)?;
        self.manifest = Some(request.manifest);
        self.chunk_index = chunks
            .iter()
            .map(|metadata| (metadata.id.clone(), metadata.clone()))
            .collect();

        Ok(MessageEnvelope {
            session_id: self.session.session_id.clone(),
            transfer_id: self.session.transfer_id.clone(),
            message: TransferMessage::Session(TransferSessionMessage::Response(
                TransferResponse::Accepted(TransferAcceptance {
                    session_id: self.session.session_id.clone(),
                    transfer_id: self.session.transfer_id.clone(),
                }),
            )),
        })
    }

    pub fn receive_chunk<C: TransferPayloadCipher>(
        &mut self,
        envelope: MessageEnvelope,
        cipher: &C,
    ) -> PipelineResult<Checkpoint> {
        let chunk = match envelope.message {
            TransferMessage::Chunk(TransferChunkMessage::Data(chunk)) => chunk,
            _ => return Err(TransferPipelineError::InvalidMessage("expected chunk data")),
        };
        let metadata = self
            .chunk_index
            .get(&chunk.id)
            .ok_or_else(|| TransferPipelineError::ChunkNotFound(chunk.id.clone()))?
            .clone();
        let plaintext = cipher.decrypt_chunk(&chunk.id, &chunk.data)?;
        let plaintext_chunk = Chunk {
            id: metadata.id.clone(),
            offset: metadata.offset,
            size: metadata.size,
            data: plaintext,
        };

        if !verify_chunk(&plaintext_chunk, &metadata) {
            return Err(TransferPipelineError::ChunkVerificationFailed(
                metadata.id.clone(),
            ));
        }

        transition_session_if_needed(&mut self.session, SessionState::Transferring)?;
        write_chunk(&self.output_path, &plaintext_chunk)?;
        self.mark_completed(metadata.id);

        Ok(self.checkpoint.clone())
    }

    pub fn verify_complete(&mut self) -> PipelineResult<MessageEnvelope> {
        let manifest = self
            .manifest
            .as_ref()
            .ok_or(TransferPipelineError::InvalidMessage("manifest is missing"))?;

        transition_session_if_needed(&mut self.session, SessionState::Verifying)?;
        let actual = sha256_file(&self.output_path)?;

        if actual != manifest.sha256 {
            return Err(TransferPipelineError::FileVerificationFailed {
                expected: manifest.sha256.clone(),
                actual,
            });
        }

        transition_session(&mut self.session, SessionState::Completed)?;

        Ok(MessageEnvelope {
            session_id: self.session.session_id.clone(),
            transfer_id: self.session.transfer_id.clone(),
            message: TransferMessage::Verification(TransferVerificationMessage::FileVerified {
                transfer_id: self.session.transfer_id.clone(),
            }),
        })
    }

    pub fn reconstructed_bytes(&self) -> PipelineResult<Vec<u8>> {
        Ok(std::fs::read(&self.output_path)?)
    }

    fn mark_completed(&mut self, chunk_id: ChunkId) {
        if !self
            .checkpoint
            .completed_chunks
            .iter()
            .any(|completed| completed == &chunk_id)
        {
            self.checkpoint.completed_chunks.push(chunk_id);
            self.checkpoint
                .completed_chunks
                .sort_by_key(|completed| completed.0);
        }
    }
}

pub fn transition_session(session: &mut SessionInfo, next: SessionState) -> PipelineResult<()> {
    session
        .transition_to(next)
        .map_err(|error| TransferPipelineError::InvalidSessionTransition {
            from: error.from,
            to: error.to,
        })
}

fn transition_session_if_needed(
    session: &mut SessionInfo,
    next: SessionState,
) -> PipelineResult<()> {
    if session.state == next {
        return Ok(());
    }

    transition_session(session, next)
}

fn write_chunk(path: &Path, chunk: &Chunk) -> PipelineResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    file.seek(SeekFrom::Start(chunk.offset))?;
    file.write_all(&chunk.data)?;
    file.flush()?;

    Ok(())
}
