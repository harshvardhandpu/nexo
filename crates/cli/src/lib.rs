use clap::{Parser, Subcommand};
use common::{
    Checkpoint, ChunkId, ChunkMetadata, MessageEnvelope, MissingChunks, PeerId, SessionId,
    SessionState, TransferChunkMessage, TransferControlMessage, TransferId, TransferMessage,
    TransferRequest, TransferResponse, TransferSessionMessage, TransferVerificationMessage,
};
use crypto::{Encryptor, EphemeralKeyPair, KeyExchange, PublicKeyBytes, SessionCipher};
use engine::chunker::{default_chunk_size, generate_manifest, missing_chunks};
use engine::pipeline::{
    PipelineResult, TransferPayloadCipher, TransferPipelineConfig, TransferPipelineError,
    TransferPipelineReceiver, TransferPipelineSender, reconcile_checkpoint, transition_session,
};
use networking::{
    LocalDiscoveryProvider, PeerAdvertisement, PeerDiscovery, PeerInfo, QuicTransportProvider,
    TransportConnection, TransportListener, TransportProvider, TransportStream,
};
use rand_core::{OsRng, RngCore};
use std::collections::HashSet;
use std::error::Error;
use std::fs::{self, File};
use std::io::{Error as IoError, ErrorKind, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use storage::{
    CheckpointStore, ResumeMetadataStore, SessionStore, SqliteStorageBackend,
    Storage as PersistentStorage,
};

pub type CliResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const RECEIVER_PEER_FILE: &str = "receiver.peer";
const LATEST_TRANSFER_FILE: &str = "latest-transfer";
const STATE_DATABASE_FILE: &str = "state.sqlite";
const PEER_ID_FILE: &str = "peer-id";
const SENDER_PEER: &str = "cli-sender";
const RECEIVER_PEER: &str = "cli-receiver";
const DISCOVERY_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, Parser)]
#[command(name = "nexo")]
#[command(about = "Nexo command-line file transfer")]
pub struct CliArgs {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    Discover,
    Receive,
    Send {
        file: PathBuf,
        #[arg(long)]
        host: Option<SocketAddr>,
    },
    Status,
}

#[derive(Debug, Clone)]
pub struct CliConfig {
    pub state_dir: PathBuf,
    pub receive_dir: PathBuf,
    pub chunk_size: usize,
}

impl CliConfig {
    pub fn from_environment() -> CliResult<Self> {
        let state_dir = match std::env::var_os("NEXO_HOME") {
            Some(path) => PathBuf::from(path),
            None => match std::env::var_os("HOME") {
                Some(home) => PathBuf::from(home).join(".nexo"),
                None => std::env::current_dir()?.join(".nexo"),
            },
        };

        Ok(Self {
            state_dir,
            receive_dir: std::env::current_dir()?,
            chunk_size: default_chunk_size() as usize,
        })
    }

    fn database_path(&self) -> PathBuf {
        self.state_dir.join(STATE_DATABASE_FILE)
    }

    fn receiver_peer_path(&self) -> PathBuf {
        self.state_dir.join(RECEIVER_PEER_FILE)
    }

    fn latest_transfer_path(&self) -> PathBuf {
        self.state_dir.join(LATEST_TRANSFER_FILE)
    }

    fn peer_id_path(&self) -> PathBuf {
        self.state_dir.join(PEER_ID_FILE)
    }
}

pub fn main_entry() -> CliResult<()> {
    let args = CliArgs::parse();
    let config = CliConfig::from_environment()?;
    run_cli(args, &config, &mut std::io::stdout())
}

pub fn run_cli<W: Write>(args: CliArgs, config: &CliConfig, output: &mut W) -> CliResult<()> {
    match args.command {
        CliCommand::Discover => run_discover(config, output),
        CliCommand::Receive => run_receive(config, output),
        CliCommand::Send { file, host } => run_send(&file, host, config, output),
        CliCommand::Status => run_status(config, output),
    }
}

pub fn run_discover<W: Write>(config: &CliConfig, output: &mut W) -> CliResult<()> {
    run_discover_for(config, output, DISCOVERY_DURATION)
}

fn run_discover_for<W: Write>(
    config: &CliConfig,
    output: &mut W,
    duration: Duration,
) -> CliResult<()> {
    fs::create_dir_all(&config.state_dir)?;
    let peer_id = load_or_create_peer_id(config)?;
    let display_name = local_display_name(&peer_id)?;
    let mut discovery =
        LocalDiscoveryProvider::new(PeerAdvertisement::new(peer_id, display_name, 0))?;
    let deadline = Instant::now()
        .checked_add(duration)
        .unwrap_or_else(Instant::now);

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        discovery.next_event(remaining)?;
    }

    let peers = discovery.peers();
    discovery.shutdown()?;
    write_discovered_peers(&peers, output)?;

    Ok(())
}

