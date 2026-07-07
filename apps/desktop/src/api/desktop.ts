import { invoke } from "@tauri-apps/api/core";

export type BackgroundSettings = {
  backgroundReceiving: boolean;
  startOnLogin: boolean;
};

export type ReceiverStatus = {
  receiving: boolean;
  discoverable: boolean;
  backgroundEnabled: boolean;
  endpoint: string | null;
};

export function getBackgroundSettings() {
  return invoke<BackgroundSettings>("get_background_settings");
}

export function setBackgroundSettings(
  backgroundReceiving: boolean,
  startOnLogin: boolean,
) {
  return invoke<BackgroundSettings>("set_background_settings", {
    backgroundReceiving,
    startOnLogin,
  });
}

export function getReceiverStatus() {
  return invoke<ReceiverStatus>("get_receiver_status");
}

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

export type TransferRequest = {
  id: string;
  filePath: string;
  fileName: string;
  fileSize: number;
  peerDisplayName: string;
  peerAddress: string;
  status: string;
};

/** Event name the backend emits when a send request needs confirmation. */
export const TRANSFER_REQUEST_EVENT = "transfer_request_created";

/** Event name emitted when an incoming transfer needs the receiver's approval. */
export const INCOMING_TRANSFER_EVENT = "incoming_transfer_request";

export type IncomingTransfer = {
  id: string;
  sender: string;
  filename: string;
  fileSize: number;
  checksum: string;
  timestamp: number;
};

/** Receiver step: accept an incoming transfer — the parked receive continues. */
export function approveIncomingRequest(requestId: string) {
  return invoke<void>("approve_incoming_request", { requestId });
}

/** Receiver step: reject an incoming transfer — the sender is told, no file. */
export function rejectIncomingRequest(requestId: string) {
  return invoke<void>("reject_incoming_request", { requestId });
}

export function listIncomingRequests() {
  return invoke<IncomingTransfer[]>("list_incoming_requests");
}

/**
 * AirDrop step 1: create a *pending* transfer request. This does NOT start a
 * transfer — the backend also emits `transfer_request_created` so the UI shows
 * the mandatory confirmation modal. Transfer only begins after approve().
 */
export function createTransferRequest(filePath: string, host?: string) {
  return invoke<TransferRequest>("create_transfer_request", {
    filePath,
    host: host || null,
  });
}

/** AirDrop step 2a: user approved — start the real transfer. */
export function approveTransferRequest(requestId: string) {
  return invoke<StartJobResponse>("approve_transfer_request", { requestId });
}

/** AirDrop step 2b: user cancelled — drop the pending request, no transfer. */
export function rejectTransferRequest(requestId: string) {
  return invoke<void>("reject_transfer_request", { requestId });
}

export function listTransferRequests() {
  return invoke<TransferRequest[]>("list_transfer_requests");
}

// ---- Phase 2: presence, trust, history -----------------------------------

export type PeerDevice = {
  id: string;
  displayName: string;
  address: string;
  platform: string;
  lastSeen: number;
  online: boolean;
  trusted: boolean;
};

export type TrustedDevice = {
  id: string;
  displayName: string;
  address: string;
  platform: string;
  fingerprint: string;
  firstTrusted: number;
  lastSeen: number;
};

export type TransferRecord = {
  id: string;
  filename: string;
  size: number;
  direction: string;
  peer: string;
  timestamp: number;
  status: string;
  durationMs: number;
  checksumOk: boolean;
};

export function listDevices() {
  return invoke<PeerDevice[]>("list_devices");
}

export function listTrustedDevices() {
  return invoke<TrustedDevice[]>("list_trusted_devices");
}

export function trustDevice(
  id: string,
  displayName: string,
  address: string,
  platform?: string,
) {
  return invoke<TrustedDevice>("trust_device", {
    id,
    displayName,
    address,
    platform: platform || null,
  });
}

export function untrustDevice(id: string) {
  return invoke<boolean>("untrust_device", { id });
}

export function renameTrustedDevice(id: string, displayName: string) {
  return invoke<boolean>("rename_trusted_device", { id, displayName });
}

export function listTransferHistory() {
  return invoke<TransferRecord[]>("list_transfer_history");
}

export function clearTransferHistory() {
  return invoke<void>("clear_transfer_history");
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
