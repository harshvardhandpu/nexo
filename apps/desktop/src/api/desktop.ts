import { invoke } from "@tauri-apps/api/core";

export type DesktopSettings = {
  stateDir: string;
  receiveDir: string;
  chunkSize: number;
};

export type StatePaths = {
  stateDir: string;
  receiveDir: string;
  database: string;
  receiverPeer: string;
  latestTransfer: string;
  peerId: string;
};

export type Peer = {
  peerId: string;
  displayName: string;
  addresses: string[];
  port: number;
};

export type ReceiverEndpoint = {
  address: string;
};

export type TransferStatusDetails = {
  transferId: string;
  sessionId: string;
  state: string | null;
  fileName: string | null;
  completedChunks: number;
  totalChunks: number;
  completedBytes: number;
  totalBytes: number;
};

export type TransferStatusSnapshot = {
  latest: TransferStatusDetails | null;
};

export type TransferJobKind = "send" | "receive";
export type TransferJobState = "running" | "completed" | "failed";

export type TransferJob = {
  jobId: number;
  kind: TransferJobKind;
  state: TransferJobState;
  output: string[];
  error: string | null;
};

export type StartJobResponse = {
  jobId: number;
};

export type StressRunState = "running" | "completed" | "failed";

export type StressIteration = {
  index: number;
  state: TransferJobState;
  error: string | null;
};

export type StressRun = {
  runId: number;
  filePath: string;
  targetIterations: number;
  completed: number;
  failed: number;
  state: StressRunState;
  iterations: StressIteration[];
  lastOutput: string[];
};

export type StartStressResponse = {
  runId: number;
};

export function getSettings() {
  return invoke<DesktopSettings>("get_settings");
}

export function getStatePaths() {
  return invoke<StatePaths>("get_state_paths");
}

export function getStatus() {
  return invoke<TransferStatusSnapshot>("get_status");
}

export function getReceiverEndpoint() {
  return invoke<ReceiverEndpoint | null>("get_receiver_endpoint");
}

export function discoverKnownPeers() {
  return invoke<Peer[]>("discover_known_peers");
}

export function startReceive() {
  return invoke<StartJobResponse>("start_receive");
}

export function startSend(filePath: string, host?: string) {
  return invoke<StartJobResponse>("start_send", {
    filePath,
    host: host || null,
  });
}

export function listTransferJobs() {
  return invoke<TransferJob[]>("list_transfer_jobs");
}

export function resetCompletedJobs() {
  return invoke<void>("reset_completed_jobs");
}

export function startStressRun(
  filePath: string,
  iterations: number,
  host?: string,
) {
  return invoke<StartStressResponse>("start_stress_run", {
    filePath,
    host: host || null,
    iterations,
  });
}

export function listStressRuns() {
  return invoke<StressRun[]>("list_stress_runs");
}

export function resetCompletedStressRuns() {
  return invoke<void>("reset_completed_stress_runs");
}
