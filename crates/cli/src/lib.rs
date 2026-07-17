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
    LocalDiscoveryProvider, PeerAdvertisement, PeerDiscovery, PeerInfo, QuicListener,
    QuicServerIdentity, QuicTransportProvider, ServiceAdvertisement, TransportConnection,
    TransportListener, TransportProvider, TransportStream,
};
use rand_core::{OsRng, RngCore};
use std::collections::HashSet;
use std::error::Error;
use std::fs::{self, File};
use std::io::{Error as IoError, ErrorKind, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use storage::{
    CheckpointStore, ResumeMetadataStore, SessionStore, SqliteStorageBackend,
    Storage as PersistentStorage,
};

pub type CliResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const RECEIVER_PEER_FILE: &str = "receiver.peer";
const RECEIVER_IDENTITY_FILE: &str = "receiver.identity";
const LATEST_TRANSFER_FILE: &str = "latest-transfer";
const STATE_DATABASE_FILE: &str = "state.sqlite";
const PEER_ID_FILE: &str = "peer-id";
const SENDER_PEER: &str = "cli-sender";
const RECEIVER_PEER: &str = "cli-receiver";
const DISCOVERY_DURATION: Duration = Duration::from_secs(3);

#[derive(Debug, Parser)]
#[command(name = "nexo")]
#[command(about = "Nexo command-line file transfer")]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct CliArgs {
    #[command(subcommand)]
    command: CliCommand,
}

