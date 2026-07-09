use common::{
    Checkpoint, ChunkId, PeerId, SessionId, SessionState, TransferId, TransferMessage,
    TransferVerificationMessage,
};
use crypto::{Encryptor, EphemeralKeyPair, KeyExchange, SessionCipher};
use engine::chunker::sha256_file;
use engine::pipeline::{
    PipelineResult, TransferPayloadCipher, TransferPipelineConfig, TransferPipelineError,
    TransferPipelineReceiver, TransferPipelineSender, reconcile_checkpoint,
};
use networking::{
    LoopbackTransportProvider, TransportConnection, TransportListener, TransportProvider,
    TransportStream,
};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use storage::{
    CheckpointStore, ResumeMetadataStore, SessionStore, SqliteStorageBackend,
    Storage as PersistentStorage,
};

#[test]
fn sender_transfers_file_to_receiver_and_marks_complete() {
    let workspace = TempWorkspace::new("complete-transfer");
    let source = workspace.path("source.bin");
    let output = workspace.path("received.bin");
    let data = b"nexo transfer pipeline moves chunks end to end";
    write_file(&source, data);

    let mut storage = in_memory_storage();
    let cipher = session_cipher();
    let sender = TransferPipelineSender::prepare(&source, config(8)).expect("sender pipeline");
    let mut checkpoint = sender.plan().initial_checkpoint();

    storage
        .save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))
        .expect("save resume metadata");

    let mut receiver = TransferPipelineReceiver::new(
        &output,
        session_id(),
        transfer_id(),
        receiver_peer(),
        sender_peer(),
        checkpoint.clone(),
    );
    let (_sender_connection, _receiver_connection, mut sender_stream, mut receiver_stream) =
        connected_streams();

    exchange_request_and_acceptance(
        &sender,
        &mut receiver,
        &mut sender_stream,
        &mut receiver_stream,
    );
    storage
        .save_session(receiver.session())
        .expect("save accepted session");

    for envelope in sender
        .chunk_envelopes(&checkpoint, &cipher)
        .expect("chunk envelopes")
    {
        sender_stream.send_message(envelope).expect("send chunk");
        let received = receiver_stream.receive_message().expect("receive chunk");
        checkpoint = receiver
            .receive_chunk(received, &cipher)
            .expect("verified chunk");
        storage
            .save_checkpoint(&checkpoint)
            .expect("save checkpoint");
        storage
            .save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))
            .expect("save resume metadata");
    }

    let verified = receiver.verify_complete().expect("verify complete");
    receiver_stream
        .send_message(verified)
        .expect("send verification");
    let verification = sender_stream
        .receive_message()
        .expect("receive verification");
    storage
        .save_session(receiver.session())
        .expect("save completed session");

    assert!(matches!(
        verification.message,
        TransferMessage::Verification(TransferVerificationMessage::FileVerified { .. })
    ));
    assert_eq!(
        receiver.reconstructed_bytes().expect("received bytes"),
        data
    );
    assert_eq!(
        sha256_file(&output).expect("received hash"),
        sender.plan().manifest.sha256
    );
    assert_eq!(
        storage
            .load_checkpoint(&transfer_id())
            .expect("load checkpoint")
            .expect("checkpoint exists")
            .completed_chunks,
        vec![
            ChunkId(0),
            ChunkId(1),
            ChunkId(2),
            ChunkId(3),
            ChunkId(4),
            ChunkId(5)
        ]
    );
    assert_eq!(
        storage
            .load_session(&session_id())
            .expect("load session")
            .expect("session exists")
            .state,
        SessionState::Completed
    );
}

#[test]
fn sender_detects_source_modified_during_transfer() {
    // If the source file changes between prepare (manifest) and building a chunk
    // envelope, the sender must refuse with an actionable SourceChunkModified
    // error naming the chunk and both hashes — not the cryptic, receiver-flavored
    // "chunk verification failed". This is the sender-local integrity guard.
    let workspace = TempWorkspace::new("source-modified");
    let source = workspace.path("source.bin");
    write_file(&source, b"original contents for chunk zero and beyond");

    let cipher = session_cipher();
    let sender = TransferPipelineSender::prepare(&source, config(8)).expect("sender pipeline");

    // Mutate the source after the plan/manifest was captured.
    write_file(&source, b"TAMPERED contents for chunk zero and beyond!!");

    let error = sender
        .chunk_envelope(&ChunkId(0), &cipher)
        .expect_err("modified source must fail chunk build");

    match error {
        TransferPipelineError::SourceChunkModified {
            chunk_id,
            expected_sha256,
            actual_sha256,
        } => {
            assert_eq!(chunk_id, ChunkId(0));
            assert_ne!(
                expected_sha256, actual_sha256,
                "hashes must differ when the source changed"
            );
        }
        other => panic!("expected SourceChunkModified, got {other:?}"),
    }

    // And the message is actionable (mentions the file changing).
    let message = sender
        .chunk_envelope(&ChunkId(0), &cipher)
        .expect_err("still fails")
        .to_string();
    assert!(
        message.contains("source file changed during transfer"),
        "unexpected message: {message}"
    );
}