fn write_discovered_peers<W: Write>(peers: &[PeerInfo], output: &mut W) -> CliResult<()> {
    writeln!(output, "Found peers:")?;
    if peers.is_empty() {
        writeln!(output, "(none)")?;
    } else {
        for peer in peers {
            writeln!(output, "* {}", peer.display_name)?;
        }
    }

    Ok(())
}

pub fn run_cli_from<I, T, W>(args: I, config: &CliConfig, output: &mut W) -> CliResult<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
    W: Write,
{
    let args = CliArgs::try_parse_from(args)?;
    run_cli(args, config, output)
}

pub fn run_receive<W: Write>(config: &CliConfig, output: &mut W) -> CliResult<()> {
    fs::create_dir_all(&config.state_dir)?;
    fs::create_dir_all(&config.receive_dir)?;

    let mut storage = storage(config)?;
    let mut provider = QuicTransportProvider::localhost(receiver_peer())?;
    let mut listener = provider.listen()?;
    let advert = ReceiverAdvert {
        address: listener.local_addr(),
        certificate_der: listener.certificate_der().to_vec(),
    };
    save_receiver_advert(config, &advert)?;
    writeln!(output, "receiving on {}", advert.address)?;

    let mut connection = listener.accept()?;
    let mut stream = connection.accept_stream()?;
    let request_envelope = stream.receive_message()?;
    let request = request_from_envelope(&request_envelope)?;
    let metadata = receive_chunk_metadata(&mut stream, &request)?;
    let output_path = config.receive_dir.join(&request.manifest.name);
    let mut checkpoint = load_receive_checkpoint(&storage, &request, &metadata, &output_path)?;

    if checkpoint.completed_chunks.is_empty() {
        File::create(&output_path)?;
    }
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&output_path)?
        .set_len(request.manifest.size)?;

    let mut receiver = TransferPipelineReceiver::new(
        &output_path,
        request.session_id.clone(),
        request.transfer_id.clone(),
        receiver_peer(),
        sender_peer(),
        checkpoint.clone(),
    );
    let acceptance = receiver.accept_transfer(request_envelope, &metadata)?;
    storage.save_session(receiver.session())?;
    storage.save_checkpoint(&checkpoint)?;
    save_resume_state(&mut storage, &request, checkpoint.clone())?;
    save_latest(config, &request.transfer_id, &request.session_id)?;
    stream.send_message(acceptance)?;

    let cipher = receive_key_exchange(&mut stream, &request)?;
    let missing = missing_chunks(
        request.transfer_id.clone(),
        &request.manifest,
        receiver.checkpoint(),
    );
    stream.send_message(MessageEnvelope {
        session_id: request.session_id.clone(),
        transfer_id: request.transfer_id.clone(),
        message: TransferMessage::Chunk(TransferChunkMessage::Missing(missing)),
    })?;
    print_progress("receiving", &checkpoint, &metadata, &request, output)?;

    while receiver.checkpoint().completed_chunks.len() < request.manifest.total_chunks as usize {
        let envelope = stream.receive_message()?;
        checkpoint = receiver.receive_chunk(envelope, &cipher)?;
        storage.save_checkpoint(&checkpoint)?;
        save_resume_state(&mut storage, &request, checkpoint.clone())?;
        storage.save_session(receiver.session())?;
        print_progress("received", &checkpoint, &metadata, &request, output)?;
    }

    let verified = receiver.verify_complete()?;
    storage.save_session(receiver.session())?;
    stream.send_message(verified)?;
    ensure_acknowledged(stream.receive_message()?, &request.transfer_id)?;
    stream.close()?;
    print_progress(
        "completed",
        receiver.checkpoint(),
        &metadata,
        &request,
        output,
    )?;

    Ok(())
}