/// The Nexo version string (from the crate's Cargo package version).
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    Discover,
    Receive {
        /// Skip the interactive "Accept incoming file?" confirmation and accept
        /// automatically. Intended for scripting/testing only; the default
        /// requires the receiver to confirm each incoming transfer.
        #[arg(long)]
        auto_accept: bool,
    },
    Send {
        file: PathBuf,
        #[arg(long)]
        host: Option<SocketAddr>,
        /// Skip the interactive "Device found" confirmation and send immediately.
        /// Intended for scripting/testing only; the default requires consent.
        #[arg(long)]
        auto_accept: bool,
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

    pub fn database_path(&self) -> PathBuf {
        self.state_dir.join(STATE_DATABASE_FILE)
    }

    pub fn receiver_peer_path(&self) -> PathBuf {
        self.state_dir.join(RECEIVER_PEER_FILE)
    }

    pub fn receiver_identity_path(&self) -> PathBuf {
        self.state_dir.join(RECEIVER_IDENTITY_FILE)
    }

    pub fn latest_transfer_path(&self) -> PathBuf {
        self.state_dir.join(LATEST_TRANSFER_FILE)
    }

    pub fn peer_id_path(&self) -> PathBuf {
        self.state_dir.join(PEER_ID_FILE)
    }

    pub fn app_paths(&self) -> CliStatePaths {
        CliStatePaths {
            state_dir: self.state_dir.clone(),
            receive_dir: self.receive_dir.clone(),
            database: self.database_path(),
            receiver_peer: self.receiver_peer_path(),
            latest_transfer: self.latest_transfer_path(),
            peer_id: self.peer_id_path(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliStatePaths {
    pub state_dir: PathBuf,
    pub receive_dir: PathBuf,
    pub database: PathBuf,
    pub receiver_peer: PathBuf,
    pub latest_transfer: PathBuf,
    pub peer_id: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer_id: String,
    pub display_name: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub fingerprint: Option<String>,
    /// The peer's advertised DER certificate, when it published one. Used by the
    /// desktop pairing flow to store a trusted peer's certificate for sending.
    pub certificate_der: Option<Vec<u8>>,
}

impl From<PeerInfo> for DiscoveredPeer {
    fn from(peer: PeerInfo) -> Self {
        Self {
            peer_id: peer.peer_id.0,
            display_name: peer.display_name,
            addresses: peer
                .addresses
                .into_iter()
                .map(|address| address.to_string())
                .collect(),
            port: peer.port,
            fingerprint: peer.fingerprint,
            certificate_der: peer.certificate_der,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverEndpoint {
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferStatusSnapshot {
    pub latest: Option<TransferStatusDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferStatusDetails {
    pub transfer_id: String,
    pub session_id: String,
    pub state: Option<String>,
    pub file_name: Option<String>,
    pub completed_chunks: u64,
    pub total_chunks: u64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

pub fn main_entry() -> CliResult<()> {
    let args = CliArgs::parse();
    let config = CliConfig::from_environment()?;
    run_cli(args, &config, &mut std::io::stdout())
}

pub fn run_cli<W: Write>(args: CliArgs, config: &CliConfig, output: &mut W) -> CliResult<()> {
    match args.command {
        CliCommand::Discover => run_discover(config, output),
        CliCommand::Receive { auto_accept } => run_receive_command(config, output, auto_accept),
        CliCommand::Send {
            file,
            host,
            auto_accept,
        } => run_send_gated(&file, host, auto_accept, config, output),
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
    let peers = discover_peers_for(config, duration)?;
    write_discovered_peers(&peers, output)?;

    Ok(())
}

pub fn discover_peers(config: &CliConfig) -> CliResult<Vec<DiscoveredPeer>> {
    discover_peers_for(config, DISCOVERY_DURATION)
}

pub fn discover_peers_for(
    config: &CliConfig,
    duration: Duration,
) -> CliResult<Vec<DiscoveredPeer>> {
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

    let peers = discovery
        .peers()
        .into_iter()
        .map(DiscoveredPeer::from)
        .collect();
    discovery.shutdown()?;

    Ok(peers)
}

fn write_discovered_peers<W: Write>(peers: &[DiscoveredPeer], output: &mut W) -> CliResult<()> {
    writeln!(output, "Found peers:")?;
    if peers.is_empty() {
        writeln!(output, "(none)")?;
    } else {
        for peer in peers {
            match peer.addresses.first() {
                Some(address) => writeln!(
                    output,
                    "* {} — {}:{}",
                    peer.display_name, address, peer.port
                )?,
                None => writeln!(output, "* {}", peer.display_name)?,
            }
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

/// Lifecycle of an incoming transfer awaiting the receiver's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingRequestStatus {
    Pending,
    Accepted,
    Rejected,
    Cancelled,
}

/// An incoming transfer the receiver must approve before any data is written.
/// Built from the sender's request metadata; carries no file handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingTransferRequest {
    pub id: String,
    pub sender: String,
    pub filename: String,
    pub file_size: u64,
    pub checksum: String,
    pub timestamp: u64,
    pub status: IncomingRequestStatus,
}

impl IncomingTransferRequest {
    fn from_request(request: &TransferRequest) -> Self {
        Self {
            id: request.transfer_id.0.clone(),
            sender: request.from_peer.0.clone(),
            filename: request.manifest.name.clone(),
            file_size: request.manifest.size,
            checksum: request.manifest.sha256.clone(),
            timestamp: unix_timestamp(),
            status: IncomingRequestStatus::Pending,
        }
    }
}

fn unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Builds the protocol's existing rejection response for a request. Uses only
/// `common` message types — no new wire format.
fn rejection_envelope(request: &TransferRequest, reason: &str) -> MessageEnvelope {
    MessageEnvelope {
        session_id: request.session_id.clone(),
        transfer_id: request.transfer_id.clone(),
        message: TransferMessage::Session(TransferSessionMessage::Response(
            TransferResponse::Rejected(common::TransferRejection {
                session_id: request.session_id.clone(),
                reason: reason.to_owned(),
            }),
        )),
    }
}

/// Receiver executor: accepts and completes one incoming transfer. Auto-accepts
/// (no prompt) — it is the unchanged post-approval path, analogous to how
/// `run_send` is the executor behind `run_send_gated`. Existing callers/tests
/// use this directly to drive an already-approved transfer.
pub fn run_receive<W: Write>(config: &CliConfig, output: &mut W) -> CliResult<()> {
    run_receive_gated(config, output, |_| true)
}

/// CLI `receive`: requires an interactive "Accept incoming file?" confirmation
/// by default; `--auto-accept` pre-approves for scripting/testing.
///
/// The receiver stays ready across successive transfers — it serves one file,
/// then loops back to accept the next connection, until the process is
/// interrupted (Ctrl-C). Previously it exited after a single transfer, so a
/// second `send` to the same receiver (and desktop stress runs) failed with
/// "connection lost".
fn run_receive_command<W: Write>(
    config: &CliConfig,
    output: &mut W,
    auto_accept: bool,
) -> CliResult<()> {
    if auto_accept {
        run_receive_loop(
            config,
            output,
            ReceiveOptions::default(),
            |request| {
                println!("auto-accept: accepting incoming {}", request.filename);
                true
            },
            || true,
        )
    } else {
        run_receive_loop(
            config,
            output,
            ReceiveOptions::default(),
            confirm_incoming_on_terminal,
            || true,
        )
    }
}

/// Blocking accept/reject prompt on the controlling terminal. Any non-`y`
/// answer (including EOF / non-interactive stdin) rejects, so a receiver never
/// accepts a transfer by accident.
fn confirm_incoming_on_terminal(request: &IncomingTransferRequest) -> bool {
    use std::io::Write as _;
    println!("\nIncoming transfer request:");
    println!("  Device: {}", request.sender);
    println!("  File:   {}", request.filename);
    println!("  Size:   {} bytes", request.file_size);
    print!("Accept? [y/N] ");
    if std::io::stdout().flush().is_err() {
        return false;
    }
    let mut answer = String::new();
    match std::io::stdin().read_line(&mut answer) {
        Ok(0) | Err(_) => false,
        Ok(_) => matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
    }
}

/// Consent-gated receive: reads the incoming transfer's metadata, asks the
/// supplied approver, and only continues into the unchanged receive path if it
/// returns `true`. On rejection it sends the protocol's existing
/// `TransferResponse::Rejected` and closes cleanly — crucially *before* any
/// output file is created, so a rejected transfer leaves nothing on disk.
/// Options that tune a gated receive session without touching the transfer
/// engine. All fields are orchestration-level policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiveOptions {
    /// Advertise the receiver over mDNS. When false the receiver still runs and
    /// accepts direct connections by address — only discovery is suppressed.
    pub discoverable: bool,
}

impl Default for ReceiveOptions {
    fn default() -> Self {
        Self { discoverable: true }
    }
}

pub fn run_receive_gated<W, F>(config: &CliConfig, output: &mut W, approve: F) -> CliResult<()>
where
    W: Write,
    F: FnOnce(&IncomingTransferRequest) -> bool,
{
    run_receive_gated_with(config, output, ReceiveOptions::default(), approve)
}

/// Consent-gated receive with explicit [`ReceiveOptions`]. See
/// [`run_receive_gated`] for the approval semantics.
pub fn run_receive_gated_with<W, F>(
    config: &CliConfig,
    output: &mut W,
    options: ReceiveOptions,
    approve: F,
) -> CliResult<()>
where
    W: Write,
    F: FnOnce(&IncomingTransferRequest) -> bool,
{
    let mut bound = bind_receiver(config, output, options)?;
    serve_one_transfer(
        config,
        &mut bound.storage,
        &mut bound.listener,
        output,
        approve,
    )
}

/// Continuously serves incoming transfers on a single bound listener until
/// `should_continue` returns false. This keeps a device "ready to receive" for
/// successive transfers instead of exiting after the first file — the behavior
/// the CLI `receive` command and the desktop background receiver need.
///
/// The one-time setup (bind, identity, mDNS advertisement) happens once; then
/// each iteration accepts one connection and runs a full transfer. A failure in
/// an individual transfer (a peer dropping mid-stream, a rejected file) is
/// reported and the loop continues to the next connection — one bad transfer
/// must never tear down the receiver. Only a fatal setup error aborts.
pub fn run_receive_loop<W, F, C>(
    config: &CliConfig,
    output: &mut W,
    options: ReceiveOptions,
    mut approve: F,
    mut should_continue: C,
) -> CliResult<()>
where
    W: Write,
    F: FnMut(&IncomingTransferRequest) -> bool,
    C: FnMut() -> bool,
{
    let mut bound = bind_receiver(config, output, options)?;

    while should_continue() {
        // `&mut approve` is `FnOnce` for this single call while remaining
        // reusable across iterations.
        match serve_one_transfer(
            config,
            &mut bound.storage,
            &mut bound.listener,
            output,
            &mut approve,
        ) {
            Ok(()) => {}
            Err(error) => {
                writeln!(output, "transfer error (receiver still listening): {error}")?;
            }
        }
    }

    Ok(())
}

/// A bound, advertising receiver: the storage handle, the accepting QUIC
/// listener, and the mDNS advertisement guard (held for as long as the receiver
/// runs; unregisters on drop).
struct BoundReceiver {
    storage: PersistentStorage<SqliteStorageBackend>,
    listener: QuicListener,
    _discovery: Option<ServiceAdvertisement>,
}

/// One-time receiver setup shared by [`run_receive_gated_with`] (single transfer)
/// and [`run_receive_loop`] (continuous): binds the listener on the stable port,
/// persists identity/advert, prints "receiving on …", and starts the mDNS
/// advertisement. No connection is accepted here.
fn bind_receiver<W: Write>(
    config: &CliConfig,
    output: &mut W,
    options: ReceiveOptions,
) -> CliResult<BoundReceiver> {
    fs::create_dir_all(&config.state_dir)?;
    fs::create_dir_all(&config.receive_dir)?;

    let storage = storage(config)?;
    let (identity, stable_port) = load_or_create_receiver_identity(config)?;
    let bind_addr = receiver_bind_addr(stable_port);
    let mut provider = QuicTransportProvider::new(receiver_peer(), bind_addr)?;
    let listener = provider.listen_with_identity(&identity)?;
    if stable_port.is_none() {
        save_receiver_identity(config, &identity, listener.local_addr().port())?;
    }
    let advertised_addr = receiver_advertised_addr(listener.local_addr())?;
    let advert = ReceiverAdvert {
        address: advertised_addr,
        certificate_der: listener.certificate_der().to_vec(),
    };
    save_receiver_advert(config, &advert)?;
    writeln!(output, "receiving on {}", advert.address)?;

    // Publish this receiver on the shared mDNS discovery layer so that
    // `discover` (CLI or desktop) actually finds it. Without this the receiver
    // was only written to the local `receiver.peer` file, which the UI reads for
    // its "Advertised" state but discovery never consults -- two different
    // sources of truth. Held for the whole receive session; unregisters on drop.
    // Best-effort: a machine without usable multicast can still receive via
    // `--host`, so a discovery failure must not abort the transfer.
    //
    // When `discoverable` is off the receiver still runs and accepts direct
    // connections by address; only the mDNS advertisement is suppressed.
    let _discovery = if options.discoverable {
        advertise_receiver(config, &advert, output)
    } else {
        writeln!(output, "discovery disabled: not advertising over mDNS")?;
        None
    };

    Ok(BoundReceiver {
        storage,
        listener,
        _discovery,
    })
}

/// Serves exactly one incoming transfer on an already-bound listener: accepts a
/// connection, runs the approval gate, and (if accepted) receives + verifies the
/// file. Extracted from [`run_receive_gated_with`] so a long-running receiver
/// can serve successive transfers on the same listener without re-binding or
/// re-advertising — the one-time setup (bind, identity, mDNS advert) stays with
/// the caller.
fn serve_one_transfer<W, F>(
    config: &CliConfig,
    storage: &mut PersistentStorage<SqliteStorageBackend>,
    listener: &mut QuicListener,
    output: &mut W,
    approve: F,
) -> CliResult<()>
where
    W: Write,
    F: FnOnce(&IncomingTransferRequest) -> bool,
{
    let mut connection = listener.accept()?;
    let mut stream = connection.accept_stream()?;
    let request_envelope = stream.receive_message()?;
    let request = request_from_envelope(&request_envelope)?;
    let metadata = receive_chunk_metadata(&mut stream, &request)?;

    // Receiver-side approval gate. We now know the sender, filename, size, and
    // whole-file checksum, but have created nothing on disk yet. If the user
    // rejects, reply with the existing rejection message and close cleanly — no
    // output file, no checkpoint, no session state. The QUIC keep-alive holds
    // the connection open while we wait for the decision.
    let incoming = IncomingTransferRequest::from_request(&request);
    writeln!(
        output,
        "incoming: {} ({} bytes) from {}",
        incoming.filename, incoming.file_size, incoming.sender
    )?;
    if !approve(&incoming) {
        stream.send_message(rejection_envelope(
            &request,
            "receiver rejected the transfer",
        ))?;
        stream.close().ok();
        writeln!(output, "rejected incoming transfer: {}", incoming.filename)?;
        return Ok(());
    }
    writeln!(output, "accepted incoming transfer: {}", incoming.filename)?;

    let output_path = config.receive_dir.join(&request.manifest.name);
    let mut checkpoint = load_receive_checkpoint(storage, &request, &metadata, &output_path)?;

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
    save_resume_state(storage, &request, checkpoint.clone())?;
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
        let completed_id = data_chunk_id(&envelope);
        checkpoint = receiver.receive_chunk(envelope, &cipher)?;
        // Persist only the one chunk just written (O(1)); the manifest/resume
        // metadata were saved once before the loop and `load_resume_metadata`
        // reads the checkpoint live from `checkpoint_chunks`, so incremental
        // appends keep the resume state fully current without rewriting it.
        if let Some(chunk_id) = completed_id {
            storage.append_completed_chunk(&request.transfer_id, chunk_id)?;
        }
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

/// AirDrop-mode transfer status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirdropRequestStatus {
    Pending,
    Approved,
    Rejected,
}

/// An intent to send a file to a specific discovered/known receiver, created
/// *before* any bytes move. A transfer only starts once this request is
/// explicitly approved (interactive terminal prompt, `--auto-accept`, or a UI
/// approve action), never automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AirdropRequest {
    pub id: String,
    pub file_path: PathBuf,
    pub file_name: String,
    pub file_size: u64,
    pub peer_display_name: String,
    pub peer_address: SocketAddr,
    pub status: AirdropRequestStatus,
}

/// How a send should obtain consent before transferring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendConsent {
    /// Ask the user on the interactive terminal (default AirDrop behavior).
    Interactive,
    /// Pre-approved (`--auto-accept`, or a UI that already confirmed). No
    /// automatic transfer path exists that does not pass through here.
    AutoAccept,
}

/// Resolves and trust-checks the destination for a send, then builds a pending
/// [`TransferRequest`]. This performs the *same* certificate-trust check as
/// [`run_send`] (an untrusted address is rejected here too), so confirmation is
/// only ever offered for a receiver whose certificate we already hold.
pub fn build_transfer_request(
    file: &Path,
    host: Option<SocketAddr>,
    config: &CliConfig,
) -> CliResult<AirdropRequest> {
    let advert = load_receiver_advert(config)?;
    let address = host.unwrap_or(advert.address);
    if address != advert.address {
        return Err(io_error(
            ErrorKind::NotFound,
            format!("no trusted receiver certificate is stored for {address}"),
        ));
    }
    if !file.is_file() {
        return Err(io_error(
            ErrorKind::NotFound,
            format!("file does not exist: {}", file.display()),
        ));
    }
    build_transfer_request_to_peer(file, address, &address.to_string())
}

/// Builds a pending transfer request for an explicit `address` and friendly
/// `display_name`, without consulting the local `receiver.peer` file. Used by
/// the desktop to send to a *trusted remote* device (whose certificate is held
/// in the trusted store, not in `receiver.peer`). Only assembles the
/// confirmation metadata — no bytes move until the request is approved.
pub fn build_transfer_request_to_peer(
    file: &Path,
    address: SocketAddr,
    display_name: &str,
) -> CliResult<AirdropRequest> {
    if !file.is_file() {
        return Err(io_error(
            ErrorKind::NotFound,
            format!("file does not exist: {}", file.display()),
        ));
    }
    let file_size = fs::metadata(file)?.len();
    let file_name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_owned();

    Ok(AirdropRequest {
        id: format!("req-{}", hex_encode(&random_request_id())),
        file_path: file.to_path_buf(),
        file_name,
        file_size,
        peer_display_name: display_name.to_owned(),
        peer_address: address,
        status: AirdropRequestStatus::Pending,
    })
}

/// Consent-gated send: the ONLY path the CLI uses to send. It builds a transfer
/// request, obtains explicit approval, and only then delegates to the unchanged
/// [`run_send`] executor. No approval => no transfer.
pub fn run_send_gated<W: Write>(
    file: &Path,
    host: Option<SocketAddr>,
    auto_accept: bool,
    config: &CliConfig,
    output: &mut W,
) -> CliResult<()> {
    let consent = if auto_accept {
        SendConsent::AutoAccept
    } else {
        SendConsent::Interactive
    };
    let request = build_transfer_request(file, host, config)?;
    write_transfer_request_prompt(&request, output)?;

    let approved = match consent {
        SendConsent::AutoAccept => {
            writeln!(output, "auto-accept: approved {}", request.id)?;
            true
        }
        SendConsent::Interactive => confirm_on_terminal(&request)?,
    };

    if !approved {
        writeln!(output, "transfer cancelled: {}", request.id)?;
        return Ok(());
    }

    writeln!(output, "transfer approved: {}", request.id)?;
    run_send(
        &request.file_path,
        Some(request.peer_address),
        config,
        output,
    )
}

/// Renders the mandatory "Device found" confirmation summary. Shared by the
/// terminal prompt and mirrored by the desktop modal.
fn write_transfer_request_prompt<W: Write>(
    request: &AirdropRequest,
    output: &mut W,
) -> CliResult<()> {
    writeln!(output, "Device found")?;
    writeln!(output, "  Device: {}", request.peer_display_name)?;
    writeln!(output, "  Endpoint: {}", request.peer_address)?;
    writeln!(
        output,
        "  File: {} ({} bytes)",
        request.file_name, request.file_size
    )?;
    Ok(())
}

/// Blocking y/n confirmation on the controlling terminal. Any non-`y` answer
/// (including EOF/non-interactive stdin) is treated as a rejection, so a
/// non-interactive `send` without `--auto-accept` never transfers by accident.
fn confirm_on_terminal(request: &AirdropRequest) -> CliResult<bool> {
    use std::io::Write as _;
    print!(
        "Send {} to {}? [y/N] ",
        request.file_name, request.peer_address
    );
    std::io::stdout().flush()?;
    let mut answer = String::new();
    let read = std::io::stdin().read_line(&mut answer)?;
    if read == 0 {
        return Ok(false);
    }
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn random_request_id() -> [u8; 8] {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

pub fn run_send<W: Write>(
    file: &Path,
    host: Option<SocketAddr>,
    config: &CliConfig,
    output: &mut W,
) -> CliResult<()> {
    // Legacy/local path: resolve the receiver certificate from this device's own
    // `receiver.peer` file. Only works when sending to the address that file
    // advertises (e.g. loopback self-tests). Desktop sends to a *trusted remote*
    // device go through `run_send_to_peer` with the peer's stored certificate.
    let advert = load_receiver_advert(config)?;
    let address = host.unwrap_or(advert.address);
    if address != advert.address {
        return Err(io_error(
            ErrorKind::NotFound,
            format!("no trusted receiver certificate is stored for {address}"),
        ));
    }

    run_send_with_cert(file, address, advert.certificate_der, config, output)
}

/// Sends `file` to an explicit `address` using a caller-supplied peer
/// certificate, bypassing the local `receiver.peer` file. This is how the
/// desktop sends to a *trusted* device: the certificate comes from the trusted
/// store (captured at pairing from the peer's advertised identity), and the
/// address is the peer's current (live-discovered) endpoint.
///
/// Uses the same unchanged transfer engine and networking API as `run_send`
/// (`register_peer` + `connect`) — only the source of `(address, certificate)`
/// differs.
pub fn run_send_to_peer<W: Write>(
    file: &Path,
    address: SocketAddr,
    certificate_der: Vec<u8>,
    config: &CliConfig,
    output: &mut W,
) -> CliResult<()> {
    run_send_with_cert(file, address, certificate_der, config, output)
}

fn run_send_with_cert<W: Write>(
    file: &Path,
    address: SocketAddr,
    certificate_der: Vec<u8>,
    config: &CliConfig,
    output: &mut W,
) -> CliResult<()> {
    fs::create_dir_all(&config.state_dir)?;

    let mut storage = storage(config)?;

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

    provider.register_peer(receiver_peer(), address, certificate_der);
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
        // Persist just this chunk (O(1)); resume metadata was written once above.
        storage.append_completed_chunk(&transfer_id, chunk_id.clone())?;
        if !progress_checkpoint
            .completed_chunks
            .iter()
            .any(|completed| completed == chunk_id)
        {
            progress_checkpoint.completed_chunks.push(chunk_id.clone());
        }
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
    let snapshot = transfer_status_snapshot(config)?;
    let latest = match snapshot.latest {
        Some(latest) => latest,
        None => {
            writeln!(output, "No transfers recorded")?;
            return Ok(());
        }
    };

    writeln!(output, "Transfer: {}", latest.transfer_id)?;
    writeln!(output, "Session: {}", latest.session_id)?;
    if let Some(state) = latest.state {
        writeln!(output, "State: {state}")?;
    }

    if let Some(file_name) = latest.file_name {
        writeln!(output, "File: {file_name}")?;
        writeln!(
            output,
            "Chunks: {}/{}",
            latest.completed_chunks, latest.total_chunks
        )?;
        writeln!(
            output,
            "Bytes: {}/{}",
            latest.completed_bytes, latest.total_bytes
        )?;
    } else {
        writeln!(output, "No resume metadata recorded")?;
    }

    Ok(())
}

pub fn receiver_endpoint(config: &CliConfig) -> CliResult<Option<ReceiverEndpoint>> {
    if !config.receiver_peer_path().exists() {
        return Ok(None);
    }

    let advert = load_receiver_advert(config)?;
    Ok(Some(ReceiverEndpoint {
        address: advert.address.to_string(),
    }))
}

/// The trusted receiver's advertised address and certificate DER, if one has
/// been recorded. Read-only accessor over the existing `receiver.peer` trust
/// anchor — used by the desktop layer to fingerprint a device without changing
/// the trust system.
pub fn receiver_advertisement(config: &CliConfig) -> CliResult<Option<(String, Vec<u8>)>> {
    if !config.receiver_peer_path().exists() {
        return Ok(None);
    }
    let advert = load_receiver_advert(config)?;
    Ok(Some((advert.address.to_string(), advert.certificate_der)))
}

pub fn transfer_status_snapshot(config: &CliConfig) -> CliResult<TransferStatusSnapshot> {
    let Some(latest) = load_latest(config)? else {
        return Ok(TransferStatusSnapshot { latest: None });
    };
    let storage = storage(config)?;
    let session = storage.load_session(&latest.session_id)?;
    let metadata = storage.load_resume_metadata(&latest.transfer_id)?;
    let (file_name, completed_chunks, total_chunks, completed_bytes, total_bytes) = match metadata {
        Some(metadata) => (
            Some(metadata.manifest.name.clone()),
            metadata.checkpoint.completed_chunks.len() as u64,
            metadata.manifest.total_chunks,
            completed_bytes_from_manifest(&metadata.manifest, &metadata.checkpoint),
            metadata.manifest.size,
        ),
        None => (None, 0, 0, 0, 0),
    };

    Ok(TransferStatusSnapshot {
        latest: Some(TransferStatusDetails {
            transfer_id: latest.transfer_id.0,
            session_id: latest.session_id.0,
            state: session.map(|session| format!("{:?}", session.state)),
            file_name,
            completed_chunks,
            total_chunks,
            completed_bytes,
            total_bytes,
        }),
    })
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

fn receiver_bind_addr(stable_port: Option<u16>) -> SocketAddr {
    SocketAddr::new(
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        stable_port.unwrap_or_default(),
    )
}

fn receiver_advertised_addr(bound_addr: SocketAddr) -> CliResult<SocketAddr> {
    Ok(receiver_advertised_addr_with_lan(
        bound_addr,
        preferred_lan_ipv4()?,
    ))
}

fn receiver_advertised_addr_with_lan(
    bound_addr: SocketAddr,
    lan_address: Option<Ipv4Addr>,
) -> SocketAddr {
    let address = lan_address.unwrap_or(Ipv4Addr::LOCALHOST);
    SocketAddr::new(IpAddr::V4(address), bound_addr.port())
}

/// This device's preferred LAN IPv4 address for peer discovery, if one exists.
/// Read-only helper for the desktop's identity preview; no engine interaction.
pub fn local_lan_address() -> Option<String> {
    preferred_lan_ipv4()
        .ok()
        .flatten()
        .map(|address| address.to_string())
}

fn preferred_lan_ipv4() -> CliResult<Option<Ipv4Addr>> {
    let interfaces = if_addrs::get_if_addrs()?;
    Ok(interfaces.into_iter().find_map(|interface| {
        if !interface.is_oper_up() || interface.is_loopback() || interface.is_p2p() {
            return None;
        }

        match interface.ip() {
            IpAddr::V4(address) if is_lan_advertisable_ipv4(address) => Some(address),
            _ => None,
        }
    }))
}

fn is_lan_advertisable_ipv4(address: Ipv4Addr) -> bool {
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_multicast()
        || address.is_documentation())
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

/// Discovery identity a receiver advertises itself under.
///
/// Derived from (but distinct from) this device's discovery peer id so that a
/// `discover` running on the *same* device does not filter the receiver out as
/// "self" (the discover self-advert uses the plain device id) and the mDNS
/// service instance name never collides with that self-advert — while staying
/// unique per device for multi-machine discovery.
fn receiver_discovery_peer_id(device_peer_id: &PeerId) -> PeerId {
    PeerId(format!("{}-recv", device_peer_id.0))
}

/// Grouped uppercase SHA-256 fingerprint of a certificate, advertised over mDNS
/// for pairing. Must match the desktop `certificate_fingerprint` format so the
/// value a pairing peer sees equals the one stored on trust.
pub fn certificate_fingerprint(certificate_der: &[u8]) -> String {
    let digest = engine::chunker::sha256_hex(certificate_der).to_uppercase();
    digest
        .as_bytes()
        .chunks(4)
        .take(8)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(":")
}

fn advertised_fingerprint(certificate_der: &[u8]) -> String {
    certificate_fingerprint(certificate_der)
}

/// Best-effort registration of this receiver on the local mDNS discovery layer.
///
/// Returns the live advertisement handle (kept for the receive session) or
/// `None` if discovery is unavailable — receiving must still work via `--host`,
/// so a failure here only warns.
fn advertise_receiver<W: Write>(
    config: &CliConfig,
    advert: &ReceiverAdvert,
    output: &mut W,
) -> Option<ServiceAdvertisement> {
    // A loopback-only endpoint is unreachable from any other machine, so there
    // is nothing to gain from announcing it on the LAN discovery layer. Skipping
    // it also keeps loopback integration tests free of mDNS daemons.
    if advert.address.ip().is_loopback() {
        return None;
    }

    let peer_id = match load_or_create_peer_id(config) {
        Ok(peer_id) => peer_id,
        Err(error) => {
            let _ = writeln!(output, "warning: skipping network advertisement: {error}");
            return None;
        }
    };
    let display_name = local_display_name(&peer_id).unwrap_or_else(|_| "Nexo".to_owned());
    // Publish the receiver's certificate fingerprint so a pairing peer can show
    // the user a verifiable fingerprint. Format matches the desktop's
    // `certificate_fingerprint` (grouped uppercase SHA-256 hex).
    let fingerprint = advertised_fingerprint(&advert.certificate_der);
    let advertisement = PeerAdvertisement::new(
        receiver_discovery_peer_id(&peer_id),
        display_name,
        advert.address.port(),
    )
    .with_fingerprint(fingerprint)
    // Publish the receiver's certificate so a pairing peer can store it and
    // later connect (the QUIC client pins this exact cert). Same public
    // certificate already written to `receiver.peer`.
    .with_certificate(advert.certificate_der.clone());

    match ServiceAdvertisement::register(advertisement) {
        Ok(handle) => Some(handle),
        Err(error) => {
            let _ = writeln!(
                output,
                "warning: receiver is not discoverable on this network: {error}"
            );
            None
        }
    }
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

/// Chunk id carried by a chunk-data envelope, if any. Used to persist exactly
/// the chunk just received without inspecting the full checkpoint.
fn data_chunk_id(envelope: &MessageEnvelope) -> Option<ChunkId> {
    match &envelope.message {
        TransferMessage::Chunk(TransferChunkMessage::Data(chunk)) => Some(chunk.id.clone()),
        _ => None,
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
        // The receiver declined via the existing protocol rejection message.
        // Surface it as a clear, distinct error rather than a generic protocol
        // mismatch, so the sender reports "rejected by receiver".
        TransferMessage::Session(TransferSessionMessage::Response(TransferResponse::Rejected(
            rejection,
        ))) => Err(io_error(
            ErrorKind::PermissionDenied,
            format!("transfer rejected by receiver: {}", rejection.reason),
        )),
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

fn load_or_create_receiver_identity(
    config: &CliConfig,
) -> CliResult<(QuicServerIdentity, Option<u16>)> {
    let path = config.receiver_identity_path();
    if path.exists() {
        let contents = fs::read_to_string(path)?;
        let certificate_der = hex_decode(read_field(&contents, "certificate")?)?;
        let private_key_der = hex_decode(read_field(&contents, "private_key")?)?;
        let port = read_field(&contents, "port")?
            .parse::<u16>()
            .map_err(|error| io_error(ErrorKind::InvalidData, format!("invalid port: {error}")))?;
        return Ok((
            QuicServerIdentity {
                certificate_der,
                private_key_der,
            },
            Some(port),
        ));
    }

    Ok((QuicTransportProvider::generate_server_identity()?, None))
}

fn save_receiver_identity(
    config: &CliConfig,
    identity: &QuicServerIdentity,
    port: u16,
) -> CliResult<()> {
    fs::write(
        config.receiver_identity_path(),
        format!(
            "certificate={}\nprivate_key={}\nport={}\n",
            hex_encode(&identity.certificate_der),
            hex_encode(&identity.private_key_der),
            port
        ),
    )?;
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
    fn app_paths_expose_state_files_for_desktop() {
        let config = test_config("app-paths");
        let paths = config.app_paths();

        assert_eq!(paths.state_dir, config.state_dir);
        assert_eq!(paths.receive_dir, config.receive_dir);
        assert_eq!(
            paths.database.file_name().and_then(|name| name.to_str()),
            Some(STATE_DATABASE_FILE)
        );
        assert_eq!(
            paths
                .receiver_peer
                .file_name()
                .and_then(|name| name.to_str()),
            Some(RECEIVER_PEER_FILE)
        );
        assert_eq!(
            paths
                .latest_transfer
                .file_name()
                .and_then(|name| name.to_str()),
            Some(LATEST_TRANSFER_FILE)
        );
        assert_eq!(
            paths.peer_id.file_name().and_then(|name| name.to_str()),
            Some(PEER_ID_FILE)
        );
    }

    #[test]
    fn transfer_status_snapshot_reports_no_latest_transfer() {
        let config = test_config("snapshot-empty");

        let snapshot = transfer_status_snapshot(&config).expect("status snapshot");

        assert_eq!(snapshot, TransferStatusSnapshot { latest: None });
    }

    #[test]
    fn receiver_endpoint_reads_advertisement_for_desktop() {
        let config = test_config("receiver-endpoint");
        let advert = ReceiverAdvert {
            address: "127.0.0.1:41000".parse().expect("address"),
            certificate_der: vec![1, 2, 3, 4],
        };
        fs::create_dir_all(&config.state_dir).expect("state dir");
        save_receiver_advert(&config, &advert).expect("save advert");

        let endpoint = receiver_endpoint(&config)
            .expect("receiver endpoint")
            .expect("advert exists");

        assert_eq!(
            endpoint,
            ReceiverEndpoint {
                address: "127.0.0.1:41000".to_owned(),
            }
        );
    }

    #[test]
    fn receiver_binds_all_ipv4_interfaces_for_lan_reachability() {
        assert_eq!(
            receiver_bind_addr(None),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
        );
        assert_eq!(
            receiver_bind_addr(Some(41000)),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 41000)
        );
    }

    #[test]
    fn receiver_advertisement_uses_lan_address_with_bound_port() {
        let bound = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 41000);

        let advertised =
            receiver_advertised_addr_with_lan(bound, Some(Ipv4Addr::new(192, 168, 1, 44)));

        assert_eq!(
            advertised,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 44)), 41000)
        );
    }

    #[test]
    fn receiver_advertisement_falls_back_to_loopback_when_no_lan_address_exists() {
        let bound = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 41000);

        let advertised = receiver_advertised_addr_with_lan(bound, None);

        assert_eq!(
            advertised,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 41000)
        );
    }

    #[test]
    fn lan_advertisement_rejects_non_reachable_ipv4_addresses() {
        assert!(is_lan_advertisable_ipv4(Ipv4Addr::new(10, 0, 0, 10)));
        assert!(is_lan_advertisable_ipv4(Ipv4Addr::new(172, 16, 0, 10)));
        assert!(is_lan_advertisable_ipv4(Ipv4Addr::new(192, 168, 1, 10)));

        assert!(!is_lan_advertisable_ipv4(Ipv4Addr::UNSPECIFIED));
        assert!(!is_lan_advertisable_ipv4(Ipv4Addr::LOCALHOST));
        assert!(!is_lan_advertisable_ipv4(Ipv4Addr::new(169, 254, 1, 10)));
        assert!(!is_lan_advertisable_ipv4(Ipv4Addr::BROADCAST));
        assert!(!is_lan_advertisable_ipv4(Ipv4Addr::new(224, 0, 0, 1)));
        assert!(!is_lan_advertisable_ipv4(Ipv4Addr::new(192, 0, 2, 1)));
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
        let peers = vec![DiscoveredPeer {
            peer_id: "advertised-peer".to_owned(),
            display_name: "Harsh-Laptop".to_owned(),
            addresses: vec!["127.0.0.1".to_owned()],
            port: 43001,
            fingerprint: None,
            certificate_der: None,
        }];
        let mut output = Vec::new();

        write_discovered_peers(&peers, &mut output).expect("discover output");

        let output = String::from_utf8(output).expect("discover output");
        assert!(output.contains("Found peers:\n"));
        // Endpoint is shown so it can be pasted into `send --host`.
        assert!(output.contains("* Harsh-Laptop — 127.0.0.1:43001\n"));
    }

    #[test]
    fn receiver_discovery_peer_id_is_device_unique_and_distinct_from_device() {
        let device = PeerId("peer-0123456789abcdef0123456789abcdef".to_owned());
        let receiver = receiver_discovery_peer_id(&device);

        // Distinct from the device id (so same-host discover does not self-filter
        // it) but derived from it (so it stays unique across machines).
        assert_ne!(receiver, device);
        assert_eq!(receiver.0, format!("{}-recv", device.0));

        let other = PeerId("peer-ffffffffffffffffffffffffffffffff".to_owned());
        assert_ne!(receiver_discovery_peer_id(&other), receiver);
    }

    #[test]
    fn build_transfer_request_rejects_untrusted_host() {
        // Consent must never be offered for a host we do not already trust:
        // build_transfer_request enforces the same cert-trust check as run_send.
        let config = test_config("airdrop-untrusted");
        fs::create_dir_all(&config.state_dir).expect("state dir");
        let advert = ReceiverAdvert {
            address: "127.0.0.1:41000".parse().expect("addr"),
            certificate_der: vec![1, 2, 3],
        };
        save_receiver_advert(&config, &advert).expect("advert");
        let file = config.state_dir.join("payload.bin");
        fs::write(&file, b"hello").expect("file");

        let error = build_transfer_request(
            &file,
            Some("127.0.0.1:49999".parse().expect("addr")),
            &config,
        )
        .expect_err("untrusted host must be rejected before any confirmation");
        assert!(
            error
                .to_string()
                .contains("no trusted receiver certificate")
        );
    }

    #[test]
    fn build_transfer_request_is_pending_with_file_and_peer_metadata() {
        let config = test_config("airdrop-pending");
        fs::create_dir_all(&config.state_dir).expect("state dir");
        let address: SocketAddr = "127.0.0.1:41234".parse().expect("addr");
        save_receiver_advert(
            &config,
            &ReceiverAdvert {
                address,
                certificate_der: vec![9, 9, 9],
            },
        )
        .expect("advert");
        let file = config.state_dir.join("movie.bin");
        fs::write(&file, vec![0u8; 2048]).expect("file");

        let request = build_transfer_request(&file, None, &config).expect("request");

        assert_eq!(request.status, AirdropRequestStatus::Pending);
        assert_eq!(request.peer_address, address);
        assert_eq!(request.file_name, "movie.bin");
        assert_eq!(request.file_size, 2048);
        assert!(request.id.starts_with("req-"));
    }

    #[test]
    fn send_without_auto_accept_and_no_terminal_does_not_transfer() {
        // The default (non-interactive stdin, no --auto-accept) must NOT start a
        // transfer: confirm_on_terminal sees EOF and treats it as a rejection.
        // This proves "no automatic transfers" even when the receiver is trusted.
        let workspace = TempWorkspace::new("airdrop-no-consent");
        let source = workspace.path("source.txt");
        let state_dir = workspace.path("state");
        fs::create_dir_all(&state_dir).expect("state dir");
        fs::write(&source, b"nexo airdrop consent gate").expect("source");
        let config = CliConfig {
            state_dir: state_dir.clone(),
            receive_dir: workspace.path("received"),
            chunk_size: 8,
        };
        // A trusted receiver advert exists, but nothing is listening: if a
        // transfer were (wrongly) attempted it would try to connect and block or
        // error; a correct rejection returns Ok without connecting.
        save_receiver_advert(
            &config,
            &ReceiverAdvert {
                address: "127.0.0.1:1".parse().expect("addr"),
                certificate_der: vec![1],
            },
        )
        .expect("advert");

        let mut output = Vec::new();
        run_send_gated(&source, None, false, &config, &mut output).expect("gated send returns");
        let text = String::from_utf8(output).expect("utf8");

        assert!(
            text.contains("Device found"),
            "must show confirmation: {text}"
        );
        assert!(text.contains("transfer cancelled"), "must cancel: {text}");
        assert!(
            !text.contains("transfer complete"),
            "must not transfer: {text}"
        );
    }

    #[test]
    fn receiver_rejection_cancels_transfer_and_writes_no_file() {
        // Reject flow: the receiver declines, the sender gets a clear rejection
        // error, and no output file is created.
        let workspace = TempWorkspace::new("quic-cli-reject");
        let source = workspace.path("secret.txt");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, b"do not accept this file").expect("source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let receive_config = config.clone();

        // Receiver rejects (approver returns false).
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive_gated(&receive_config, &mut output, |_request| false)?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&config);
        let advert = load_receiver_advert(&config).expect("receiver advert");
        let mut send_output = Vec::new();

        let send_result = run_send_gated(
            &source,
            Some(advert.address),
            true,
            &config,
            &mut send_output,
        );
        let receive_output = receiver
            .join()
            .expect("receiver thread")
            .expect("receive output");

        // Sender saw a rejection, not a generic failure.
        let error = send_result.expect_err("send must fail when rejected");
        assert!(
            error.to_string().contains("rejected by receiver"),
            "unexpected error: {error}"
        );
        // Receiver reported the rejection and created NO output file.
        assert!(receive_output.contains("rejected incoming transfer"));
        assert!(
            !receive_dir.join("secret.txt").exists(),
            "no file may be created on rejection"
        );
    }

    #[test]
    fn receiver_gated_accept_completes_transfer() {
        // Accept flow through the gate: approver returns true, transfer completes
        // and the received bytes match the source exactly.
        let workspace = TempWorkspace::new("quic-cli-accept");
        let source = workspace.path("ok.txt");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        let contents = b"nexo receiver approval accept path";
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, contents).expect("source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let receive_config = config.clone();

        let accepted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let accepted_probe = accepted.clone();
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive_gated(&receive_config, &mut output, move |request| {
                assert_eq!(request.status, IncomingRequestStatus::Pending);
                assert_eq!(request.filename, "ok.txt");
                assert_eq!(request.file_size, contents.len() as u64);
                assert!(!request.checksum.is_empty(), "checksum available");
                accepted_probe.store(true, std::sync::atomic::Ordering::SeqCst);
                true
            })
        });
        wait_for_receiver_advert(&config);
        let advert = load_receiver_advert(&config).expect("receiver advert");
        let mut send_output = Vec::new();

        run_send_gated(
            &source,
            Some(advert.address),
            true,
            &config,
            &mut send_output,
        )
        .expect("send after acceptance");
        receiver
            .join()
            .expect("receiver thread")
            .expect("receive completes");

        assert!(accepted.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            fs::read(receive_dir.join("ok.txt")).expect("received"),
            contents
        );
    }

    #[test]
    fn non_discoverable_receiver_skips_mdns_but_still_receives_by_address() {
        // Task 3: discoverable=false must suppress the mDNS advertisement while
        // the receiver still accepts a direct connection by address.
        let workspace = TempWorkspace::new("quic-cli-nodisc");
        let source = workspace.path("direct.txt");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        let contents = b"nexo direct connect without discovery";
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, contents).expect("source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let receive_config = config.clone();

        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive_gated_with(
                &receive_config,
                &mut output,
                ReceiveOptions {
                    discoverable: false,
                },
                |_request| true,
            )?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&config);
        let advert = load_receiver_advert(&config).expect("receiver advert");
        let mut send_output = Vec::new();

        run_send_gated(
            &source,
            Some(advert.address),
            true,
            &config,
            &mut send_output,
        )
        .expect("direct send to non-discoverable receiver");
        let receive_output = receiver
            .join()
            .expect("receiver thread")
            .expect("receive completes");

        assert!(
            receive_output.contains("discovery disabled"),
            "must announce discovery is off: {receive_output}"
        );
        assert_eq!(
            fs::read(receive_dir.join("direct.txt")).expect("received"),
            contents
        );
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

        let send_result = run_send_gated(
            &source,
            Some(advert.address),
            true,
            &config,
            &mut send_output,
        );
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
    fn receiver_serves_successive_transfers_on_one_listener() {
        // Regression: the receiver used to exit after a single transfer, so a
        // second `send` to the same receiver (and desktop stress runs) failed
        // with "connection lost". `run_receive_loop` keeps one bound listener
        // serving successive transfers. Here it serves two, then stops.
        let workspace = TempWorkspace::new("quic-cli-successive");
        let source_a = workspace.path("a.txt");
        let source_b = workspace.path("b.txt");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source_a, b"first transfer payload").expect("source a");
        fs::write(&source_b, b"second transfer payload, different").expect("source b");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let receive_config = config.clone();

        // Receiver serves exactly two transfers, then `should_continue` returns
        // false so the loop ends and the thread joins (keeps the test bounded).
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            let mut served = 0u32;
            run_receive_loop(
                &receive_config,
                &mut output,
                ReceiveOptions::default(),
                |_request| true,
                || {
                    // Continue while fewer than two transfers have completed.
                    // Checked before each accept; after two it returns false.
                    let go = served < 2;
                    served += 1;
                    go
                },
            )?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });

        wait_for_receiver_advert(&config);
        let advert = load_receiver_advert(&config).expect("receiver advert");

        // First transfer.
        let mut out_a = Vec::new();
        run_send_gated(&source_a, Some(advert.address), true, &config, &mut out_a).expect("send a");
        assert!(
            String::from_utf8(out_a)
                .unwrap()
                .contains("transfer complete"),
            "first transfer should complete"
        );
        // The received file from the first transfer is present.
        assert_eq!(
            fs::read(receive_dir.join("a.txt")).expect("received a"),
            b"first transfer payload"
        );

        // Second transfer to the SAME still-alive receiver — this is what used
        // to fail with "connection lost".
        let mut out_b = Vec::new();
        run_send_gated(&source_b, Some(advert.address), true, &config, &mut out_b).expect("send b");
        assert!(
            String::from_utf8(out_b)
                .unwrap()
                .contains("transfer complete"),
            "second transfer should complete on the same receiver"
        );
        assert_eq!(
            fs::read(receive_dir.join("b.txt")).expect("received b"),
            b"second transfer payload, different"
        );

        let receive_output = receiver.join().expect("receiver thread").expect("output");
        // Both transfers were accepted and completed by the one receiver.
        assert_eq!(
            receive_output.matches("completed").count(),
            2,
            "receiver should have completed two transfers: {receive_output}"
        );
    }

    #[test]
    fn send_to_peer_uses_explicit_certificate_without_local_receiver_peer() {
        // The desktop send path: send using a caller-supplied certificate (as if
        // from the trusted store) with a sender that has NO local receiver.peer.
        // Proves sending no longer depends on this device's own receiver.peer —
        // the root cause of desktop-to-receiver failures.
        let workspace = TempWorkspace::new("quic-cli-topeer");
        let source = workspace.path("source.txt");
        let receive_dir = workspace.path("received");
        let recv_config = CliConfig {
            state_dir: workspace.path("recv-state"),
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        // Sender uses a separate state dir that never runs `receive`, so it has
        // no receiver.peer of its own.
        let send_config = CliConfig {
            state_dir: workspace.path("send-state"),
            receive_dir: workspace.path("send-recv"),
            chunk_size: 8,
        };
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, b"desktop sends with a trusted certificate").expect("source");

        let receiver_config = recv_config.clone();
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive(&receiver_config, &mut output)?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&recv_config);

        // The certificate the desktop would have captured at pairing and stored.
        let advert = load_receiver_advert(&recv_config).expect("receiver advert");
        // The sender genuinely has no receiver.peer of its own.
        assert!(
            load_receiver_advert(&send_config).is_err(),
            "sender must not rely on a local receiver.peer"
        );

        let mut send_output = Vec::new();
        run_send_to_peer(
            &source,
            advert.address,
            advert.certificate_der,
            &send_config,
            &mut send_output,
        )
        .expect("send to trusted peer");

        let receive_output = receiver.join().expect("receiver thread").expect("output");
        assert_eq!(
            fs::read(receive_dir.join("source.txt")).expect("received"),
            b"desktop sends with a trusted certificate"
        );
        assert!(
            String::from_utf8(send_output)
                .expect("send utf8")
                .contains("transfer complete")
        );
        assert!(receive_output.contains("completed"));
    }

    #[test]
    fn self_transfer_sends_to_own_receiver_over_full_quic_path() {
        // Reproduction / regression for desktop self-transfer: ONE config runs
        // the receiver and also sends a file to its OWN advertised endpoint using
        // its OWN certificate. This exercises the complete production path — QUIC,
        // pairing cert pinning, chunking, encryption, integrity — with no special
        // self-copy shortcut. The sender is the same identity as the receiver.
        let workspace = TempWorkspace::new("quic-cli-self");
        let source = workspace.path("source.txt");
        let receive_dir = workspace.path("received");
        let config = CliConfig {
            state_dir: workspace.path("state"),
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        fs::create_dir_all(&receive_dir).expect("receive dir");
        let payload = b"nexo sends this file to itself over real QUIC".to_vec();
        fs::write(&source, &payload).expect("source");

        let receiver_config = config.clone();
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive(&receiver_config, &mut output)?;
            String::from_utf8(output).map_err(|error| {
                Box::new(IoError::new(ErrorKind::InvalidData, error))
                    as Box<dyn Error + Send + Sync>
            })
        });
        wait_for_receiver_advert(&config);

        // The device's own advertised endpoint + certificate — exactly what the
        // desktop would resolve for "send to this same device".
        let advert = load_receiver_advert(&config).expect("own receiver advert");

        let mut send_output = Vec::new();
        run_send_to_peer(
            &source,
            advert.address,
            advert.certificate_der,
            &config,
            &mut send_output,
        )
        .expect("self-send over quic");

        let receive_output = receiver.join().expect("receiver thread").expect("output");
        assert_eq!(
            fs::read(receive_dir.join("source.txt")).expect("received"),
            payload,
            "self-transferred bytes must match the source exactly"
        );
        assert!(
            String::from_utf8(send_output)
                .expect("send utf8")
                .contains("transfer complete")
        );
        assert!(receive_output.contains("completed"));
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

    #[test]
    fn resume_after_real_interrupt_reconnects_and_skips_completed_chunks() {
        // End-to-end reproduction of the real-world resume failure:
        //   1. A real receiver accepts a transfer and persists checkpoints.
        //   2. The sender is interrupted mid-transfer (connection dropped).
        //   3. Both peers restart.
        //   4. The sender, pinned to the receiver's ORIGINAL advertised address,
        //      must reconnect and resume, sending only the chunks the receiver is
        //      still missing.
        //
        // Before the fix, a restarted receiver bound a new random port with a new
        // certificate, so the pinned sender could never reconnect and resume.
        let workspace = TempWorkspace::new("quic-cli-interrupt-resume");
        let source = workspace.path("source.bin");
        let receive_dir = workspace.path("received");
        let state_dir = workspace.path("state");
        // 48 bytes / 8-byte chunks => 6 chunks.
        let contents: Vec<u8> = (0..48u8).collect();
        fs::create_dir_all(&receive_dir).expect("receive dir");
        fs::write(&source, &contents).expect("source");
        let config = CliConfig {
            state_dir,
            receive_dir: receive_dir.clone(),
            chunk_size: 8,
        };
        let total_chunks = generate_manifest(&source, config.chunk_size)
            .expect("manifest")
            .total_chunks as usize;

        // --- Run 1: partial transfer, then interrupt ---
        let receive_config = config.clone();
        let receiver = thread::spawn(move || {
            let mut output = Vec::new();
            run_receive(&receive_config, &mut output)
        });
        wait_for_receiver_advert(&config);
        let advert_v1 = load_receiver_advert(&config).expect("advert v1");

        let interrupted_at =
            send_partial_then_drop(&source, advert_v1.address, &config, 2).expect("partial send");
        assert!(
            interrupted_at >= 1 && interrupted_at < total_chunks,
            "expected a partial interrupt, got {interrupted_at}/{total_chunks}"
        );
        let run1 = receiver.join().expect("receiver thread");
        assert!(
            run1.is_err(),
            "receiver should observe the interrupted connection as an error"
        );

        // The receiver must have persisted real checkpoint + resume state.
        let persisted = {
            let storage = storage(&config).expect("storage");
            let transfer_id = TransferId(format!(
                "transfer-{}",
                generate_manifest(&source, config.chunk_size)
                    .expect("manifest")
                    .sha256
            ));
            let checkpoint = storage
                .load_checkpoint(&transfer_id)
                .expect("load checkpoint")
                .expect("checkpoint persisted across interrupt");
            assert!(
                storage
                    .load_resume_metadata(&transfer_id)
                    .expect("load resume metadata")
                    .is_some(),
                "resume metadata must survive the interrupt"
            );
            checkpoint.completed_chunks.len()
        };
        assert!(
            persisted >= 1 && persisted < total_chunks,
            "expected {persisted} to be a partial checkpoint of {total_chunks}"
        );

        // --- Run 2: restart receiver and sender, expect resume ---
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
        let advert_v2 = load_receiver_advert(&config).expect("advert v2");

        // The restarted receiver must keep the same address and certificate, so
        // the sender's previously learned endpoint stays valid.
        assert_eq!(
            advert_v2.address, advert_v1.address,
            "restarted receiver must reuse its advertised address"
        );
        assert_eq!(
            advert_v2.certificate_der, advert_v1.certificate_der,
            "restarted receiver must reuse its certificate"
        );

        // Pin the sender to the ORIGINAL address (as `--host` would).
        let mut send_output = Vec::new();
        run_send(&source, Some(advert_v1.address), &config, &mut send_output)
            .expect("resumed send reconnects");
        let receive_output = receiver
            .join()
            .expect("receiver thread")
            .expect("resumed receive");
        let send_output = String::from_utf8(send_output).expect("send utf8");

        // Final file matches the source byte-for-byte (SHA-256 verified by the
        // receiver's verify_complete step before it reports completion).
        assert_eq!(
            fs::read(receive_dir.join("source.bin")).expect("received"),
            contents
        );
        // The sender retransmitted ONLY the missing chunks, not the whole file.
        let resent = send_output
            .lines()
            .filter(|line| line.starts_with("sent:"))
            .count();
        assert_eq!(
            resent,
            total_chunks - persisted,
            "sender must resend only missing chunks"
        );
        assert!(resent < total_chunks, "resume must not resend everything");
        assert!(receive_output.contains(&format!("receiving: {persisted}/{total_chunks} chunks")));

        // Checkpoint persistence survived the restart and now reflects completion.
        let storage = storage(&config).expect("storage");
        let transfer_id = TransferId(format!(
            "transfer-{}",
            generate_manifest(&source, config.chunk_size)
                .expect("manifest")
                .sha256
        ));
        let checkpoint = storage
            .load_checkpoint(&transfer_id)
            .expect("load checkpoint")
            .expect("checkpoint exists");
        assert_eq!(checkpoint.completed_chunks.len(), total_chunks);
    }

    /// Drives a real transfer through the pipeline + QUIC + storage, but sends
    /// only the first `limit` missing chunks before dropping the connection,
    /// simulating an interrupted sender. Returns how many chunks were sent.
    fn send_partial_then_drop(
        source: &Path,
        host: SocketAddr,
        config: &CliConfig,
        limit: usize,
    ) -> CliResult<usize> {
        let advert = load_receiver_advert(config)?;
        let manifest = generate_manifest(source, config.chunk_size)?;
        let transfer_id = TransferId(format!("transfer-{}", manifest.sha256));
        let session_id = SessionId(format!("session-{}", transfer_id.0));
        let sender = TransferPipelineSender::prepare(
            source,
            TransferPipelineConfig {
                session_id: session_id.clone(),
                transfer_id: transfer_id.clone(),
                sender_peer: sender_peer(),
                receiver_peer: receiver_peer(),
                chunk_size: config.chunk_size,
            },
        )?;
        let mut provider = QuicTransportProvider::localhost(sender_peer())?;
        provider.register_peer(receiver_peer(), host, advert.certificate_der);
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
        ensure_acceptance(&stream.receive_message()?)?;
        let cipher = send_key_exchange(&mut stream, &session_id, &transfer_id)?;
        let missing = missing_from_envelope(stream.receive_message()?, &transfer_id)?;

        let mut sent = 0;
        for chunk_id in missing.chunks.iter().take(limit) {
            stream.send_message(sender.chunk_envelope(chunk_id, &cipher)?)?;
            sent += 1;
        }
        stream.close()?;

        // Keep the connection alive until the receiver has actually persisted at
        // least one chunk, so the interrupted state on disk is deterministic.
        // Tolerate transient SQLite contention with the receiver's writer.
        let poll_storage = storage(config)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(checkpoint)) = poll_storage.load_checkpoint(&transfer_id)
                && !checkpoint.completed_chunks.is_empty()
            {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }

        Ok(sent)
    }

    /// Task 5: crash-recovery reliability harness. Runs the full
    /// interrupt -> restart -> resume -> verify cycle `cycles` times and returns
    /// (successes, total_recovery_time). A cycle "succeeds" when the resumed
    /// transfer reproduces the source file byte-for-byte (the receiver's
    /// verify_complete SHA-256 gate must pass for the file to be written).
    ///
    /// This drives the real QUIC + resume + checkpoint stack; nothing in the
    /// engine is modified. Kept small by default so it is fast and not flaky; a
    /// heavier variant is available via the ignored large-file test.
    fn run_recovery_cycles(cycles: usize, chunk_size: usize, bytes: usize) -> (usize, Duration) {
        let mut successes = 0;
        let mut total_recovery = Duration::ZERO;

        for cycle in 0..cycles {
            let workspace = TempWorkspace::new(&format!("recovery-{cycle}"));
            let source = workspace.path("payload.bin");
            let receive_dir = workspace.path("received");
            let state_dir = workspace.path("state");
            let contents: Vec<u8> = (0..bytes).map(|index| (index % 251) as u8).collect();
            fs::create_dir_all(&receive_dir).expect("receive dir");
            fs::write(&source, &contents).expect("source");
            let config = CliConfig {
                state_dir,
                receive_dir: receive_dir.clone(),
                chunk_size,
            };
            let total_chunks = generate_manifest(&source, config.chunk_size)
                .expect("manifest")
                .total_chunks as usize;

            // Run 1: receiver accepts, sender sends part of the file, then drops.
            let receive_config = config.clone();
            let receiver = thread::spawn(move || {
                let mut output = Vec::new();
                run_receive(&receive_config, &mut output)
            });
            wait_for_receiver_advert(&config);
            let advert = load_receiver_advert(&config).expect("advert");
            let partial = (total_chunks / 2).max(1);
            let sent = send_partial_then_drop(&source, advert.address, &config, partial)
                .expect("partial send");
            assert!(
                sent >= 1 && sent < total_chunks,
                "expected a partial interrupt"
            );
            let _ = receiver.join().expect("receiver thread"); // errors on interrupt

            // Run 2: restart receiver + sender, time the resume to completion.
            let recovery_start = Instant::now();
            let receive_config = config.clone();
            let receiver = thread::spawn(move || {
                let mut output = Vec::new();
                run_receive(&receive_config, &mut output)
            });
            wait_for_receiver_advert(&config);
            let mut send_output = Vec::new();
            let resumed = run_send_gated(
                &source,
                Some(advert.address),
                true,
                &config,
                &mut send_output,
            );
            let received = receiver.join().expect("receiver thread");
            let recovery = recovery_start.elapsed();

            // A cycle succeeds only if both sides finished and bytes match.
            let ok = resumed.is_ok()
                && received.is_ok()
                && fs::read(receive_dir.join("payload.bin")).ok().as_deref() == Some(&contents[..]);
            if ok {
                successes += 1;
                total_recovery += recovery;
            }
        }

        (successes, total_recovery)
    }

    #[test]
    fn crash_recovery_is_reliable_across_repeated_cycles() {
        // Repeated interrupt/resume/verify must succeed every time. Small file +
        // a few cycles keeps this fast and deterministic; the mechanism is
        // identical to a 5 GB run (only chunk count differs).
        let cycles = 3;
        let (successes, total_recovery) = run_recovery_cycles(cycles, 8, 96);
        assert_eq!(successes, cycles, "every recovery cycle must succeed");
        let avg = total_recovery / cycles as u32;
        // Sanity: recovery completes quickly for a tiny file.
        assert!(
            avg < Duration::from_secs(30),
            "average recovery {avg:?} unexpectedly slow"
        );
    }

    #[test]
    #[ignore = "heavy: 10 recovery cycles over a larger file; run with --ignored"]
    fn crash_recovery_reliability_report_10x() {
        // Task 5's headline scenario: 10 interrupt/resume/verify cycles with a
        // reliability report. Uses a larger (still bounded) file so it exercises
        // many chunks per cycle without needing a literal 5 GB payload.
        let cycles = 10;
        let (successes, total_recovery) = run_recovery_cycles(cycles, 4 * 1024, 4 * 1024 * 1024);
        let avg = if successes > 0 {
            total_recovery / successes as u32
        } else {
            Duration::ZERO
        };
        println!("crash-recovery report: {successes}/{cycles} succeeded, avg recovery {avg:?}");
        assert_eq!(successes, cycles, "all 10 recovery cycles must succeed");
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
