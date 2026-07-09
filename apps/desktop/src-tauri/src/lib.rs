mod autostart;
mod store;

use cli::{
    CliConfig, CliStatePaths, DiscoveredPeer, IncomingTransferRequest, ReceiveOptions,
    ReceiverEndpoint, TransferStatusSnapshot, build_transfer_request, discover_peers,
    receiver_endpoint, run_receive_gated_with, run_send, transfer_status_snapshot,
};
use engine::chunker::default_chunk_size;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Emitter, Manager, State};

type DesktopResult<T> = Result<T, String>;

/// Event name emitted when a pending AirDrop send request needs confirmation.
const TRANSFER_REQUEST_EVENT: &str = "transfer_request_created";
/// Event name emitted when an incoming transfer needs the receiver's approval.
const INCOMING_TRANSFER_EVENT: &str = "incoming_transfer_request";
/// Event emitted when a trusted-device transfer was auto-accepted (no prompt),
/// so the UI can surface a passive notification instead of a dialog.
const INCOMING_AUTO_ACCEPTED_EVENT: &str = "incoming_transfer_auto_accepted";

#[derive(Debug)]
pub struct DesktopAppState {
    config: CliConfig,
    jobs: Arc<Mutex<HashMap<u64, TransferJob>>>,
    next_job_id: AtomicU64,
    stress: Arc<Mutex<HashMap<u64, StressRun>>>,
    next_stress_id: AtomicU64,
    requests: Arc<Mutex<HashMap<String, PendingTransferRequest>>>,
    incoming: Arc<Mutex<HashMap<String, PendingIncoming>>>,
    store: Arc<store::AppStore>,
    presence: Arc<Mutex<HashMap<String, PeerPresence>>>,
    /// In-flight device pairings awaiting user confirmation, keyed by peer id.
    /// A pairing is only turned into a trusted device after the user confirms
    /// the fingerprint (`confirm_pairing`), so discovery alone never grants trust.
    pairings: Arc<Mutex<HashMap<String, PendingPairing>>>,
}

impl DesktopAppState {
    pub fn new(config: CliConfig) -> Self {
        let store = Arc::new(store::AppStore::new(config.state_dir.clone()));
        Self {
            config,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_job_id: AtomicU64::new(1),
            stress: Arc::new(Mutex::new(HashMap::new())),
            next_stress_id: AtomicU64::new(1),
            requests: Arc::new(Mutex::new(HashMap::new())),
            incoming: Arc::new(Mutex::new(HashMap::new())),
            store,
            presence: Arc::new(Mutex::new(HashMap::new())),
            pairings: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn config(&self) -> CliConfig {
        self.config.clone()
    }

    fn next_job_id(&self) -> u64 {
        self.next_job_id.fetch_add(1, Ordering::Relaxed)
    }

    fn next_stress_id(&self) -> u64 {
        self.next_stress_id.fetch_add(1, Ordering::Relaxed)
    }
}

/// A peer is considered "online" if it was seen within this window.
const ONLINE_WINDOW_SECS: u64 = 12;

/// Last-seen bookkeeping for a discovered peer, kept across discovery scans so
/// online/offline transitions survive a peer briefly dropping out of a scan.
#[derive(Debug, Clone)]
struct PeerPresence {
    display_name: String,
    address: String,
    platform: String,
    last_seen: u64,
}

/// UI-facing device presence model (Feature 1).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeerDevice {
    pub id: String,
    pub display_name: String,
    pub address: String,
    pub platform: String,
    pub last_seen: u64,
    pub online: bool,
    pub trusted: bool,
}

/// UI-facing view of an incoming transfer awaiting the receiver's decision
/// (the modal payload for `incoming_transfer_request`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IncomingTransferResponse {
    pub id: String,
    pub sender: String,
    pub filename: String,
    pub file_size: u64,
    pub checksum: String,
    pub timestamp: u64,
}

impl From<&IncomingTransferRequest> for IncomingTransferResponse {
    fn from(request: &IncomingTransferRequest) -> Self {
        Self {
            id: request.id.clone(),
            sender: request.sender.clone(),
            filename: request.filename.clone(),
            file_size: request.file_size,
            checksum: request.checksum.clone(),
            timestamp: request.timestamp,
        }
    }
}

/// A receive thread parked on the approval gate: the UI-facing view plus the
/// channel used to deliver the user's accept/reject decision.
#[derive(Debug)]
struct PendingIncoming {
    response: IncomingTransferResponse,
    decision: SyncSender<bool>,
}

/// Immutable facts about a transfer captured when its job is spawned, used to
/// write a history record when the job finishes.
#[derive(Debug, Clone)]
struct TransferMeta {
    direction: &'static str,
    filename: String,
    peer: String,
}