pub fn run_send<W: Write>(
    file: &Path,
    host: Option<SocketAddr>,
    config: &CliConfig,
    output: &mut W,
) -> CliResult<()> {
    fs::create_dir_all(&config.state_dir)?;

    let mut storage = storage(config)?;
    let advert = load_receiver_advert(config)?;
    let address = host.unwrap_or(advert.address);
    if address != advert.address {
        return Err(io_error(
            ErrorKind::NotFound,
            format!("no trusted receiver certificate is stored for {address}"),
        ));
    }

    let manifest = generate_manifest(file, config.chunk_size)?;
    let transfer_id = TransferId(format!("transfer-{}", manifest.sha256));
    let session_id = SessionId(format!("session-{}", transfer_id.0));
    let sender = TransferPipelineSender::prepare(
        file,
        TransferPipelineConfig {
            session_id: session_id.clone(),
            transfer_id: transfer_id.clone(),
            sender_peer: sender_peer(),
            receiver_peer: receiver_peer(),
            chunk_size: config.chunk_size,
        },
    )?;
    let mut session = sender.plan().session.clone();
    let mut provider = QuicTransportProvider::localhost(sender_peer())?;

    provider.register_peer(receiver_peer(), address, advert.certificate_der);
    transition_session(&mut session, SessionState::Connecting)?;
    storage.save_session(&session)?;
    save_latest(config, &transfer_id, &session_id)?;

    let mut connection = provider.connect(&receiver_peer(), session_id.clone())?;
    let mut stream = connection.open_stream()?;
    stream.send_message(sender.session_request_envelope())?;
    for metadata in &sender.plan().chunks {
        stream.send_message(metadata_envelope(
            &session_id,
            &transfer_id,
            metadata.clone(),
        ))?;
    }

    let acceptance = stream.receive_message()?;
    ensure_acceptance(&acceptance)?;
    transition_session(&mut session, SessionState::PendingAcceptance)?;
    transition_session(&mut session, SessionState::Accepted)?;
    storage.save_session(&session)?;

    let cipher = send_key_exchange(&mut stream, &session_id, &transfer_id)?;
    let missing = missing_from_envelope(stream.receive_message()?, &transfer_id)?;
    let checkpoint =
        checkpoint_from_missing(&transfer_id, sender.plan().manifest.total_chunks, &missing);
    storage.save_checkpoint(&checkpoint)?;
    storage.save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))?;
    transition_session(&mut session, SessionState::Transferring)?;
    storage.save_session(&session)?;

    let mut progress_checkpoint = checkpoint.clone();
    print_progress(
        "sending",
        &progress_checkpoint,
        &sender.plan().chunks,
        &sender.plan().request,
        output,
    )?;
    for chunk_id in &missing.chunks {
        let envelope = sender.chunk_envelope(chunk_id, &cipher)?;
        stream.send_message(envelope)?;
        if !progress_checkpoint
            .completed_chunks
            .iter()
            .any(|completed| completed == chunk_id)
        {
            progress_checkpoint.completed_chunks.push(chunk_id.clone());
            progress_checkpoint
                .completed_chunks
                .sort_by_key(|completed| completed.0);
        }
        storage.save_checkpoint(&progress_checkpoint)?;
        storage
            .save_resume_metadata(&sender.plan().resume_metadata(progress_checkpoint.clone()))?;
        print_progress(
            "sent",
            &progress_checkpoint,
            &sender.plan().chunks,
            &sender.plan().request,
            output,
        )?;
    }

    let verification = stream.receive_message()?;
    ensure_file_verified(&verification)?;
    stream.send_message(acknowledged_envelope(&session_id, &transfer_id))?;
    stream.close()?;
    sender.complete_session(&mut session)?;
    storage.save_session(&session)?;
    storage.save_resume_metadata(
        &sender.plan().resume_metadata(Checkpoint {
            transfer_id: transfer_id.clone(),
            completed_chunks: (0..sender.plan().manifest.total_chunks)
                .map(ChunkId)
                .collect(),
        }),
    )?;
    writeln!(output, "transfer complete: {}", file.display())?;

    Ok(())
}