#[test]
fn interrupted_transfer_resumes_from_checkpoint() {
    let workspace = TempWorkspace::new("resume-transfer");
    let source = workspace.path("source.bin");
    let output = workspace.path("received.bin");
    let database = workspace.path("transfer-state.sqlite");
    let data = b"resume this transfer after only part of the file arrived";
    write_file(&source, data);

    let cipher = session_cipher();
    let sender = TransferPipelineSender::prepare(&source, config(7)).expect("sender pipeline");
    let mut checkpoint = sender.plan().initial_checkpoint();

    {
        let mut storage = file_storage(&database);
        let mut receiver = TransferPipelineReceiver::new(
            &output,
            session_id(),
            transfer_id(),
            receiver_peer(),
            sender_peer(),
            checkpoint.clone(),
        );
        let (_sender_connection, _receiver_connection, mut sender_stream, mut receiver_stream) =
            connected_streams();

        storage
            .save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))
            .expect("save initial resume metadata");
        exchange_request_and_acceptance(
            &sender,
            &mut receiver,
            &mut sender_stream,
            &mut receiver_stream,
        );

        for envelope in sender
            .chunk_envelopes_with_limit(&checkpoint, &cipher, 2)
            .expect("partial chunk envelopes")
        {
            sender_stream.send_message(envelope).expect("send chunk");
            let received = receiver_stream.receive_message().expect("receive chunk");
            checkpoint = receiver
                .receive_chunk(received, &cipher)
                .expect("verified chunk");
            storage
                .save_checkpoint(&checkpoint)
                .expect("save partial checkpoint");
            storage
                .save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))
                .expect("save partial resume metadata");
        }

        assert_eq!(checkpoint.completed_chunks, vec![ChunkId(0), ChunkId(1)]);
    }

    let loaded_checkpoint = {
        let storage = file_storage(&database);
        let metadata = storage
            .load_resume_metadata(&transfer_id())
            .expect("load resume metadata")
            .expect("resume metadata exists");
        assert_eq!(
            metadata.checkpoint.completed_chunks,
            vec![ChunkId(0), ChunkId(1)]
        );
        metadata.checkpoint
    };

    let remaining = sender
        .chunk_envelopes(&loaded_checkpoint, &cipher)
        .expect("remaining envelopes");
    assert_eq!(
        remaining.len(),
        sender.plan().chunks.len() - loaded_checkpoint.completed_chunks.len()
    );

    {
        let mut storage = file_storage(&database);
        let mut receiver = TransferPipelineReceiver::new(
            &output,
            session_id(),
            transfer_id(),
            receiver_peer(),
            sender_peer(),
            loaded_checkpoint.clone(),
        );
        let (_sender_connection, _receiver_connection, mut sender_stream, mut receiver_stream) =
            connected_streams();

        exchange_request_and_acceptance(
            &sender,
            &mut receiver,
            &mut sender_stream,
            &mut receiver_stream,
        );

        for envelope in remaining {
            sender_stream.send_message(envelope).expect("send chunk");
            let received = receiver_stream.receive_message().expect("receive chunk");
            checkpoint = receiver
                .receive_chunk(received, &cipher)
                .expect("verified chunk");
            storage
                .save_checkpoint(&checkpoint)
                .expect("save resumed checkpoint");
            storage
                .save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))
                .expect("save resumed metadata");
        }

        receiver.verify_complete().expect("verify complete");
        storage
            .save_session(receiver.session())
            .expect("save completed session");
    }

    assert_eq!(fs::read(&output).expect("received file"), data);
    assert_eq!(
        sha256_file(&output).expect("received hash"),
        sha256_file(&source).expect("source hash")
    );

    let storage = file_storage(&database);
    assert_eq!(
        storage
            .load_checkpoint(&transfer_id())
            .expect("load final checkpoint")
            .expect("checkpoint exists")
            .completed_chunks
            .len(),
        sender.plan().chunks.len()
    );
    assert_eq!(
        storage
            .load_session(&session_id())
            .expect("load final session")
            .expect("session exists")
            .state,
        SessionState::Completed
    );
}

#[test]
fn checkpoint_reconciliation_keeps_only_verified_destination_chunks() {
    let workspace = TempWorkspace::new("reconcile-checkpoint");
    let source = workspace.path("source.bin");
    let output = workspace.path("received.bin");
    let data = b"first-oksecond-badthird";
    write_file(&source, data);
    write_file(&output, b"first-okcorrupted!");

    let sender = TransferPipelineSender::prepare(&source, config(8)).expect("sender pipeline");
    let checkpoint = Checkpoint {
        transfer_id: transfer_id(),
        completed_chunks: vec![ChunkId(0), ChunkId(1), ChunkId(99), ChunkId(0)],
    };

    let reconciled = reconcile_checkpoint(&output, &checkpoint, &sender.plan().chunks)
        .expect("reconcile checkpoint");

    assert_eq!(reconciled.completed_chunks, vec![ChunkId(0)]);
}

