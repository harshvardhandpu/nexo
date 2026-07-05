use cli::{
    CliConfig, CliStatePaths, DiscoveredPeer, ReceiverEndpoint, TransferStatusSnapshot,
    discover_peers, receiver_endpoint, run_receive, run_send, transfer_status_snapshot,
};
use engine::chunker::default_chunk_size;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use tauri::{Manager, State};

type DesktopResult<T> = Result<T, String>;

#[derive(Debug)]
pub struct DesktopAppState {
    config: CliConfig,
    jobs: Arc<Mutex<HashMap<u64, TransferJob>>>,
    next_job_id: AtomicU64,
    stress: Arc<Mutex<HashMap<u64, StressRun>>>,
    next_stress_id: AtomicU64,
}

impl DesktopAppState {
    pub fn new(config: CliConfig) -> Self {
        Self {
            config,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            next_job_id: AtomicU64::new(1),
            stress: Arc::new(Mutex::new(HashMap::new())),
            next_stress_id: AtomicU64::new(1),
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
        move |output| run_receive(&config, output),
    );

    Ok(StartJobResponse { job_id })
}

#[tauri::command]
fn start_send(
    file_path: String,
    host: Option<String>,
    state: State<'_, DesktopAppState>,
) -> DesktopResult<StartJobResponse> {
    let path = PathBuf::from(file_path);
    validate_file_path(&path)?;
    let host = parse_host(host)?;
    let config = state.config();
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
        move |output| run_send(&path, host, &config, output),
    );

    Ok(StartJobResponse { job_id })
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
            start_send,
            list_transfer_jobs,
            reset_completed_jobs,
            start_stress_run,
            list_stress_runs,
            reset_completed_stress_runs,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Nexo desktop");
}

fn spawn_transfer_job<F>(
    jobs: Arc<Mutex<HashMap<u64, TransferJob>>>,
    job_id: u64,
    kind: TransferJobKind,
    work: F,
) where
    F: FnOnce(&mut JobOutput) -> cli::CliResult<()> + Send + 'static,
{
    thread::spawn(move || {
        let mut output = JobOutput::new(jobs.clone(), job_id);
        let result = work(&mut output);
        let lines = output.lines();
        let (state, error) = match result {
            Ok(()) => (TransferJobState::Completed, None),
            Err(error) => (TransferJobState::Failed, Some(error.to_string())),
        };

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