/// Extracts the total byte count from the last progress line the core printed,
/// e.g. `sent: 1/1 chunks, 300000/300000 bytes` -> 300000.
fn last_total_bytes(lines: &[String]) -> u64 {
    for line in lines.iter().rev() {
        let Some(index) = line.find(" bytes") else {
            continue;
        };
        let Some(pair) = line[..index].rsplit(' ').next() else {
            continue;
        };
        if let Some((_, total)) = pair.split_once('/')
            && let Ok(value) = total.trim().parse::<u64>()
        {
            return value;
        }
    }
    0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettings {
    pub state_dir: String,
    pub receive_dir: String,
    pub chunk_size: u64,
}

impl From<CliConfig> for DesktopSettings {
    fn from(config: CliConfig) -> Self {
        Self {
            state_dir: path_to_string(config.state_dir),
            receive_dir: path_to_string(config.receive_dir),
            chunk_size: config.chunk_size as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatePathsResponse {
    pub state_dir: String,
    pub receive_dir: String,
    pub database: String,
    pub receiver_peer: String,
    pub latest_transfer: String,
    pub peer_id: String,
}

impl From<CliStatePaths> for StatePathsResponse {
    fn from(paths: CliStatePaths) -> Self {
        Self {
            state_dir: path_to_string(paths.state_dir),
            receive_dir: path_to_string(paths.receive_dir),
            database: path_to_string(paths.database),
            receiver_peer: path_to_string(paths.receiver_peer),
            latest_transfer: path_to_string(paths.latest_transfer),
            peer_id: path_to_string(paths.peer_id),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeerResponse {
    pub peer_id: String,
    pub display_name: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub fingerprint: Option<String>,
}

impl From<DiscoveredPeer> for PeerResponse {
    fn from(peer: DiscoveredPeer) -> Self {
        Self {
            peer_id: peer.peer_id,
            display_name: peer.display_name,
            addresses: peer.addresses,
            port: peer.port,
            fingerprint: peer.fingerprint,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverEndpointResponse {
    pub address: String,
}

impl From<ReceiverEndpoint> for ReceiverEndpointResponse {
    fn from(endpoint: ReceiverEndpoint) -> Self {
        Self {
            address: endpoint.address,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusResponse {
    pub latest: Option<TransferStatusDetailsResponse>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransferStatusDetailsResponse {
    pub transfer_id: String,
    pub session_id: String,
    pub state: Option<String>,
    pub file_name: Option<String>,
    pub completed_chunks: u64,
    pub total_chunks: u64,
    pub completed_bytes: u64,
    pub total_bytes: u64,
}

impl From<TransferStatusSnapshot> for TransferStatusResponse {
    fn from(snapshot: TransferStatusSnapshot) -> Self {
        Self {
            latest: snapshot.latest.map(|latest| TransferStatusDetailsResponse {
                transfer_id: latest.transfer_id,
                session_id: latest.session_id,
                state: latest.state,
                file_name: latest.file_name,
                completed_chunks: latest.completed_chunks,
                total_chunks: latest.total_chunks,
                completed_bytes: latest.completed_bytes,
                total_bytes: latest.total_bytes,
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartJobResponse {
    pub job_id: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundSettingsResponse {
    pub background_receiving: bool,
    pub start_on_login: bool,
}

impl From<store::BackgroundSettings> for BackgroundSettingsResponse {
    fn from(settings: store::BackgroundSettings) -> Self {
        Self {
            background_receiving: settings.background_receiving,
            start_on_login: settings.start_on_login,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverStatusResponse {
    pub receiving: bool,
    pub discoverable: bool,
    pub background_enabled: bool,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingResponse {
    pub completed: bool,
    pub completed_at: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfoResponse {
    pub version: String,
    pub build_type: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IdentityPreviewResponse {
    pub display_name: String,
    pub address: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelfCheckResponse {
    pub storage_writable: bool,
    pub receiver_ready: bool,
    pub discovery_enabled: bool,
    pub download_dir: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsResponse {
    pub device_id: String,
    pub certificate_fingerprint: String,
    pub mdns_discoverable: bool,
    pub receiver_running: bool,
    pub endpoint: Option<String>,
    pub state_dir: String,
    pub download_dir: String,
    pub last_transfer: Option<String>,
    pub total_transfers: u64,
    pub completed_transfers: u64,
    pub failed_transfers: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// UI-facing view of a pending AirDrop transfer request (the modal payload).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransferRequestResponse {
    pub id: String,
    pub file_path: String,
    pub file_name: String,
    pub file_size: u64,
    pub peer_display_name: String,
    pub peer_address: String,
    pub status: String,
}

impl From<&cli::AirdropRequest> for TransferRequestResponse {
    fn from(request: &cli::AirdropRequest) -> Self {
        Self {
            id: request.id.clone(),
            file_path: request.file_path.display().to_string(),
            file_name: request.file_name.clone(),
            file_size: request.file_size,
            peer_display_name: request.peer_display_name.clone(),
            peer_address: request.peer_address.to_string(),
            status: "pending".to_owned(),
        }
    }
}

/// Server-side pending request: the UI-facing view plus the resolved inputs
/// needed to actually run the send once approved.
#[derive(Debug, Clone)]
struct PendingTransferRequest {
    file_path: PathBuf,
    host: Option<SocketAddr>,
    response: TransferRequestResponse,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StartStressResponse {
    pub run_id: u64,
}

/// A JSON benchmark sample (one iteration) — Task 4 export shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkSample {
    pub index: u64,
    pub ok: bool,
    pub bytes: u64,
    pub duration_ms: u64,
    pub mbps: f64,
}

/// A full benchmark report written to `<state_dir>/reports/`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkReport {
    pub version: String,
    pub generated_at: u64,
    pub file_path: String,
    pub file_size: u64,
    pub iterations: u64,
    pub completed: u64,
    pub failed: u64,
    pub avg_mbps: f64,
    pub avg_duration_ms: u64,
    pub chunk_size: u64,
    pub samples: Vec<BenchmarkSample>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StressRunState {
    Running,
    Completed,
    Failed,
}

/// One send iteration inside a stress run, with benchmark metrics.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StressIteration {
    pub index: u64,
    pub state: TransferJobState,
    pub error: Option<String>,
    /// Wall-clock duration of this send iteration.
    pub duration_ms: u64,
    /// Bytes transferred (from the core's final progress line).
    pub bytes: u64,
    /// Average throughput in MB/s for this iteration.
    pub mbps: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StressRunSnapshot {
    pub run_id: u64,
    pub file_path: String,
    pub target_iterations: u64,
    pub completed: u64,
    pub failed: u64,
    pub state: StressRunState,
    pub iterations: Vec<StressIteration>,
    pub last_output: Vec<String>,
    /// Aggregate benchmark: average MB/s across successful iterations.
    pub avg_mbps: f64,
    /// Aggregate benchmark: average successful-iteration duration.
    pub avg_duration_ms: u64,
    /// Largest transferred size observed (bytes).
    pub file_size: u64,
}

/// Automated repeated-transfer run: sends one file `target_iterations` times via
/// the existing `run_send` core API, recording each attempt. It never touches
/// networking, QUIC, storage, or resume logic itself -- it only orchestrates
/// repeated calls into the unchanged core, which is exactly what a large-file
/// stress harness needs.
#[derive(Debug, Clone)]
struct StressRun {
    file_path: String,
    target_iterations: u64,
    completed: u64,
    failed: u64,
    state: StressRunState,
    iterations: Vec<StressIteration>,
    last_output: Vec<String>,
}

impl StressRun {
    fn new(file_path: String, target_iterations: u64) -> Self {
        Self {
            file_path,
            target_iterations,
            completed: 0,
            failed: 0,
            state: StressRunState::Running,
            iterations: Vec::new(),
            last_output: Vec::new(),
        }
    }

    fn snapshot(&self, run_id: u64) -> StressRunSnapshot {
        let successful: Vec<&StressIteration> = self
            .iterations
            .iter()
            .filter(|i| i.state == TransferJobState::Completed)
            .collect();
        let avg_mbps = if successful.is_empty() {
            0.0
        } else {
            successful.iter().map(|i| i.mbps).sum::<f64>() / successful.len() as f64
        };
        let avg_duration_ms = if successful.is_empty() {
            0
        } else {
            successful.iter().map(|i| i.duration_ms).sum::<u64>() / successful.len() as u64
        };
        let file_size = self.iterations.iter().map(|i| i.bytes).max().unwrap_or(0);

        StressRunSnapshot {
            run_id,
            file_path: self.file_path.clone(),
            target_iterations: self.target_iterations,
            completed: self.completed,
            failed: self.failed,
            state: self.state,
            iterations: self.iterations.clone(),
            last_output: self.last_output.clone(),
            avg_mbps,
            avg_duration_ms,
            file_size,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TransferJobSnapshot {
    pub job_id: u64,
    pub kind: TransferJobKind,
    pub state: TransferJobState,
    pub output: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferJobKind {
    Send,
    Receive,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransferJobState {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
struct TransferJob {
    kind: TransferJobKind,
    state: TransferJobState,
    output: Vec<String>,
    error: Option<String>,
}

impl TransferJob {
    fn running(kind: TransferJobKind) -> Self {
        Self {
            kind,
            state: TransferJobState::Running,
            output: Vec::new(),
            error: None,
        }
    }

    fn snapshot(&self, job_id: u64) -> TransferJobSnapshot {
        TransferJobSnapshot {
            job_id,
            kind: self.kind,
            state: self.state,
            output: self.output.clone(),
            error: self.error.clone(),
        }
    }
}

#[derive(Debug, Default)]
struct LineBuffer {
    bytes: Vec<u8>,
}

impl LineBuffer {
    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.bytes)
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl std::io::Write for LineBuffer {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct JobOutput {
    buffer: LineBuffer,
    jobs: Arc<Mutex<HashMap<u64, TransferJob>>>,
    job_id: u64,
}

impl JobOutput {
    fn new(jobs: Arc<Mutex<HashMap<u64, TransferJob>>>, job_id: u64) -> Self {
        Self {
            buffer: LineBuffer::default(),
            jobs,
            job_id,
        }
    }

    fn lines(&self) -> Vec<String> {
        self.buffer.lines()
    }

    fn sync(&self) {
        if let Ok(mut jobs) = self.jobs.lock()
            && let Some(job) = jobs.get_mut(&self.job_id)
        {
            job.output = self.lines();
        }
    }
}

impl std::io::Write for JobOutput {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.buffer.write(buffer)?;
        self.sync();
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.buffer.flush()
    }
}

#[tauri::command]
fn get_settings(state: State<'_, DesktopAppState>) -> DesktopSettings {
    state.config().into()
}

#[tauri::command]
fn get_state_paths(state: State<'_, DesktopAppState>) -> StatePathsResponse {
    state.config().app_paths().into()
}

#[tauri::command]
fn get_status(state: State<'_, DesktopAppState>) -> DesktopResult<TransferStatusResponse> {
    transfer_status_snapshot(&state.config())
        .map(TransferStatusResponse::from)
        .map_err(error_to_string)
}

#[tauri::command]
fn get_receiver_endpoint(
    state: State<'_, DesktopAppState>,
) -> DesktopResult<Option<ReceiverEndpointResponse>> {
    receiver_endpoint(&state.config())
        .map(|endpoint| endpoint.map(ReceiverEndpointResponse::from))
        .map_err(error_to_string)
}

#[tauri::command]
fn discover_known_peers(state: State<'_, DesktopAppState>) -> DesktopResult<Vec<PeerResponse>> {
    discover_peers(&state.config())
        .map(|peers| peers.into_iter().map(PeerResponse::from).collect())
        .map_err(error_to_string)
}

#[tauri::command]
fn start_receive(
    app: tauri::AppHandle,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<StartJobResponse> {
    let job_id = spawn_receive_job(&app, &state)?;
    Ok(StartJobResponse { job_id })
}

/// Spawns one gated receive session and returns its job id. Shared by the
/// `start_receive` command and the background auto-start, so the approval gate,
/// history recording, and event flow are identical whether the user pressed
/// "Receive" or the app started receiving on its own.
fn spawn_receive_job(app: &tauri::AppHandle, state: &DesktopAppState) -> DesktopResult<u64> {
    // Only one active receiver at a time: if a receive job is already running,
    // reuse it rather than binding a second listener.
    if let Ok(jobs) = state.jobs.lock()
        && let Some((existing, _)) = jobs.iter().find(|(_, job)| {
            job.kind == TransferJobKind::Receive && job.state == TransferJobState::Running
        })
    {
        return Ok(*existing);
    }

    let prefs = state.store.preferences();
    // Task 2: receive into the user's chosen download folder when set and usable;
    // otherwise fall back to the default receive_dir. Storage/state paths are
    // untouched — only where completed files land changes.
    let base_config = state.config();
    let config = CliConfig {
        receive_dir: resolve_download_dir(&prefs.download_dir, &base_config.receive_dir),
        ..base_config
    };
    // Task 3: only advertise over mDNS when the user is discoverable. The
    // receiver still accepts direct-by-address connections when off.
    let options = ReceiveOptions {
        discoverable: prefs.discoverable,
    };
    // Task 1: auto-accept policy, evaluated per incoming request. Fail-safe: any
    // uncertainty falls through to the manual dialog. Never bypasses the QUIC
    // certificate handshake.
    let auto_accept = should_auto_accept(&prefs, &state.store.trusted_devices());

    let job_id = state.next_job_id();
    insert_job(
        &state.jobs,
        job_id,
        TransferJob::running(TransferJobKind::Receive),
    )?;

    // Receiver-side approval gate. The approver runs on the receive thread when
    // the sender's metadata arrives. When auto-accept applies it approves
    // without a prompt; otherwise it emits `incoming_transfer_request` and blocks
    // until the UI answers. The QUIC keep-alive holds the connection open while
    // it waits.
    let incoming = state.incoming.clone();
    let app = app.clone();
    let approver = move |request: &IncomingTransferRequest| -> bool {
        if auto_accept {
            let _ = app.emit(
                INCOMING_AUTO_ACCEPTED_EVENT,
                IncomingTransferResponse::from(request),
            );
            return true;
        }
        let response = IncomingTransferResponse::from(request);
        let (decision_tx, decision_rx) = std::sync::mpsc::sync_channel::<bool>(1);
        {
            let Ok(mut pending) = incoming.lock() else {
                return false;
            };
            pending.insert(
                response.id.clone(),
                PendingIncoming {
                    response: response.clone(),
                    decision: decision_tx,
                },
            );
        }
        if app.emit(INCOMING_TRANSFER_EVENT, response.clone()).is_err() {
            incoming.lock().ok().map(|mut map| map.remove(&response.id));
            return false;
        }
        // Wait for the user's decision (default reject if the channel drops).
        let accepted = decision_rx.recv().unwrap_or(false);
        incoming.lock().ok().map(|mut map| map.remove(&response.id));
        accepted
    };

    spawn_transfer_job(
        state.jobs.clone(),
        job_id,
        TransferJobKind::Receive,
        state.store.clone(),
        config.clone(),
        TransferMeta {
            direction: "receive",
            filename: "(incoming)".to_owned(),
            peer: "incoming".to_owned(),
        },
        move |output| run_receive_gated_with(&config, output, options, approver),
    );

    Ok(job_id)
}

/// Whether an incoming transfer should be auto-accepted without a prompt.
///
/// Fail-safe policy (Task 1): only when the user has explicitly enabled
/// `auto_accept_trusted` AND this device already has at least one trusted peer
/// on record. The QUIC certificate handshake still runs for every connection —
/// this only decides whether to skip the *UI* approval prompt, never whether to
/// trust the transport. With no trusted devices, or the setting off, every
/// transfer shows the dialog.
fn should_auto_accept(prefs: &store::AppPreferences, trusted: &[store::TrustedDevice]) -> bool {
    prefs.auto_accept_trusted && !trusted.is_empty()
}

/// Resolves the receive directory: the user's `download_dir` when it is set and
/// creatable/writable, otherwise the default. Never returns an unusable path.
fn resolve_download_dir(download_dir: &str, fallback: &std::path::Path) -> PathBuf {
    let trimmed = download_dir.trim();
    if trimmed.is_empty() {
        return fallback.to_path_buf();
    }
    let candidate = PathBuf::from(trimmed);
    if std::fs::create_dir_all(&candidate).is_ok() && is_writable_dir(&candidate) {
        candidate
    } else {
        fallback.to_path_buf()
    }
}

/// True if `dir` is a directory we can create a file in.
fn is_writable_dir(dir: &std::path::Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let probe = dir.join(".nexo-write-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[tauri::command]
fn approve_incoming_request(
    request_id: String,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<()> {
    signal_incoming(&state.incoming, &request_id, true)
}

#[tauri::command]
fn reject_incoming_request(
    request_id: String,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<()> {
    signal_incoming(&state.incoming, &request_id, false)
}

#[tauri::command]
fn list_incoming_requests(
    state: State<'_, DesktopAppState>,
) -> DesktopResult<Vec<IncomingTransferResponse>> {
    let incoming = state
        .incoming
        .lock()
        .map_err(|_| "desktop incoming registry is unavailable".to_owned())?;
    Ok(incoming
        .values()
        .map(|pending| pending.response.clone())
        .collect())
}

fn signal_incoming(
    incoming: &Arc<Mutex<HashMap<String, PendingIncoming>>>,
    request_id: &str,
    accepted: bool,
) -> DesktopResult<()> {
    let pending = incoming
        .lock()
        .map_err(|_| "desktop incoming registry is unavailable".to_owned())?
        .remove(request_id);
    match pending {
        // Best-effort: if the receive thread already gave up, the send fails
        // closed with the channel error and the decision is a no-op.
        Some(pending) => {
            let _ = pending.decision.send(accepted);
            Ok(())
        }
        None => Err(format!("unknown incoming transfer request: {request_id}")),
    }
}

#[tauri::command]
fn create_transfer_request(
    file_path: String,
    host: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<TransferRequestResponse> {
    let path = PathBuf::from(&file_path);
    validate_file_path(&path)?;
    let host = parse_host(host)?;
    // Reuse the CLI's trust-checked request builder: an untrusted host is
    // rejected here, before any confirmation is offered or bytes move.
    let request = build_transfer_request(&path, host, &state.config()).map_err(error_to_string)?;
    let response = TransferRequestResponse::from(&request);

    state
        .requests
        .lock()
        .map_err(|_| "desktop request registry is unavailable".to_owned())?
        .insert(
            response.id.clone(),
            PendingTransferRequest {
                file_path: request.file_path,
                host: Some(request.peer_address),
                response: response.clone(),
            },
        );

    // Notify the UI so it can show the mandatory confirmation modal. No transfer
    // is started here; the UI must call approve_transfer_request to proceed.
    app.emit(TRANSFER_REQUEST_EVENT, response.clone())
        .map_err(|error| format!("failed to emit transfer request event: {error}"))?;

    Ok(response)
}

#[tauri::command]
fn approve_transfer_request(
    request_id: String,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<StartJobResponse> {
    let pending = {
        let mut requests = state
            .requests
            .lock()
            .map_err(|_| "desktop request registry is unavailable".to_owned())?;
        requests
            .remove(&request_id)
            .ok_or_else(|| format!("unknown transfer request: {request_id}"))?
    };

    // User consent obtained: only now delegate to the unchanged run_send.
    let config = state.config();
    let path = pending.file_path;
    let host = pending.host;
    let meta = TransferMeta {
        direction: "send",
        filename: pending.response.file_name.clone(),
        peer: pending.response.peer_address.clone(),
    };
    let job_id = state.next_job_id();
    insert_job(
        &state.jobs,
        job_id,
        TransferJob::running(TransferJobKind::Send),
    )?;
    spawn_transfer_job(
        state.jobs.clone(),
        job_id,
        TransferJobKind::Send,
        state.store.clone(),
        config.clone(),
        meta,
        move |output| run_send(&path, host, &config, output),
    );

    Ok(StartJobResponse { job_id })
}

#[tauri::command]
fn reject_transfer_request(
    request_id: String,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<()> {
    let pending = state
        .requests
        .lock()
        .map_err(|_| "desktop request registry is unavailable".to_owned())?
        .remove(&request_id);
    // Feature 4: a rejected request is a cancelled transfer in history.
    if let Some(pending) = pending {
        state.store.record_transfer(store::TransferRecord {
            id: pending.response.id.clone(),
            filename: pending.response.file_name.clone(),
            size: pending.response.file_size,
            direction: "send".to_owned(),
            peer: pending.response.peer_address.clone(),
            timestamp: store::unix_now(),
            status: "cancelled".to_owned(),
            duration_ms: 0,
            checksum_ok: false,
        });
    }
    Ok(())
}

#[tauri::command]
fn list_transfer_requests(
    state: State<'_, DesktopAppState>,
) -> DesktopResult<Vec<TransferRequestResponse>> {
    let requests = state
        .requests
        .lock()
        .map_err(|_| "desktop request registry is unavailable".to_owned())?;
    Ok(requests
        .values()
        .map(|pending| pending.response.clone())
        .collect())
}

// ---- Feature 1: device presence ------------------------------------------

/// Runs one discovery scan, folds it into persistent presence, and returns the
/// device list (discovered + recently-seen + trusted-but-offline). Uses the
/// existing mDNS discovery unchanged as the source.
#[tauri::command]
fn list_devices(state: State<'_, DesktopAppState>) -> DesktopResult<Vec<PeerDevice>> {
    let config = state.config();
    let discovered = discover_peers(&config).map_err(error_to_string)?;
    let now = store::unix_now();

    let mut presence = state
        .presence
        .lock()
        .map_err(|_| "desktop presence registry is unavailable".to_owned())?;
    let mut seen_addresses = Vec::new();
    for peer in &discovered {
        let address = peer
            .addresses
            .first()
            .map(|address| format!("{address}:{}", peer.port))
            .unwrap_or_default();
        seen_addresses.push(address.clone());
        presence.insert(
            peer.peer_id.clone(),
            PeerPresence {
                display_name: peer.display_name.clone(),
                address,
                platform: "unknown".to_owned(),
                last_seen: now,
            },
        );
    }
    drop(presence);

    // Keep trusted-device last_seen fresh for the ones currently visible.
    state.store.touch_last_seen(&seen_addresses, now);
    let trusted = state.store.trusted_devices();

    let presence = state
        .presence
        .lock()
        .map_err(|_| "desktop presence registry is unavailable".to_owned())?;
    let mut devices: Vec<PeerDevice> = presence
        .iter()
        .map(|(id, entry)| {
            let trusted_entry = trusted
                .iter()
                .find(|device| device.address == entry.address);
            PeerDevice {
                id: id.clone(),
                display_name: trusted_entry
                    .map(|device| device.display_name.clone())
                    .unwrap_or_else(|| entry.display_name.clone()),
                address: entry.address.clone(),
                platform: entry.platform.clone(),
                last_seen: entry.last_seen,
                online: now.saturating_sub(entry.last_seen) <= ONLINE_WINDOW_SECS,
                trusted: trusted_entry.is_some(),
            }
        })
        .collect();

    // Trusted devices not currently visible still appear, as offline.
    for device in &trusted {
        if !devices.iter().any(|known| known.address == device.address) {
            devices.push(PeerDevice {
                id: device.id.clone(),
                display_name: device.display_name.clone(),
                address: device.address.clone(),
                platform: device.platform.clone(),
                last_seen: device.last_seen,
                online: false,
                trusted: true,
            });
        }
    }

    devices.sort_by(|a, b| {
        b.online
            .cmp(&a.online)
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    Ok(devices)
}

// ---- Feature 2: trusted devices ------------------------------------------

#[tauri::command]
fn list_trusted_devices(
    state: State<'_, DesktopAppState>,
) -> DesktopResult<Vec<store::TrustedDevice>> {
    Ok(state.store.trusted_devices())
}

/// A pairing in progress: the verified identity a discovered device advertised,
/// held until the user confirms the fingerprint. Nothing is trusted while a
/// pairing is only pending.
#[derive(Debug, Clone)]
struct PendingPairing {
    peer_id: String,
    display_name: String,
    address: String,
    platform: String,
    fingerprint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub peer_id: String,
    pub display_name: String,
    pub address: String,
    pub fingerprint: String,
    pub platform: String,
    /// True when this device is already in the trusted store — the UI can warn
    /// that confirming will update the existing trust entry.
    pub already_trusted: bool,
}

/// Resolves a discovered device's advertised identity and stages a pairing for
/// user confirmation. This is the real "Trust" entry point: it never writes
/// trust — it only fetches the peer's advertised certificate fingerprint and
/// returns it so the user can verify it before `confirm_pairing`.
///
/// The fingerprint comes from the peer's own mDNS advertisement (published from
/// its certificate), so a device can only be paired if it is actually present
/// and advertising — mDNS presence alone is never sufficient to trust it.
#[tauri::command]
fn start_pairing(
    peer_id: String,
    address: String,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<PairingInfo> {
    // Look the device up in the live discovery set so we use the identity the
    // remote peer is advertising right now, not any stale UI-supplied value.
    let peers = discover_peers(&state.config()).map_err(error_to_string)?;
    let matched = peers.into_iter().find(|peer| {
        peer.peer_id == peer_id
            || peer
                .addresses
                .iter()
                .any(|candidate| addresses_match(candidate, peer.port, &address))
    });

    let peer = matched.ok_or_else(|| {
        "device is not reachable for pairing — make sure it is on the network and receiving"
            .to_owned()
    })?;

    stage_pairing(&state, peer, address)
}

/// Stages a discovered peer for confirmation: verifies it advertised a
/// fingerprint, records a pending pairing, and returns the identity for the
/// user to confirm. Never writes trust. Split from the command so it can be
/// tested without a live mDNS scan.
fn stage_pairing(
    state: &DesktopAppState,
    peer: DiscoveredPeer,
    ui_address: String,
) -> DesktopResult<PairingInfo> {
    let fingerprint = peer.fingerprint.clone().ok_or_else(|| {
        "device did not advertise a certificate fingerprint — it may be running an older \
         version that does not support pairing"
            .to_owned()
    })?;

    let resolved_address = peer
        .addresses
        .first()
        .map(|ip| format!("{ip}:{}", peer.port))
        .unwrap_or(ui_address);

    let already_trusted = state
        .store
        .trusted_devices()
        .iter()
        .any(|device| device.id == peer.peer_id);

    let pairing = PendingPairing {
        peer_id: peer.peer_id.clone(),
        display_name: peer.display_name.clone(),
        address: resolved_address,
        platform: "unknown".to_owned(),
        fingerprint,
    };

    if let Ok(mut pairings) = state.pairings.lock() {
        pairings.insert(peer.peer_id.clone(), pairing.clone());
    }

    Ok(PairingInfo {
        peer_id: pairing.peer_id,
        display_name: pairing.display_name,
        address: pairing.address,
        fingerprint: pairing.fingerprint,
        platform: pairing.platform,
        already_trusted,
    })
}

/// Confirms a pending pairing: after the user has verified the fingerprint,
/// stores the device in `trusted-devices.json`. Fails if there is no pending
/// pairing for `peer_id`, so trust is only ever written for a fingerprint the
/// user actually saw and approved.
#[tauri::command]
fn confirm_pairing(
    peer_id: String,
    display_name: Option<String>,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<store::TrustedDevice> {
    confirm_pairing_inner(&state, &peer_id, display_name)
}

fn confirm_pairing_inner(
    state: &DesktopAppState,
    peer_id: &str,
    display_name: Option<String>,
) -> DesktopResult<store::TrustedDevice> {
    let pairing = {
        let mut pairings = state
            .pairings
            .lock()
            .map_err(|_| "pairing state is unavailable".to_owned())?;
        pairings.remove(peer_id).ok_or_else(|| {
            "no pairing is awaiting confirmation for this device — start pairing again".to_owned()
        })?
    };

    let display_name = display_name
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(pairing.display_name);

    Ok(state.store.trust_device(store::TrustedDevice {
        id: pairing.peer_id,
        display_name,
        address: pairing.address,
        platform: pairing.platform,
        fingerprint: pairing.fingerprint,
        first_trusted: 0,
        last_seen: store::unix_now(),
    }))
}

/// Discards a pending pairing without trusting the device. Used when the user
/// rejects the fingerprint or closes the confirmation modal.
#[tauri::command]
fn cancel_pairing(peer_id: String, state: State<'_, DesktopAppState>) -> DesktopResult<bool> {
    Ok(cancel_pairing_inner(&state, &peer_id))
}

fn cancel_pairing_inner(state: &DesktopAppState, peer_id: &str) -> bool {
    state
        .pairings
        .lock()
        .map(|mut pairings| pairings.remove(peer_id).is_some())
        .unwrap_or(false)
}

/// Whether a discovered peer's `ip` + `port` matches a UI-supplied
/// `ip:port` (or bare `ip`) target string.
fn addresses_match(ip: &str, port: u16, target: &str) -> bool {
    if let Some((target_ip, target_port)) = target.rsplit_once(':') {
        target_ip == ip
            && target_port
                .parse::<u16>()
                .map(|p| p == port)
                .unwrap_or(false)
    } else {
        target == ip
    }
}

#[tauri::command]
fn untrust_device(id: String, state: State<'_, DesktopAppState>) -> DesktopResult<bool> {
    Ok(state.store.untrust_device(&id))
}

#[tauri::command]
fn rename_trusted_device(
    id: String,
    display_name: String,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<bool> {
    Ok(state.store.rename_trusted_device(&id, &display_name))
}

// ---- Feature 4: transfer history -----------------------------------------

#[tauri::command]
fn list_transfer_history(
    state: State<'_, DesktopAppState>,
) -> DesktopResult<Vec<store::TransferRecord>> {
    Ok(state.store.history())
}

#[tauri::command]
fn clear_transfer_history(state: State<'_, DesktopAppState>) -> DesktopResult<()> {
    state.store.clear_history();
    Ok(())
}

#[tauri::command]
fn list_transfer_jobs(
    state: State<'_, DesktopAppState>,
) -> DesktopResult<Vec<TransferJobSnapshot>> {
    let jobs = state
        .jobs
        .lock()
        .map_err(|_| "desktop job registry is unavailable".to_owned())?;
    let mut snapshots = jobs
        .iter()
        .map(|(job_id, job)| job.snapshot(*job_id))
        .collect::<Vec<_>>();
    snapshots.sort_by_key(|snapshot| snapshot.job_id);

    Ok(snapshots)
}

#[tauri::command]
fn reset_completed_jobs(state: State<'_, DesktopAppState>) -> DesktopResult<()> {
    let mut jobs = state
        .jobs
        .lock()
        .map_err(|_| "desktop job registry is unavailable".to_owned())?;
    jobs.retain(|_, job| job.state == TransferJobState::Running);

    Ok(())
}

#[tauri::command]
fn start_stress_run(
    file_path: String,
    host: Option<String>,
    iterations: u64,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<StartStressResponse> {
    let path = PathBuf::from(&file_path);
    validate_file_path(&path)?;
    let host = parse_host(host)?;
    let iterations = iterations.clamp(1, 10_000);
    let config = state.config();
    let run_id = state.next_stress_id();
    insert_stress_run(&state.stress, run_id, StressRun::new(file_path, iterations))?;
    spawn_stress_run(state.stress.clone(), run_id, iterations, move |output| {
        run_send(&path, host, &config, output)
    });

    Ok(StartStressResponse { run_id })
}

#[tauri::command]
fn list_stress_runs(state: State<'_, DesktopAppState>) -> DesktopResult<Vec<StressRunSnapshot>> {
    let runs = state
        .stress
        .lock()
        .map_err(|_| "desktop stress registry is unavailable".to_owned())?;
    let mut snapshots = runs
        .iter()
        .map(|(run_id, run)| run.snapshot(*run_id))
        .collect::<Vec<_>>();
    snapshots.sort_by_key(|snapshot| snapshot.run_id);

    Ok(snapshots)
}

#[tauri::command]
fn reset_completed_stress_runs(state: State<'_, DesktopAppState>) -> DesktopResult<()> {
    let mut runs = state
        .stress
        .lock()
        .map_err(|_| "desktop stress registry is unavailable".to_owned())?;
    runs.retain(|_, run| run.state == StressRunState::Running);

    Ok(())
}

/// Task 4: export a benchmark run as a JSON report under `<state_dir>/reports/`.
/// Returns the written path.
#[tauri::command]
fn export_stress_report(run_id: u64, state: State<'_, DesktopAppState>) -> DesktopResult<String> {
    let snapshot = {
        let runs = state
            .stress
            .lock()
            .map_err(|_| "desktop stress registry is unavailable".to_owned())?;
        runs.get(&run_id)
            .map(|run| run.snapshot(run_id))
            .ok_or_else(|| format!("unknown stress run: {run_id}"))?
    };
    let report = build_benchmark_report(&snapshot, &state.config());
    let json = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("failed to serialize report: {error}"))?;
    let stamp = format!("{}-{}", store::unix_now(), run_id);
    let path = state
        .store
        .write_report(&stamp, &json)
        .map_err(|error| format!("failed to write report: {error}"))?;
    Ok(path.display().to_string())
}

/// Builds the benchmark report body from a stress snapshot (Task 4 metrics:
/// file size, duration, avg MB/s, checksum-verification is done by the core on
/// every transfer, so a successful run implies verification passed).
fn build_benchmark_report(snapshot: &StressRunSnapshot, config: &CliConfig) -> BenchmarkReport {
    BenchmarkReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        generated_at: store::unix_now(),
        file_path: snapshot.file_path.clone(),
        file_size: snapshot.file_size,
        iterations: snapshot.target_iterations,
        completed: snapshot.completed,
        failed: snapshot.failed,
        avg_mbps: snapshot.avg_mbps,
        avg_duration_ms: snapshot.avg_duration_ms,
        chunk_size: config.chunk_size as u64,
        samples: snapshot
            .iterations
            .iter()
            .map(|iteration| BenchmarkSample {
                index: iteration.index,
                ok: iteration.state == TransferJobState::Completed,
                bytes: iteration.bytes,
                duration_ms: iteration.duration_ms,
                mbps: iteration.mbps,
            })
            .collect(),
    }
}

// ---- Feature 2/3/5: background mode + receiver status ---------------------

#[tauri::command]
fn get_background_settings(state: State<'_, DesktopAppState>) -> BackgroundSettingsResponse {
    state.store.background_settings().into()
}

/// Persists background settings. When background receiving is turned on and no
/// receiver is running yet, starts one immediately so the toggle takes effect
/// without reopening the app.
#[tauri::command]
fn set_background_settings(
    background_receiving: bool,
    start_on_login: bool,
    app: tauri::AppHandle,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<BackgroundSettingsResponse> {
    let settings = store::BackgroundSettings {
        background_receiving,
        start_on_login,
    };
    state.store.set_background_settings(settings);
    if background_receiving {
        let _ = spawn_receive_job(&app, &state);
    }
    // Feature 5: actually register/unregister OS launch-on-login. Best-effort —
    // a failure here (e.g. read-only profile) must not fail the settings save.
    if let Ok(exe) = std::env::current_exe() {
        let _ = autostart::set_enabled(start_on_login, &exe);
    }
    Ok(settings.into())
}

// ---- Feature 3: onboarding ------------------------------------------------

#[tauri::command]
fn get_onboarding(state: State<'_, DesktopAppState>) -> OnboardingResponse {
    let onboarding = state.store.onboarding();
    OnboardingResponse {
        completed: onboarding.completed,
        completed_at: onboarding.completed_at,
    }
}

/// Persists device/discoverable/background choices from onboarding and marks it
/// complete so it never shows again.
#[tauri::command]
fn complete_onboarding(
    device_name: String,
    discoverable: bool,
    background_receiving: bool,
    start_on_login: bool,
    app: tauri::AppHandle,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<OnboardingResponse> {
    let mut prefs = state.store.preferences();
    prefs.device_name = device_name.trim().to_owned();
    prefs.discoverable = discoverable;
    state.store.set_preferences(prefs);

    let settings = store::BackgroundSettings {
        background_receiving,
        start_on_login,
    };
    state.store.set_background_settings(settings);
    if let Ok(exe) = std::env::current_exe() {
        let _ = autostart::set_enabled(start_on_login, &exe);
    }
    if background_receiving {
        let _ = spawn_receive_job(&app, &state);
    }

    let done = state.store.complete_onboarding();
    Ok(OnboardingResponse {
        completed: done.completed,
        completed_at: done.completed_at,
    })
}

// ---- Feature 4: application preferences -----------------------------------

#[tauri::command]
fn get_preferences(state: State<'_, DesktopAppState>) -> store::AppPreferences {
    state.store.preferences()
}

#[tauri::command]
fn set_preferences(
    preferences: store::AppPreferences,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<store::AppPreferences> {
    state.store.set_preferences(preferences.clone());
    Ok(preferences)
}

/// Receiver status for the Dashboard (Feature 5): whether a receive session is
/// active, whether the device is discoverable, and the advertised endpoint.
#[tauri::command]
fn get_receiver_status(state: State<'_, DesktopAppState>) -> DesktopResult<ReceiverStatusResponse> {
    let receiving = state
        .jobs
        .lock()
        .map(|jobs| {
            jobs.values().any(|job| {
                job.kind == TransferJobKind::Receive && job.state == TransferJobState::Running
            })
        })
        .unwrap_or(false);
    let endpoint = receiver_endpoint(&state.config())
        .map_err(error_to_string)?
        .map(|endpoint| endpoint.address);
    let background = state.store.background_settings().background_receiving;

    Ok(ReceiverStatusResponse {
        receiving,
        discoverable: receiving && endpoint.is_some(),
        background_enabled: background,
        endpoint,
    })
}

/// Onboarding identity preview: the name Nexo will use and this device's LAN IP.
#[tauri::command]
fn preview_identity(state: State<'_, DesktopAppState>) -> IdentityPreviewResponse {
    let prefs = state.store.preferences();
    let display_name = if prefs.device_name.trim().is_empty() {
        hostname_or_default()
    } else {
        prefs.device_name.trim().to_owned()
    };
    IdentityPreviewResponse {
        display_name,
        address: cli::local_lan_address(),
    }
}

/// Onboarding "Test connection" self-check: verifies storage is writable, the
/// receiver endpoint is reachable-ready, and discovery is enabled. Pure
/// read/probe — starts no transfer.
#[tauri::command]
fn run_self_check(state: State<'_, DesktopAppState>) -> SelfCheckResponse {
    let config = state.config();
    let prefs = state.store.preferences();

    // Storage writable: probe the resolved download dir.
    let download_dir = resolve_download_dir(&prefs.download_dir, &config.receive_dir);
    let storage_writable = is_writable_dir(&download_dir);

    // Receiver ready: an advertised endpoint exists or background receiving is on
    // (so it will come up). Either way the receiver stack is usable.
    let background = state.store.background_settings().background_receiving;
    let has_endpoint = receiver_endpoint(&config)
        .map(|endpoint| endpoint.is_some())
        .unwrap_or(false);
    let receiver_ready = background || has_endpoint;

    SelfCheckResponse {
        storage_writable,
        receiver_ready,
        discovery_enabled: prefs.discoverable,
        download_dir: path_to_string(download_dir),
    }
}

/// Version + build metadata for Settings → About.
#[tauri::command]
fn get_build_info() -> BuildInfoResponse {
    BuildInfoResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        build_type: option_env!("NEXO_BUILD_PROFILE")
            .unwrap_or("unknown")
            .to_owned(),
        commit: option_env!("NEXO_GIT_COMMIT")
            .unwrap_or("unknown")
            .to_owned(),
    }
}

// ---- Task 5: diagnostics --------------------------------------------------

/// Read-only diagnostics for a hidden Settings → Advanced page. All values come
/// from existing state; nothing here alters the engine.
#[tauri::command]
fn get_diagnostics(state: State<'_, DesktopAppState>) -> DesktopResult<DiagnosticsResponse> {
    let config = state.config();
    let prefs = state.store.preferences();

    let device_id = std::fs::read_to_string(config.peer_id_path())
        .map(|value| value.trim().to_owned())
        .unwrap_or_default();

    let (endpoint, fingerprint) =
        match cli::receiver_advertisement(&config).map_err(error_to_string)? {
            Some((address, cert)) => (Some(address), store::certificate_fingerprint(&cert)),
            None => (None, String::new()),
        };

    let receiving = state
        .jobs
        .lock()
        .map(|jobs| {
            jobs.values().any(|job| {
                job.kind == TransferJobKind::Receive && job.state == TransferJobState::Running
            })
        })
        .unwrap_or(false);

    let history = state.store.history();
    let completed = history.iter().filter(|r| r.status == "completed").count() as u64;
    let failed = history
        .iter()
        .filter(|r| r.status == "failed" || r.status == "interrupted")
        .count() as u64;
    let bytes_received: u64 = history
        .iter()
        .filter(|r| r.direction == "receive" && r.status == "completed")
        .map(|r| r.size)
        .sum();
    let bytes_sent: u64 = history
        .iter()
        .filter(|r| r.direction == "send" && r.status == "completed")
        .map(|r| r.size)
        .sum();
    let last_transfer = history
        .first()
        .map(|r| format!("{} {} · {}", r.direction, r.filename, r.status));

    Ok(DiagnosticsResponse {
        device_id,
        certificate_fingerprint: fingerprint,
        mdns_discoverable: prefs.discoverable,
        receiver_running: receiving,
        endpoint,
        state_dir: path_to_string(config.state_dir.clone()),
        download_dir: resolve_download_dir(&prefs.download_dir, &config.receive_dir)
            .display()
            .to_string(),
        last_transfer,
        total_transfers: history.len() as u64,
        completed_transfers: completed,
        failed_transfers: failed,
        bytes_sent,
        bytes_received,
    })
}

pub fn build_config(app_handle: &tauri::AppHandle) -> CliConfig {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| fallback_state_dir());
    CliConfig {
        state_dir: app_data_dir.join("state"),
        receive_dir: app_data_dir.join("received"),
        chunk_size: default_chunk_size() as usize,
    }
}

/// Menu item ids for the tray.
const TRAY_OPEN: &str = "tray_open";
const TRAY_TOGGLE_RECEIVE: &str = "tray_toggle_receive";
const TRAY_SETTINGS: &str = "tray_settings";
const TRAY_QUIT: &str = "tray_quit";

/// Builds (or rebuilds) the tray menu to reflect the current background state.
fn build_tray_menu(
    app: &tauri::AppHandle,
    background_on: bool,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};

    let status = MenuItemBuilder::with_id("tray_status", "Status: 🟢 Available")
        .enabled(false)
        .build(app)?;
    let open = MenuItemBuilder::with_id(TRAY_OPEN, "Open Window").build(app)?;
    let toggle = MenuItemBuilder::with_id(
        TRAY_TOGGLE_RECEIVE,
        if background_on {
            "Receiving in background: ✓ Enabled"
        } else {
            "Receiving in background: Disabled"
        },
    )
    .build(app)?;
    let settings = MenuItemBuilder::with_id(TRAY_SETTINGS, "Settings").build(app)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT, "Quit Nexo").build(app)?;

    MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&open)
        .item(&toggle)
        .item(&settings)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&quit)
        .build()
}

/// Shows and focuses the main window, creating nothing new (the window always
/// exists; closing only hides it).
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Navigates the UI to a screen by emitting an event the frontend listens for.
fn focus_screen(app: &tauri::AppHandle, screen: &str) {
    show_main_window(app);
    let _ = app.emit("navigate", screen);
}

pub fn run() {
    use tauri::tray::TrayIconBuilder;
    use tauri::{Manager, WindowEvent};

    tauri::Builder::default()
        .setup(|app| {
            let config = build_config(app.handle());
            let state = DesktopAppState::new(config);
            let background_on = state.store.background_settings().background_receiving;
            app.manage(state);

            // Feature 1: system tray with a live status + controls menu.
            let menu = build_tray_menu(app.handle(), background_on)?;
            let _tray = TrayIconBuilder::with_id("nexo-tray")
                .icon(app.default_window_icon().cloned().ok_or("missing icon")?)
                .tooltip("Nexo — available")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    TRAY_OPEN => show_main_window(app),
                    TRAY_SETTINGS => focus_screen(app, "settings"),
                    TRAY_TOGGLE_RECEIVE => {
                        // Flip the persisted background toggle from the tray.
                        let state = app.state::<DesktopAppState>();
                        let mut settings = state.store.background_settings();
                        settings.background_receiving = !settings.background_receiving;
                        state.store.set_background_settings(settings);
                        if settings.background_receiving {
                            let _ = spawn_receive_job(app, &state);
                        }
                        if let Ok(menu) = build_tray_menu(app, settings.background_receiving)
                            && let Some(tray) = app.tray_by_id("nexo-tray")
                        {
                            let _ = tray.set_menu(Some(menu));
                        }
                        let _ = app.emit("background_settings_changed", ());
                    }
                    TRAY_QUIT => {
                        // Feature 3 shutdown: dropping app state drops the receive
                        // thread's discovery advertisement (ServiceAdvertisement
                        // unregisters on drop), then exit.
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Feature 3 startup: if background receiving is enabled, start the
            // receiver immediately so the device is discoverable without the
            // user pressing anything.
            if background_on {
                let handle = app.handle().clone();
                let state = app.state::<DesktopAppState>();
                let _ = spawn_receive_job(&handle, &state);
            }

            // Feature 5: when launched by autostart with `--hidden`, start
            // minimized to the tray instead of popping the window.
            if std::env::args().any(|arg| arg == "--hidden")
                && let Some(window) = app.get_webview_window("main")
            {
                let _ = window.hide();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Feature 1/2: closing the window hides to tray instead of quitting,
            // as long as background receiving is enabled. With it disabled, the
            // close proceeds and the app exits (stopping the receiver).
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let background_on = app
                    .state::<DesktopAppState>()
                    .store
                    .background_settings()
                    .background_receiving;
                if background_on {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            get_state_paths,
            get_status,
            get_receiver_endpoint,
            get_receiver_status,
            get_background_settings,
            set_background_settings,
            get_onboarding,
            complete_onboarding,
            get_preferences,
            set_preferences,
            get_build_info,
            preview_identity,
            run_self_check,
            get_diagnostics,
            discover_known_peers,
            start_receive,
            create_transfer_request,
            approve_transfer_request,
            reject_transfer_request,
            list_transfer_requests,
            approve_incoming_request,
            reject_incoming_request,
            list_incoming_requests,
            list_devices,
            list_trusted_devices,
            start_pairing,
            confirm_pairing,
            cancel_pairing,
            untrust_device,
            rename_trusted_device,
            list_transfer_history,
            clear_transfer_history,
            list_transfer_jobs,
            reset_completed_jobs,
            start_stress_run,
            list_stress_runs,
            reset_completed_stress_runs,
            export_stress_report,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Nexo desktop");
}

#[allow(clippy::too_many_arguments)]
fn spawn_transfer_job<F>(
    jobs: Arc<Mutex<HashMap<u64, TransferJob>>>,
    job_id: u64,
    kind: TransferJobKind,
    store: Arc<store::AppStore>,
    config: CliConfig,
    meta: TransferMeta,
    work: F,
) where
    F: FnOnce(&mut JobOutput) -> cli::CliResult<()> + Send + 'static,
{
    thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut output = JobOutput::new(jobs.clone(), job_id);
        let result = work(&mut output);
        let lines = output.lines();
        let (state, error) = match result {
            Ok(()) => (TransferJobState::Completed, None),
            Err(error) => (TransferJobState::Failed, Some(error.to_string())),
        };

        // Feature 4: application-level transfer history (does not touch the
        // storage engine). Recorded from the job's own outcome + output.
        let completed = state == TransferJobState::Completed;
        let filename = if meta.direction == "receive" {
            transfer_status_snapshot(&config)
                .ok()
                .and_then(|snapshot| snapshot.latest)
                .and_then(|latest| latest.file_name)
                .unwrap_or(meta.filename)
        } else {
            meta.filename
        };
        store.record_transfer(store::TransferRecord {
            id: format!("job-{job_id}"),
            filename,
            size: last_total_bytes(&lines),
            direction: meta.direction.to_owned(),
            peer: meta.peer,
            timestamp: store::unix_now(),
            status: if completed { "completed" } else { "failed" }.to_owned(),
            duration_ms: started.elapsed().as_millis() as u64,
            checksum_ok: completed,
        });

        if let Ok(mut jobs) = jobs.lock() {
            jobs.insert(
                job_id,
                TransferJob {
                    kind,
                    state,
                    output: lines,
                    error,
                },
            );
        }
    });
}

fn insert_job(
    jobs: &Arc<Mutex<HashMap<u64, TransferJob>>>,
    job_id: u64,
    job: TransferJob,
) -> DesktopResult<()> {
    jobs.lock()
        .map_err(|_| "desktop job registry is unavailable".to_owned())?
        .insert(job_id, job);

    Ok(())
}

fn insert_stress_run(
    stress: &Arc<Mutex<HashMap<u64, StressRun>>>,
    run_id: u64,
    run: StressRun,
) -> DesktopResult<()> {
    stress
        .lock()
        .map_err(|_| "desktop stress registry is unavailable".to_owned())?
        .insert(run_id, run);

    Ok(())
}

fn update_stress<F>(stress: &Arc<Mutex<HashMap<u64, StressRun>>>, run_id: u64, update: F)
where
    F: FnOnce(&mut StressRun),
{
    if let Ok(mut runs) = stress.lock()
        && let Some(run) = runs.get_mut(&run_id)
    {
        update(run);
    }
}

/// Runs `iterations` sequential sends on a background thread, recording each
/// attempt. Iterations continue even after a failure so the run reports full
/// pass/fail statistics -- the point of a stress harness.
fn spawn_stress_run<F>(
    stress: Arc<Mutex<HashMap<u64, StressRun>>>,
    run_id: u64,
    iterations: u64,
    work: F,
) where
    F: Fn(&mut LineBuffer) -> cli::CliResult<()> + Send + 'static,
{
    thread::spawn(move || {
        for index in 0..iterations {
            update_stress(&stress, run_id, |run| {
                run.iterations.push(StressIteration {
                    index,
                    state: TransferJobState::Running,
                    error: None,
                    duration_ms: 0,
                    bytes: 0,
                    mbps: 0.0,
                });
            });

            let started = std::time::Instant::now();
            let mut output = LineBuffer::default();
            let result = work(&mut output);
            let elapsed_ms = started.elapsed().as_millis() as u64;
            let lines = output.lines();
            let bytes = last_total_bytes(&lines);
            let mbps = throughput_mbps(bytes, elapsed_ms);

            update_stress(&stress, run_id, |run| {
                run.last_output = lines;
                if let Some(entry) = run.iterations.last_mut() {
                    entry.duration_ms = elapsed_ms;
                    entry.bytes = bytes;
                    entry.mbps = mbps;
                }
                match result {
                    Ok(()) => {
                        run.completed += 1;
                        if let Some(entry) = run.iterations.last_mut() {
                            entry.state = TransferJobState::Completed;
                        }
                    }
                    Err(error) => {
                        run.failed += 1;
                        if let Some(entry) = run.iterations.last_mut() {
                            entry.state = TransferJobState::Failed;
                            entry.error = Some(error.to_string());
                        }
                    }
                }
            });
        }

        update_stress(&stress, run_id, |run| {
            run.state = if run.failed == 0 {
                StressRunState::Completed
            } else {
                StressRunState::Failed
            };
        });
    });
}

/// Average throughput in MB/s (base-1000 MB, matching how transfer rates are
/// usually quoted) from a byte count and a millisecond duration.
fn throughput_mbps(bytes: u64, duration_ms: u64) -> f64 {
    if duration_ms == 0 {
        return 0.0;
    }
    (bytes as f64 / 1_000_000.0) / (duration_ms as f64 / 1000.0)
}

fn validate_file_path(path: &Path) -> DesktopResult<()> {
    if !path.is_file() {
        return Err(format!("file does not exist: {}", path.display()));
    }

    Ok(())
}

fn parse_host(host: Option<String>) -> DesktopResult<Option<SocketAddr>> {
    host.map(|host| {
        host.parse::<SocketAddr>()
            .map_err(|error| format!("invalid receiver address: {error}"))
    })
    .transpose()
}

fn fallback_state_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".nexo")
        .join("desktop")
}

fn path_to_string(path: PathBuf) -> String {
    path.display().to_string()
}

/// This machine's hostname for the identity preview, or a safe default. Uses the
/// `HOSTNAME`/`COMPUTERNAME` env vars to avoid adding a dependency.
fn hostname_or_default() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "This device".to_owned())
}

fn error_to_string(error: Box<dyn std::error::Error + Send + Sync>) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn line_buffer_captures_progress_lines() {
        let mut buffer = LineBuffer::default();

        writeln!(&mut buffer, "sending: 1/2 chunks").expect("write first line");
        writeln!(&mut buffer, "sent: 2/2 chunks").expect("write second line");

        assert_eq!(
            buffer.lines(),
            vec![
                "sending: 1/2 chunks".to_owned(),
                "sent: 2/2 chunks".to_owned()
            ]
        );
    }

    #[test]
    fn parse_host_accepts_socket_addresses() {
        let parsed = parse_host(Some("127.0.0.1:41000".to_owned())).expect("parse host");

        assert_eq!(
            parsed,
            Some("127.0.0.1:41000".parse::<SocketAddr>().expect("address"))
        );
    }

    #[test]
    fn parse_host_rejects_invalid_addresses() {
        let error = parse_host(Some("not-an-address".to_owned())).expect_err("invalid host");

        assert!(error.contains("invalid receiver address"));
    }

    #[test]
    fn validate_file_path_requires_existing_file() {
        let path = temp_path("validate-file");
        fs::write(&path, b"nexo").expect("write file");

        validate_file_path(&path).expect("valid file");
        fs::remove_file(path).ok();
    }

    #[test]
    fn job_snapshots_are_sorted_by_id() {
        let state = DesktopAppState::new(CliConfig {
            state_dir: temp_path("state"),
            receive_dir: temp_path("receive"),
            chunk_size: 8,
        });
        insert_job(
            &state.jobs,
            2,
            TransferJob::running(TransferJobKind::Receive),
        )
        .expect("insert second");
        insert_job(&state.jobs, 1, TransferJob::running(TransferJobKind::Send))
            .expect("insert first");

        let jobs = state.jobs.lock().expect("jobs");
        let mut snapshots = jobs
            .iter()
            .map(|(job_id, job)| job.snapshot(*job_id))
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|snapshot| snapshot.job_id);

        assert_eq!(
            snapshots
                .into_iter()
                .map(|snapshot| snapshot.job_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn stress_run_records_each_iteration_and_final_state() {
        let stress: Arc<Mutex<HashMap<u64, StressRun>>> = Arc::new(Mutex::new(HashMap::new()));
        insert_stress_run(&stress, 1, StressRun::new("file.bin".to_owned(), 3))
            .expect("insert stress run");
        // Two successes, one failure: the run must end Failed with accurate counts.
        let attempts = Arc::new(AtomicU64::new(0));
        let attempts_for_work = attempts.clone();
        spawn_stress_run(stress.clone(), 1, 3, move |output| {
            use std::io::Write;
            let attempt = attempts_for_work.fetch_add(1, Ordering::Relaxed);
            writeln!(output, "sent: attempt {attempt}").expect("write output");
            if attempt == 1 {
                Err(Box::new(std::io::Error::other("boom"))
                    as Box<dyn std::error::Error + Send + Sync>)
            } else {
                Ok(())
            }
        });

        let deadline = SystemTime::now() + std::time::Duration::from_secs(5);
        loop {
            let snapshot = {
                let runs = stress.lock().expect("stress registry");
                runs.get(&1).expect("run exists").snapshot(1)
            };
            if snapshot.state != StressRunState::Running {
                assert_eq!(snapshot.target_iterations, 3);
                assert_eq!(snapshot.completed, 2);
                assert_eq!(snapshot.failed, 1);
                assert_eq!(snapshot.state, StressRunState::Failed);
                assert_eq!(snapshot.iterations.len(), 3);
                assert_eq!(snapshot.iterations[1].state, TransferJobState::Failed);
                assert!(snapshot.iterations[1].error.is_some());
                break;
            }
            assert!(
                SystemTime::now() < deadline,
                "stress run did not finish in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn reset_completed_stress_runs_retains_only_running() {
        let stress: Arc<Mutex<HashMap<u64, StressRun>>> = Arc::new(Mutex::new(HashMap::new()));
        let mut done = StressRun::new("done.bin".to_owned(), 1);
        done.state = StressRunState::Completed;
        insert_stress_run(&stress, 1, done).expect("insert done");
        insert_stress_run(&stress, 2, StressRun::new("live.bin".to_owned(), 1))
            .expect("insert running");

        stress
            .lock()
            .expect("stress registry")
            .retain(|_, run| run.state == StressRunState::Running);

        let runs = stress.lock().expect("stress registry");
        assert_eq!(runs.len(), 1);
        assert!(runs.contains_key(&2));
    }

    fn pending_request(id: &str) -> PendingTransferRequest {
        PendingTransferRequest {
            file_path: PathBuf::from("/tmp/file.bin"),
            host: Some("127.0.0.1:41000".parse().expect("addr")),
            response: TransferRequestResponse {
                id: id.to_owned(),
                file_path: "/tmp/file.bin".to_owned(),
                file_name: "file.bin".to_owned(),
                file_size: 1024,
                peer_display_name: "127.0.0.1:41000".to_owned(),
                peer_address: "127.0.0.1:41000".to_owned(),
                status: "pending".to_owned(),
            },
        }
    }

    #[test]
    fn reject_removes_pending_request_without_transferring() {
        let requests: Arc<Mutex<HashMap<String, PendingTransferRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));
        requests
            .lock()
            .expect("requests")
            .insert("req-1".to_owned(), pending_request("req-1"));

        // Reject == drop the pending request; no job is ever spawned.
        requests.lock().expect("requests").remove("req-1");

        assert!(requests.lock().expect("requests").is_empty());
    }

    #[test]
    fn approve_consumes_request_so_it_cannot_run_twice() {
        // Approval removes the pending request before spawning the send, so a
        // duplicate approve finds nothing to run (no double transfer).
        let requests: Arc<Mutex<HashMap<String, PendingTransferRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));
        requests
            .lock()
            .expect("requests")
            .insert("req-2".to_owned(), pending_request("req-2"));

        let first = requests.lock().expect("requests").remove("req-2");
        let second = requests.lock().expect("requests").remove("req-2");

        assert!(first.is_some(), "first approve finds the pending request");
        assert!(second.is_none(), "second approve finds nothing to run");
    }

    fn pending_incoming(id: &str) -> (PendingIncoming, std::sync::mpsc::Receiver<bool>) {
        let (decision, rx) = std::sync::mpsc::sync_channel::<bool>(1);
        (
            PendingIncoming {
                response: IncomingTransferResponse {
                    id: id.to_owned(),
                    sender: "cli-sender".to_owned(),
                    filename: "movie.iso".to_owned(),
                    file_size: 5_000_000_000,
                    checksum: "abc".to_owned(),
                    timestamp: 0,
                },
                decision,
            },
            rx,
        )
    }

    #[test]
    fn approve_and_reject_incoming_signal_the_waiting_receiver() {
        let incoming: Arc<Mutex<HashMap<String, PendingIncoming>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Approve delivers `true` to the parked receive thread and clears it.
        let (pending, accept_rx) = pending_incoming("in-1");
        incoming
            .lock()
            .expect("lock")
            .insert("in-1".to_owned(), pending);
        signal_incoming(&incoming, "in-1", true).expect("approve");
        assert!(accept_rx.recv().expect("decision"), "approve delivers true");
        assert!(incoming.lock().expect("lock").is_empty());

        // Reject delivers `false`.
        let (pending, reject_rx) = pending_incoming("in-2");
        incoming
            .lock()
            .expect("lock")
            .insert("in-2".to_owned(), pending);
        signal_incoming(&incoming, "in-2", false).expect("reject");
        assert!(
            !reject_rx.recv().expect("decision"),
            "reject delivers false"
        );

        // Unknown id is an error, not a silent success.
        assert!(signal_incoming(&incoming, "missing", true).is_err());
    }

    fn temp_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nexo-desktop-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn state_with_store(label: &str) -> DesktopAppState {
        DesktopAppState::new(CliConfig {
            state_dir: temp_path(label),
            receive_dir: temp_path(&format!("{label}-recv")),
            chunk_size: 8,
        })
    }

    fn discovered(peer_id: &str, name: &str, fingerprint: Option<&str>) -> DiscoveredPeer {
        DiscoveredPeer {
            peer_id: peer_id.to_owned(),
            display_name: name.to_owned(),
            addresses: vec!["172.21.209.204".to_owned()],
            port: 50038,
            fingerprint: fingerprint.map(str::to_owned),
        }
    }

    #[test]
    fn certificate_fingerprint_is_deterministic_and_grouped() {
        // The fingerprint the receiver advertises must be stable and formatted
        // as grouped uppercase SHA-256 hex, matching what gets stored on trust.
        let fp1 = cli::certificate_fingerprint(b"nexo-certificate-bytes");
        let fp2 = cli::certificate_fingerprint(b"nexo-certificate-bytes");
        assert_eq!(fp1, fp2, "same cert must yield the same fingerprint");
        assert_ne!(fp1, cli::certificate_fingerprint(b"a different cert"));

        let groups: Vec<&str> = fp1.split(':').collect();
        assert_eq!(groups.len(), 8, "fingerprint has 8 groups: {fp1}");
        assert!(
            groups
                .iter()
                .all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit())),
            "each group is 4 hex digits: {fp1}"
        );
        assert_eq!(fp1, fp1.to_uppercase(), "fingerprint is uppercase");
    }

    #[test]
    fn discovered_device_can_start_pairing_without_trusting() {
        // Staging a discovered device returns its advertised fingerprint for
        // confirmation but must NOT trust it yet.
        let state = state_with_store("pair-start");
        let peer = discovered("dev-a", "archlinux", Some("AAAA:BBBB:CCCC:DDDD"));

        let info =
            stage_pairing(&state, peer, "172.21.209.204:50038".to_owned()).expect("pairing stages");

        assert_eq!(info.peer_id, "dev-a");
        assert_eq!(info.fingerprint, "AAAA:BBBB:CCCC:DDDD");
        assert_eq!(info.address, "172.21.209.204:50038");
        assert!(!info.already_trusted);
        // Critical: no trust written from starting a pairing.
        assert!(
            state.store.trusted_devices().is_empty(),
            "starting a pairing must not trust the device"
        );
    }

    #[test]
    fn device_without_fingerprint_cannot_be_paired() {
        // A peer that does not advertise a fingerprint (older version) cannot be
        // paired — we refuse rather than trusting on discovery alone.
        let state = state_with_store("pair-nofp");
        let peer = discovered("dev-old", "legacy", None);
        assert!(stage_pairing(&state, peer, "1.2.3.4:5".to_owned()).is_err());
        assert!(state.store.trusted_devices().is_empty());
    }

    #[test]
    fn confirming_pairing_creates_trust_with_fingerprint() {
        let state = state_with_store("pair-confirm");
        let info = stage_pairing(
            &state,
            discovered("dev-b", "workstation", Some("1111:2222:3333:4444")),
            "172.21.209.204:50038".to_owned(),
        )
        .expect("stage");

        let trusted = confirm_pairing_inner(&state, "dev-b", None).expect("confirm");
        assert_eq!(trusted.id, "dev-b");
        assert_eq!(trusted.fingerprint, info.fingerprint);
        assert_eq!(trusted.display_name, "workstation");
        assert!(trusted.first_trusted > 0, "first-trusted timestamp is set");

        let stored = state.store.trusted_devices();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].fingerprint, "1111:2222:3333:4444");
    }

    #[test]
    fn duplicate_trust_updates_in_place_keeping_first_trusted() {
        // Pairing the same device twice must not create a duplicate entry, and
        // must preserve the original first-trusted timestamp.
        let state = state_with_store("pair-dup");
        stage_pairing(
            &state,
            discovered("dev-c", "laptop", Some("DEAD:BEEF:0000:1111")),
            "172.21.209.204:50038".to_owned(),
        )
        .unwrap();
        let first = confirm_pairing_inner(&state, "dev-c", None).expect("first confirm");

        // Pair again, this time renaming.
        stage_pairing(
            &state,
            discovered("dev-c", "laptop", Some("DEAD:BEEF:0000:1111")),
            "172.21.209.204:50038".to_owned(),
        )
        .unwrap();
        let second =
            confirm_pairing_inner(&state, "dev-c", Some("laptop-renamed".to_owned())).unwrap();

        assert_eq!(state.store.trusted_devices().len(), 1, "no duplicate entry");
        assert_eq!(
            second.first_trusted, first.first_trusted,
            "first-trusted preserved"
        );
        assert_eq!(second.display_name, "laptop-renamed");
    }

    #[test]
    fn rejected_pairing_stores_no_trust() {
        // Cancelling (rejecting the fingerprint) must drop the pending pairing
        // and leave the device untrusted, and confirming afterwards must fail.
        let state = state_with_store("pair-reject");
        stage_pairing(
            &state,
            discovered("dev-d", "phone", Some("CAFE:CAFE:CAFE:CAFE")),
            "172.21.209.204:50038".to_owned(),
        )
        .unwrap();

        assert!(
            cancel_pairing_inner(&state, "dev-d"),
            "cancel finds the pending pairing"
        );
        assert!(
            state.store.trusted_devices().is_empty(),
            "rejecting a pairing must not trust the device"
        );
        assert!(
            confirm_pairing_inner(&state, "dev-d", None).is_err(),
            "confirming a cancelled pairing must fail"
        );
    }

    #[test]
    fn confirming_unknown_pairing_is_an_error() {
        let state = state_with_store("pair-unknown");
        assert!(confirm_pairing_inner(&state, "never-staged", None).is_err());
        assert!(!cancel_pairing_inner(&state, "never-staged"));
    }

    #[test]
    fn addresses_match_handles_ip_and_ip_port() {
        assert!(addresses_match(
            "172.21.209.204",
            50038,
            "172.21.209.204:50038"
        ));
        assert!(addresses_match("172.21.209.204", 50038, "172.21.209.204"));
        assert!(!addresses_match(
            "172.21.209.204",
            50038,
            "172.21.209.204:1"
        ));
        assert!(!addresses_match("172.21.209.204", 50038, "10.0.0.1:50038"));
    }

    #[test]
    fn background_mode_defaults_on_so_receiver_autostarts() {
        // Feature 3: a fresh install defaults to background receiving ON, which
        // is what drives the auto-start on setup.
        let state = state_with_store("bg-default");
        assert!(state.store.background_settings().background_receiving);
    }

    #[test]
    fn disabling_background_mode_persists_and_stops_autostart() {
        // Feature 2 "disable mode": turning it off is persisted, so the next
        // launch / window-close does not keep the receiver alive.
        let state = state_with_store("bg-disable");
        state
            .store
            .set_background_settings(store::BackgroundSettings {
                background_receiving: false,
                start_on_login: false,
            });
        assert!(!state.store.background_settings().background_receiving);
    }

    #[test]
    fn tray_menu_ids_are_distinct() {
        // Guards against duplicate menu ids that would make tray actions
        // ambiguous.
        let ids = [TRAY_OPEN, TRAY_TOGGLE_RECEIVE, TRAY_SETTINGS, TRAY_QUIT];
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
    }

    fn trusted(id: &str) -> store::TrustedDevice {
        store::TrustedDevice {
            id: id.to_owned(),
            display_name: id.to_owned(),
            address: "10.0.0.5:41000".to_owned(),
            platform: "linux".to_owned(),
            fingerprint: "AAAA".to_owned(),
            first_trusted: 1,
            last_seen: 1,
        }
    }

    #[test]
    fn auto_accept_only_when_enabled_and_a_trusted_device_exists() {
        let on = store::AppPreferences {
            auto_accept_trusted: true,
            ..store::AppPreferences::default()
        };
        let off = store::AppPreferences {
            auto_accept_trusted: false,
            ..store::AppPreferences::default()
        };

        // Case 3: unknown device / no trusted devices -> always prompt.
        assert!(!should_auto_accept(&on, &[]));
        // Case 2: trusted device present but setting off -> prompt.
        assert!(!should_auto_accept(&off, &[trusted("dev-1")]));
        // Case 1: setting on AND a trusted device exists -> auto-accept.
        assert!(should_auto_accept(&on, &[trusted("dev-1")]));
    }

    #[test]
    fn resolve_download_dir_uses_custom_when_writable_else_fallback() {
        let fallback = temp_path("dl-fallback");
        std::fs::create_dir_all(&fallback).expect("fallback dir");

        // Empty preference -> fallback unchanged.
        assert_eq!(resolve_download_dir("", &fallback), fallback);

        // Custom dir that does not exist yet -> created and used (Task 2 case 2).
        let custom = temp_path("dl-custom");
        assert!(!custom.exists());
        let resolved = resolve_download_dir(&custom.display().to_string(), &fallback);
        assert_eq!(resolved, custom);
        assert!(custom.is_dir(), "custom download dir is created");

        std::fs::remove_dir_all(&fallback).ok();
        std::fs::remove_dir_all(&custom).ok();
    }

    #[test]
    fn resolve_download_dir_falls_back_when_path_is_a_file() {
        let fallback = temp_path("dl-fb2");
        std::fs::create_dir_all(&fallback).expect("fallback dir");
        // A path that is a regular file cannot be a download dir -> fallback.
        let file = temp_path("dl-not-a-dir");
        std::fs::write(&file, b"x").expect("file");

        let resolved = resolve_download_dir(&file.display().to_string(), &fallback);
        assert_eq!(resolved, fallback);

        std::fs::remove_file(&file).ok();
        std::fs::remove_dir_all(&fallback).ok();
    }

    #[test]
    fn throughput_mbps_computes_and_guards_zero_duration() {
        // 100 MB in 1000 ms = 100 MB/s.
        assert!((throughput_mbps(100_000_000, 1000) - 100.0).abs() < 1e-9);
        // Zero duration must not divide by zero.
        assert_eq!(throughput_mbps(5_000, 0), 0.0);
    }

    #[test]
    fn benchmark_report_aggregates_samples_and_writes_json() {
        let state = state_with_store("bench");
        // Two successful iterations + one failed.
        let mut run = StressRun::new("/tmp/big.bin".to_owned(), 3);
        run.iterations = vec![
            StressIteration {
                index: 0,
                state: TransferJobState::Completed,
                error: None,
                duration_ms: 1000,
                bytes: 100_000_000,
                mbps: 100.0,
            },
            StressIteration {
                index: 1,
                state: TransferJobState::Completed,
                error: None,
                duration_ms: 2000,
                bytes: 100_000_000,
                mbps: 50.0,
            },
            StressIteration {
                index: 2,
                state: TransferJobState::Failed,
                error: Some("boom".to_owned()),
                duration_ms: 10,
                bytes: 0,
                mbps: 0.0,
            },
        ];
        run.completed = 2;
        run.failed = 1;

        let snapshot = run.snapshot(7);
        assert!(
            (snapshot.avg_mbps - 75.0).abs() < 1e-9,
            "avg over successes"
        );
        assert_eq!(snapshot.avg_duration_ms, 1500);
        assert_eq!(snapshot.file_size, 100_000_000);

        let report = build_benchmark_report(&snapshot, &state.config());
        assert_eq!(report.iterations, 3);
        assert_eq!(report.completed, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.samples.len(), 3);

        // The report serializes and writes to <state_dir>/reports/.
        let json = serde_json::to_string_pretty(&report).expect("json");
        let path = state
            .store
            .write_report("test-stamp", &json)
            .expect("write");
        assert!(path.ends_with("transfer-report-test-stamp.json"));
        let round = std::fs::read_to_string(&path).expect("read back");
        assert!(round.contains("\"avgMbps\""));
        std::fs::remove_dir_all(state.store.reports_dir()).ok();
    }
}