fn exchange_request_and_acceptance(
    sender: &TransferPipelineSender,
    receiver: &mut TransferPipelineReceiver,
    sender_stream: &mut networking::LoopbackStream,
    receiver_stream: &mut networking::LoopbackStream,
) {
    sender_stream
        .send_message(sender.session_request_envelope())
        .expect("send transfer request");
    let request = receiver_stream
        .receive_message()
        .expect("receive transfer request");
    let acceptance = receiver
        .accept_transfer(request, &sender.plan().chunks)
        .expect("accept transfer");
    receiver_stream
        .send_message(acceptance)
        .expect("send acceptance");
    let response = sender_stream.receive_message().expect("receive acceptance");

    assert!(matches!(
        response.message,
        TransferMessage::Session(common::TransferSessionMessage::Response(
            common::TransferResponse::Accepted(_)
        ))
    ));
}

fn connected_streams() -> (
    networking::LoopbackConnection,
    networking::LoopbackConnection,
    networking::LoopbackStream,
    networking::LoopbackStream,
) {
    let (mut sender_provider, mut receiver_provider) =
        LoopbackTransportProvider::paired(sender_peer(), receiver_peer());
    let mut listener = receiver_provider.listen().expect("receiver listener");
    let mut sender_connection = sender_provider
        .connect(&receiver_peer(), session_id())
        .expect("sender connection");
    let mut receiver_connection = listener.accept().expect("receiver connection");
    let sender_stream = sender_connection.open_stream().expect("sender stream");
    let receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");

    (
        sender_connection,
        receiver_connection,
        sender_stream,
        receiver_stream,
    )
}

fn config(chunk_size: usize) -> TransferPipelineConfig {
    TransferPipelineConfig {
        session_id: session_id(),
        transfer_id: transfer_id(),
        sender_peer: sender_peer(),
        receiver_peer: receiver_peer(),
        chunk_size,
    }
}

fn sender_peer() -> PeerId {
    PeerId("sender".to_owned())
}

fn receiver_peer() -> PeerId {
    PeerId("receiver".to_owned())
}

fn session_id() -> SessionId {
    SessionId("session-1".to_owned())
}

fn transfer_id() -> TransferId {
    TransferId("transfer-1".to_owned())
}

fn in_memory_storage() -> PersistentStorage<SqliteStorageBackend> {
    PersistentStorage::new(SqliteStorageBackend::in_memory().expect("sqlite backend"))
        .expect("storage")
}

fn file_storage(path: &Path) -> PersistentStorage<SqliteStorageBackend> {
    PersistentStorage::new(SqliteStorageBackend::open(path).expect("sqlite backend"))
        .expect("storage")
}

fn session_cipher() -> ChunkCipher {
    let sender = EphemeralKeyPair::generate();
    let receiver = EphemeralKeyPair::generate();
    let receiver_public = receiver.public_key();
    let sender_key = sender.complete(&receiver_public).expect("session key");

    ChunkCipher {
        cipher: SessionCipher::new(sender_key),
    }
}

struct ChunkCipher {
    cipher: SessionCipher,
}

impl TransferPayloadCipher for ChunkCipher {
    fn encrypt_chunk(&self, chunk_id: &ChunkId, plaintext: &[u8]) -> PipelineResult<Vec<u8>> {
        self.cipher
            .encrypt(&nonce(chunk_id), aad(chunk_id).as_bytes(), plaintext)
            .map_err(|error| TransferPipelineError::Cipher(error.to_string()))
    }

    fn decrypt_chunk(&self, chunk_id: &ChunkId, ciphertext: &[u8]) -> PipelineResult<Vec<u8>> {
        self.cipher
            .decrypt(&nonce(chunk_id), aad(chunk_id).as_bytes(), ciphertext)
            .map_err(|error| TransferPipelineError::Cipher(error.to_string()))
    }
}

fn nonce(chunk_id: &ChunkId) -> [u8; crypto::NONCE_LEN] {
    let mut nonce = [0u8; crypto::NONCE_LEN];
    nonce[..4].copy_from_slice(b"nexo");
    nonce[4..].copy_from_slice(&chunk_id.0.to_be_bytes());
    nonce
}

fn aad(chunk_id: &ChunkId) -> String {
    format!("{}:{}:{}", session_id().0, transfer_id().0, chunk_id.0)
}

fn write_file(path: &Path, data: &[u8]) {
    let mut file = File::create(path).expect("create file");
    file.write_all(data).expect("write file");
}

struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "nexo-transfer-pipeline-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp workspace");

        Self { path }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).ok();
    }
}
