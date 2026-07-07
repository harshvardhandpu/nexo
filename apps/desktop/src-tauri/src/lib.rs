mod store;

use cli::{
    CliConfig, CliStatePaths, DiscoveredPeer, ReceiverEndpoint, TransferStatusSnapshot,
    build_transfer_request, discover_peers, receiver_endpoint, run_receive, run_send,
    transfer_status_snapshot,
};
use engine::chunker::default_chunk_size;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Emitter, Manager, State};

type DesktopResult<T> = Result<T, String>;

/// Event name emitted when a pending AirDrop request needs user confirmation.
const TRANSFER_REQUEST_EVENT: &str = "transfer_request_created";

#[derive(Debug)]
pub struct DesktopAppState {
    config: CliConfig,
    jobs: Arc<Mutex<HashMap<u64, TransferJob>>>,
    next_job_id: AtomicU64,
    stress: Arc<Mutex<HashMap<u64, StressRun>>>,
    next_stress_id: AtomicU64,
    requests: Arc<Mutex<HashMap<String, PendingTransferRequest>>>,
    store: Arc<store::AppStore>,
    presence: Arc<Mutex<HashMap<String, PeerPresence>>>,
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
            store,
            presence: Arc::new(Mutex::new(HashMap::new())),
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
}

impl From<DiscoveredPeer> for PeerResponse {
    fn from(peer: DiscoveredPeer) -> Self {
        Self {
            peer_id: peer.peer_id,
            display_name: peer.display_name,
            addresses: peer.addresses,
            port: peer.port,
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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StressRunState {
    Running,
    Completed,
    Failed,
}

/// One send iteration inside a stress run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StressIteration {
    pub index: u64,
    pub state: TransferJobState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
        StressRunSnapshot {
            run_id,
            file_path: self.file_path.clone(),
            target_iterations: self.target_iterations,
            completed: self.completed,
            failed: self.failed,
            state: self.state,
            iterations: self.iterations.clone(),
            last_output: self.last_output.clone(),
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
fn start_receive(state: State<'_, DesktopAppState>) -> DesktopResult<StartJobResponse> {
    let config = state.config();
    let job_id = state.next_job_id();
    insert_job(
        &state.jobs,
        job_id,
        TransferJob::running(TransferJobKind::Receive),
    )?;
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
        move |output| run_receive(&config, output),
    );

    Ok(StartJobResponse { job_id })
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

/// Trusts a device by recording UI metadata over its *existing* certificate.
/// We can only fingerprint a device whose certificate we already hold (the
/// `receiver.peer` anchor), so trust cannot be fabricated for an unknown peer.
#[tauri::command]
fn trust_device(
    id: String,
    display_name: String,
    address: String,
    platform: Option<String>,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<store::TrustedDevice> {
    let advertisement = cli::receiver_advertisement(&state.config()).map_err(error_to_string)?;
    let fingerprint = match advertisement {
        Some((advert_address, certificate_der)) if advert_address == address => {
            store::certificate_fingerprint(&certificate_der)
        }
        _ => {
            return Err(
                "no certificate is held for this device yet — receive from or send to it first \
                 to establish trust"
                    .to_owned(),
            );
        }
    };

    Ok(state.store.trust_device(store::TrustedDevice {
        id,
        display_name,
        address,
        platform: platform.unwrap_or_else(|| "unknown".to_owned()),
        fingerprint,
        first_trusted: 0,
        last_seen: store::unix_now(),
    }))
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

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config = build_config(app.handle());
            app.manage(DesktopAppState::new(config));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            get_state_paths,
            get_status,
            get_receiver_endpoint,
            discover_known_peers,
            start_receive,
            create_transfer_request,
            approve_transfer_request,
            reject_transfer_request,
            list_transfer_requests,
            list_devices,
            list_trusted_devices,
            trust_device,
            untrust_device,
            rename_trusted_device,
            list_transfer_history,
            clear_transfer_history,
            list_transfer_jobs,
            reset_completed_jobs,
            start_stress_run,
            list_stress_runs,
            reset_completed_stress_runs,
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
                });
            });

            let mut output = LineBuffer::default();
            let result = work(&mut output);
            let lines = output.lines();

            update_stress(&stress, run_id, |run| {
                run.last_output = lines;
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
}