pub fn run_status<W: Write>(config: &CliConfig, output: &mut W) -> CliResult<()> {
    let latest = match load_latest(config)? {
        Some(latest) => latest,
        None => {
            writeln!(output, "No transfers recorded")?;
            return Ok(());
        }
    };
    let storage = storage(config)?;
    let session = storage.load_session(&latest.session_id)?;
    let metadata = storage.load_resume_metadata(&latest.transfer_id)?;

    writeln!(output, "Transfer: {}", latest.transfer_id.0)?;
    if let Some(session) = session {
        writeln!(output, "Session: {}", session.session_id.0)?;
        writeln!(output, "State: {:?}", session.state)?;
    }

    if let Some(metadata) = metadata {
        let completed_chunks = metadata.checkpoint.completed_chunks.len() as u64;
        let completed_bytes =
            completed_bytes_from_manifest(&metadata.manifest, &metadata.checkpoint);

        writeln!(output, "File: {}", metadata.manifest.name)?;
        writeln!(
            output,
            "Chunks: {completed_chunks}/{}",
            metadata.manifest.total_chunks
        )?;
        writeln!(
            output,
            "Bytes: {completed_bytes}/{}",
            metadata.manifest.size
        )?;
    } else {
        writeln!(output, "No resume metadata recorded")?;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceiverAdvert {
    address: SocketAddr,
    certificate_der: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LatestTransfer {
    transfer_id: TransferId,
    session_id: SessionId,
}

struct CliChunkCipher {
    cipher: SessionCipher,
    session_id: SessionId,
    transfer_id: TransferId,
}

impl TransferPayloadCipher for CliChunkCipher {
    fn encrypt_chunk(&self, chunk_id: &ChunkId, plaintext: &[u8]) -> PipelineResult<Vec<u8>> {
        self.cipher
            .encrypt(&nonce(chunk_id), self.aad(chunk_id).as_bytes(), plaintext)
            .map_err(|error| TransferPipelineError::Cipher(error.to_string()))
    }

    fn decrypt_chunk(&self, chunk_id: &ChunkId, ciphertext: &[u8]) -> PipelineResult<Vec<u8>> {
        self.cipher
            .decrypt(&nonce(chunk_id), self.aad(chunk_id).as_bytes(), ciphertext)
            .map_err(|error| TransferPipelineError::Cipher(error.to_string()))
    }
}

impl CliChunkCipher {
    fn new(cipher: SessionCipher, session_id: SessionId, transfer_id: TransferId) -> Self {
        Self {
            cipher,
            session_id,
            transfer_id,
        }
    }

    fn aad(&self, chunk_id: &ChunkId) -> String {
        format!(
            "{}:{}:{}",
            self.session_id.0, self.transfer_id.0, chunk_id.0
        )
    }
}

fn storage(config: &CliConfig) -> CliResult<PersistentStorage<SqliteStorageBackend>> {
    fs::create_dir_all(&config.state_dir)?;
    Ok(PersistentStorage::new(SqliteStorageBackend::open(
        config.database_path(),
    )?)?)
}

fn load_or_create_peer_id(config: &CliConfig) -> CliResult<PeerId> {
    fs::create_dir_all(&config.state_dir)?;
    let path = config.peer_id_path();
    if path.exists() {
        let value = fs::read_to_string(path)?;
        let value = value.trim();
        let valid = value.strip_prefix("peer-").is_some_and(|suffix| {
            suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if !valid {
            return Err(io_error(
                ErrorKind::InvalidData,
                "stored peer ID is invalid",
            ));
        }
        return Ok(PeerId(value.to_owned()));
    }

    let mut random = [0u8; 16];
    OsRng.fill_bytes(&mut random);
    let peer_id = PeerId(format!("peer-{}", hex_encode(&random)));
    fs::write(path, format!("{}\n", peer_id.0))?;
    Ok(peer_id)
}

fn local_display_name(peer_id: &PeerId) -> CliResult<String> {
    let hostname = hostname::get()?;
    let hostname = hostname.to_string_lossy();
    let hostname = hostname.trim();
    if hostname.is_empty() {
        return Ok(format!("Nexo-{}", &peer_id.0[..8]));
    }

    Ok(hostname.chars().take(200).collect())
}

fn load_receive_checkpoint(
    storage: &PersistentStorage<SqliteStorageBackend>,
    request: &TransferRequest,
    chunks: &[ChunkMetadata],
    output_path: &Path,
) -> CliResult<Checkpoint> {
    let empty = || Checkpoint {
        transfer_id: request.transfer_id.clone(),
        completed_chunks: Vec::new(),
    };
    let Some(resume) = storage.load_resume_metadata(&request.transfer_id)? else {
        return Ok(empty());
    };

    if resume.transfer_id != request.transfer_id
        || resume.checkpoint.transfer_id != request.transfer_id
        || resume.manifest != request.manifest
    {
        return Ok(empty());
    }

    let checkpoint = storage
        .load_checkpoint(&request.transfer_id)?
        .unwrap_or(resume.checkpoint);
    if checkpoint.transfer_id != request.transfer_id {
        return Ok(empty());
    }

    Ok(reconcile_checkpoint(output_path, &checkpoint, chunks)?)
}

fn request_from_envelope(envelope: &MessageEnvelope) -> CliResult<TransferRequest> {
    match &envelope.message {
        TransferMessage::Session(TransferSessionMessage::Request(request)) => Ok(request.clone()),
        _ => Err(io_error(
            ErrorKind::InvalidData,
            "expected transfer request",
        )),
    }
}

fn receive_chunk_metadata<S: TransportStream>(
    stream: &mut S,
    request: &TransferRequest,
) -> CliResult<Vec<ChunkMetadata>> {
    let mut metadata = Vec::with_capacity(request.manifest.total_chunks as usize);
    for _ in 0..request.manifest.total_chunks {
        let envelope = stream.receive_message()?;
        match envelope.message {
            TransferMessage::Chunk(TransferChunkMessage::Metadata(chunk)) => metadata.push(chunk),
            _ => {
                return Err(io_error(
                    ErrorKind::InvalidData,
                    "expected chunk metadata message",
                ));
            }
        }
    }

    Ok(metadata)
}

fn metadata_envelope(
    session_id: &SessionId,
    transfer_id: &TransferId,
    metadata: ChunkMetadata,
) -> MessageEnvelope {
    MessageEnvelope {
        session_id: session_id.clone(),
        transfer_id: transfer_id.clone(),
        message: TransferMessage::Chunk(TransferChunkMessage::Metadata(metadata)),
    }
}

fn ensure_acceptance(envelope: &MessageEnvelope) -> CliResult<()> {
    match &envelope.message {
        TransferMessage::Session(TransferSessionMessage::Response(TransferResponse::Accepted(
            _,
        ))) => Ok(()),
        _ => Err(io_error(
            ErrorKind::InvalidData,
            "expected transfer acceptance",
        )),
    }
}

fn ensure_file_verified(envelope: &MessageEnvelope) -> CliResult<()> {
    match &envelope.message {
        TransferMessage::Verification(TransferVerificationMessage::FileVerified { .. }) => Ok(()),
        _ => Err(io_error(
            ErrorKind::InvalidData,
            "expected file verification",
        )),
    }
}

fn ensure_acknowledged(
    envelope: MessageEnvelope,
    expected_transfer_id: &TransferId,
) -> CliResult<()> {
    match envelope.message {
        TransferMessage::Control(TransferControlMessage::Acknowledged { transfer_id })
            if transfer_id == *expected_transfer_id =>
        {
            Ok(())
        }
        _ => Err(io_error(
            ErrorKind::InvalidData,
            "expected transfer acknowledgement",
        )),
    }
}

fn send_key_exchange<S: TransportStream>(
    stream: &mut S,
    session_id: &SessionId,
    transfer_id: &TransferId,
) -> CliResult<CliChunkCipher> {
    let local_key = EphemeralKeyPair::generate();
    let public_key = local_key.public_key();
    stream.send_message(key_exchange_envelope(session_id, transfer_id, &public_key))?;

    let peer_public_key = public_key_from_envelope(stream.receive_message()?, transfer_id)?;
    let session_key = local_key.complete(&peer_public_key)?;

    Ok(CliChunkCipher::new(
        SessionCipher::new(session_key),
        session_id.clone(),
        transfer_id.clone(),
    ))
}

fn receive_key_exchange<S: TransportStream>(
    stream: &mut S,
    request: &TransferRequest,
) -> CliResult<CliChunkCipher> {
    let peer_public_key =
        public_key_from_envelope(stream.receive_message()?, &request.transfer_id)?;
    let local_key = EphemeralKeyPair::generate();
    let public_key = local_key.public_key();
    stream.send_message(key_exchange_envelope(
        &request.session_id,
        &request.transfer_id,
        &public_key,
    ))?;
    let session_key = local_key.complete(&peer_public_key)?;

    Ok(CliChunkCipher::new(
        SessionCipher::new(session_key),
        request.session_id.clone(),
        request.transfer_id.clone(),
    ))
}

fn key_exchange_envelope(
    session_id: &SessionId,
    transfer_id: &TransferId,
    public_key: &PublicKeyBytes,
) -> MessageEnvelope {
    MessageEnvelope {
        session_id: session_id.clone(),
        transfer_id: transfer_id.clone(),
        message: TransferMessage::Control(TransferControlMessage::KeyExchange {
            transfer_id: transfer_id.clone(),
            public_key: public_key.as_bytes().to_vec(),
        }),
    }
}

fn acknowledged_envelope(session_id: &SessionId, transfer_id: &TransferId) -> MessageEnvelope {
    MessageEnvelope {
        session_id: session_id.clone(),
        transfer_id: transfer_id.clone(),
        message: TransferMessage::Control(TransferControlMessage::Acknowledged {
            transfer_id: transfer_id.clone(),
        }),
    }
}

fn public_key_from_envelope(
    envelope: MessageEnvelope,
    expected_transfer_id: &TransferId,
) -> CliResult<PublicKeyBytes> {
    match envelope.message {
        TransferMessage::Control(TransferControlMessage::KeyExchange {
            transfer_id,
            public_key,
        }) if transfer_id == *expected_transfer_id => {
            let bytes: [u8; 32] = public_key.try_into().map_err(|payload: Vec<u8>| {
                IoError::new(
                    ErrorKind::InvalidData,
                    format!("invalid public key length: {}", payload.len()),
                )
            })?;
            Ok(PublicKeyBytes::from_bytes(bytes))
        }
        _ => Err(io_error(ErrorKind::InvalidData, "expected key exchange")),
    }
}

fn missing_from_envelope(
    envelope: MessageEnvelope,
    expected_transfer_id: &TransferId,
) -> CliResult<MissingChunks> {
    match envelope.message {
        TransferMessage::Chunk(TransferChunkMessage::Missing(missing))
            if missing.transfer_id == *expected_transfer_id =>
        {
            Ok(missing)
        }
        _ => Err(io_error(ErrorKind::InvalidData, "expected missing chunks")),
    }
}

fn checkpoint_from_missing(
    transfer_id: &TransferId,
    total_chunks: u64,
    missing: &MissingChunks,
) -> Checkpoint {
    let missing = missing
        .chunks
        .iter()
        .map(|chunk| chunk.0)
        .collect::<HashSet<_>>();
    let completed_chunks = (0..total_chunks)
        .filter(|chunk_id| !missing.contains(chunk_id))
        .map(ChunkId)
        .collect();

    Checkpoint {
        transfer_id: transfer_id.clone(),
        completed_chunks,
    }
}

fn save_resume_state(
    storage: &mut PersistentStorage<SqliteStorageBackend>,
    request: &TransferRequest,
    checkpoint: Checkpoint,
) -> CliResult<()> {
    storage.save_resume_metadata(&common::ResumeMetadata {
        transfer_id: request.transfer_id.clone(),
        manifest: request.manifest.clone(),
        checkpoint,
    })?;

    Ok(())
}

fn save_receiver_advert(config: &CliConfig, advert: &ReceiverAdvert) -> CliResult<()> {
    fs::write(
        config.receiver_peer_path(),
        format!(
            "address={}\ncertificate={}\n",
            advert.address,
            hex_encode(&advert.certificate_der)
        ),
    )?;
    Ok(())
}

fn load_receiver_advert(config: &CliConfig) -> CliResult<ReceiverAdvert> {
    let contents = fs::read_to_string(config.receiver_peer_path()).map_err(|error| {
        IoError::new(
            error.kind(),
            format!("receiver is not advertised; run `nexo receive` first: {error}"),
        )
    })?;
    let address = read_field(&contents, "address")?.parse::<SocketAddr>()?;
    let certificate_der = hex_decode(read_field(&contents, "certificate")?)?;

    Ok(ReceiverAdvert {
        address,
        certificate_der,
    })
}

fn save_latest(
    config: &CliConfig,
    transfer_id: &TransferId,
    session_id: &SessionId,
) -> CliResult<()> {
    fs::write(
        config.latest_transfer_path(),
        format!(
            "transfer_id={}\nsession_id={}\n",
            transfer_id.0, session_id.0
        ),
    )?;
    Ok(())
}

fn load_latest(config: &CliConfig) -> CliResult<Option<LatestTransfer>> {
    let path = config.latest_transfer_path();
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(path)?;
    Ok(Some(LatestTransfer {
        transfer_id: TransferId(read_field(&contents, "transfer_id")?.to_owned()),
        session_id: SessionId(read_field(&contents, "session_id")?.to_owned()),
    }))
}

fn print_progress<W: Write>(
    label: &str,
    checkpoint: &Checkpoint,
    metadata: &[ChunkMetadata],
    request: &TransferRequest,
    output: &mut W,
) -> CliResult<()> {
    let completed_chunks = checkpoint.completed_chunks.len() as u64;
    let completed_bytes = completed_bytes_from_metadata(metadata, checkpoint);
    writeln!(
        output,
        "{label}: {completed_chunks}/{} chunks, {completed_bytes}/{} bytes",
        request.manifest.total_chunks, request.manifest.size
    )?;
    Ok(())
}

fn completed_bytes_from_metadata(metadata: &[ChunkMetadata], checkpoint: &Checkpoint) -> u64 {
    let completed = checkpoint
        .completed_chunks
        .iter()
        .collect::<HashSet<&ChunkId>>();

    metadata
        .iter()
        .filter(|chunk| completed.contains(&chunk.id))
        .map(|chunk| chunk.size)
        .sum()
}

fn completed_bytes_from_manifest(manifest: &common::FileManifest, checkpoint: &Checkpoint) -> u64 {
    if manifest.total_chunks == 0 {
        return 0;
    }

    checkpoint
        .completed_chunks
        .iter()
        .map(|chunk| {
            let offset = chunk.0 * manifest.chunk_size;
            manifest
                .size
                .saturating_sub(offset)
                .min(manifest.chunk_size)
        })
        .sum()
}

fn read_field<'a>(contents: &'a str, key: &str) -> CliResult<&'a str> {
    let prefix = format!("{key}=");
    contents
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .ok_or_else(|| io_error(ErrorKind::InvalidData, format!("missing field: {key}")))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> CliResult<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(io_error(
            ErrorKind::InvalidData,
            "hex value must contain an even number of characters",
        ));
    }

    let mut bytes = Vec::with_capacity(value.len() / 2);
    for index in (0..value.len()).step_by(2) {
        let byte = u8::from_str_radix(&value[index..index + 2], 16)?;
        bytes.push(byte);
    }

    Ok(bytes)
}

fn nonce(chunk_id: &ChunkId) -> [u8; crypto::NONCE_LEN] {
    let mut nonce = [0u8; crypto::NONCE_LEN];
    nonce[..4].copy_from_slice(b"nexo");
    nonce[4..].copy_from_slice(&chunk_id.0.to_be_bytes());
    nonce
}

fn sender_peer() -> PeerId {
    PeerId(SENDER_PEER.to_owned())
}

fn receiver_peer() -> PeerId {
    PeerId(RECEIVER_PEER.to_owned())
}

fn io_error<T: Into<String>>(kind: ErrorKind, message: T) -> Box<dyn Error + Send + Sync> {
    Box::new(IoError::new(kind, message.into()))
}

#[cfg(test)]
fn unique_id() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn parses_required_commands() {
        let discover = CliArgs::try_parse_from(["nexo", "discover"]).expect("discover parses");
        assert!(matches!(discover.command, CliCommand::Discover));
        run_cli_from(
            ["nexo", "status"],
            &test_config("parse-status"),
            &mut Vec::new(),
        )
        .expect("status parses");
        run_cli_from(
            ["nexo", "send", "file.bin", "--host", "127.0.0.1:4444"],
            &test_config("parse-send"),
            &mut Vec::new(),
        )
        .expect_err("send fails after parsing because no receiver is advertised");
    }

    #[test]
    fn status_reports_empty_state() {
        let config = test_config("empty-status");
        let mut output = Vec::new();

        run_status(&config, &mut output).expect("status");

        assert_eq!(
            String::from_utf8(output).expect("utf8"),
            "No transfers recorded\n"
        );
    }

    #[test]
    fn local_peer_identity_is_persisted() {
        let config = test_config("peer-identity");

        let first = load_or_create_peer_id(&config).expect("create peer ID");
        let second = load_or_create_peer_id(&config).expect("load peer ID");

        assert_eq!(first, second);
        assert_eq!(first.0.len(), 37);
        assert!(first.0.starts_with("peer-"));
    }

    #[test]
    fn discover_output_lists_peers() {
        let peers = vec![PeerInfo {
            peer_id: PeerId("advertised-peer".to_owned()),
            display_name: "Harsh-Laptop".to_owned(),
            addresses: vec![std::net::IpAddr::from([127, 0, 0, 1])],
            port: 43001,
        }];
        let mut output = Vec::new();

        write_discovered_peers(&peers, &mut output).expect("discover output");

        let output = String::from_utf8(output).expect("discover output");
        assert!(output.contains("Found peers:\n"));
        assert!(output.contains("* Harsh-Laptop\n"));
    }

    #[test]
    fn send_and_receive_transfer_file_over_quic() {
        let workspace = TempWorkspace::new("quic-cli");
        let source = workspace.path("source.txt");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, b"nexo cli moves bytes through quic").expect("source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let receive_config = config.clone();

        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive(&receive_config, &mut output)?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&config);
        let advert = load_receiver_advert(&config).expect("receiver advert");
        let mut send_output = Vec::new();

        let send_result = run_send(&source, Some(advert.address), &config, &mut send_output);
        let receive_result = receiver.join().expect("receiver thread");
        if let Err(error) = send_result {
            panic!("send failed: {error}; receiver result: {receive_result:?}");
        }
        let receive_output = receive_result.expect("receive output");

        assert_eq!(
            fs::read(receive_dir.join("source.txt")).expect("received"),
            b"nexo cli moves bytes through quic"
        );
        assert!(
            String::from_utf8(send_output)
                .expect("send utf8")
                .contains("transfer complete")
        );
        assert!(receive_output.contains("completed"));

        let mut status = Vec::new();
        run_status(&config, &mut status).expect("status");
        let status = String::from_utf8(status).expect("status utf8");
        assert!(status.contains("State: Completed"));
        assert!(status.contains("Chunks: 5/5"));
    }

    #[test]
    fn send_and_receive_resumes_from_verified_receiver_checkpoint() {
        let workspace = TempWorkspace::new("quic-cli-resume");
        let source = workspace.path("source.txt");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        let contents = b"nexo resumes verified chunks over quic transport";
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, contents).expect("source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };

        let manifest = generate_manifest(&source, config.chunk_size).expect("manifest");
        let transfer_id = TransferId(format!("transfer-{}", manifest.sha256));
        let session_id = SessionId(format!("session-{}", transfer_id.0));
        let sender = TransferPipelineSender::prepare(
            &source,
            TransferPipelineConfig {
                session_id,
                transfer_id: transfer_id.clone(),
                sender_peer: sender_peer(),
                receiver_peer: receiver_peer(),
                chunk_size: config.chunk_size,
            },
        )
        .expect("sender pipeline");
        let checkpoint = Checkpoint {
            transfer_id,
            completed_chunks: vec![ChunkId(0), ChunkId(1)],
        };
        fs::write(
            receive_dir.join("source.txt"),
            &contents[..config.chunk_size * 2],
        )
        .expect("partial destination");
        {
            let mut storage = storage(&config).expect("storage");
            storage
                .save_checkpoint(&checkpoint)
                .expect("save checkpoint");
            storage
                .save_resume_metadata(&sender.plan().resume_metadata(checkpoint.clone()))
                .expect("save resume metadata");
        }

        let expected_total = sender.plan().chunks.len();
        let expected_missing = expected_total - checkpoint.completed_chunks.len();
        let receive_config = config.clone();
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive(&receive_config, &mut output)?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&config);
        let mut send_output = Vec::new();

        run_send(&source, None, &config, &mut send_output).expect("send resumed transfer");
        let receive_output = receiver
            .join()
            .expect("receiver thread")
            .expect("receive resumed transfer");
        let send_output = String::from_utf8(send_output).expect("send utf8");

        assert_eq!(
            fs::read(receive_dir.join("source.txt")).expect("received"),
            contents
        );
        assert_eq!(
            send_output
                .lines()
                .filter(|line| line.starts_with("sent:"))
                .count(),
            expected_missing
        );
        assert!(receive_output.contains(&format!("receiving: 2/{expected_total} chunks")));
    }

    #[test]
    fn send_and_receive_empty_file_over_quic() {
        let workspace = TempWorkspace::new("quic-cli-empty");
        let source = workspace.path("empty.bin");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, []).expect("empty source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let receive_config = config.clone();

        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive(&receive_config, &mut output)?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&config);
        let mut send_output = Vec::new();

        run_send(&source, None, &config, &mut send_output).expect("send empty file");
        let receive_output = receiver
            .join()
            .expect("receiver thread")
            .expect("receive empty file");

        assert_eq!(
            fs::read(receive_dir.join("empty.bin")).expect("received"),
            Vec::<u8>::new()
        );
        assert!(receive_output.contains("completed: 0/0 chunks, 0/0 bytes"));
        assert!(
            String::from_utf8(send_output)
                .expect("send utf8")
                .contains("sending: 0/0 chunks, 0/0 bytes")
        );
    }

    fn wait_for_receiver_advert(config: &CliConfig) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if config.receiver_peer_path().exists() {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }

        panic!("receiver advert was not written");
    }

    fn test_config(label: &str) -> CliConfig {
        let workspace = TempWorkspace::new(label);
        let path = workspace.path.clone();
        std::mem::forget(workspace);
        CliConfig {
            state_dir: path.join("state"),
            receive_dir: path.join("received"),
            chunk_size: 8,
        }
    }

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "nexo-cli-{label}-{}-{}",
                std::process::id(),
                unique_id()
            ));
            fs::create_dir_all(&path).expect("workspace");
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
}
