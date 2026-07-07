import { useCallback, useEffect, useRef, useState } from "react";
import {
  DesktopSettings,
  Peer,
  ReceiverEndpoint,
  StatePaths,
  StressRun,
  TransferJob,
  TransferStatusSnapshot,
  discoverKnownPeers,
  getReceiverEndpoint,
  getSettings,
  getStatePaths,
  getStatus,
  listStressRuns,
  listTransferJobs,
  resetCompletedJobs,
  resetCompletedStressRuns,
  startReceive,
  approveTransferRequest,
  createTransferRequest,
  rejectTransferRequest,
  startStressRun,
} from "../api/desktop";

const POLL_MS = 800;

export type DesktopData = {
  settings: DesktopSettings | null;
  paths: StatePaths | null;
  status: TransferStatusSnapshot | null;
  receiver: ReceiverEndpoint | null;
  jobs: TransferJob[];
  stress: StressRun[];
  peers: Peer[];
  peersLoading: boolean;
  peersError: string | null;
  error: string | null;
  ready: boolean;
  refreshPeers: () => Promise<void>;
  requestSend: (filePath: string, host?: string) => Promise<void>;
  approveRequest: (requestId: string) => Promise<void>;
  rejectRequest: (requestId: string) => Promise<void>;
  receive: () => Promise<void>;
  startStress: (
    filePath: string,
    iterations: number,
    host?: string,
  ) => Promise<void>;
  clearJobs: () => Promise<void>;
  clearStress: () => Promise<void>;
};

/**
 * Central data spine for the desktop UI. Polls the Rust bridge (which wraps the
 * unchanged core) for live status/jobs/stress, loads static config once, and
 * exposes the action commands. All engine work stays behind these bridge calls.
 */
export function useDesktopData(): DesktopData {
  const [settings, setSettings] = useState<DesktopSettings | null>(null);
  const [paths, setPaths] = useState<StatePaths | null>(null);
  const [status, setStatus] = useState<TransferStatusSnapshot | null>(null);
  const [receiver, setReceiver] = useState<ReceiverEndpoint | null>(null);
  const [jobs, setJobs] = useState<TransferJob[]>([]);
  const [stress, setStress] = useState<StressRun[]>([]);
  const [peers, setPeers] = useState<Peer[]>([]);
  const [peersLoading, setPeersLoading] = useState(false);
  const [peersError, setPeersError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  const inFlight = useRef(false);

  const pollLive = useCallback(async () => {
    if (inFlight.current) {
      return;
    }
    inFlight.current = true;
    try {
      const [nextStatus, nextReceiver, nextJobs, nextStress] = await Promise.all([
        getStatus(),
        getReceiverEndpoint(),
        listTransferJobs(),
        listStressRuns(),
      ]);
      setStatus(nextStatus);
      setReceiver(nextReceiver);
      setJobs(nextJobs);
      setStress(nextStress);
      setError(null);
      setReady(true);
    } catch (cause) {
      setError(String(cause));
    } finally {
      inFlight.current = false;
    }
  }, []);

  const loadStatic = useCallback(async () => {
    try {
      const [nextSettings, nextPaths] = await Promise.all([
        getSettings(),
        getStatePaths(),
      ]);
      setSettings(nextSettings);
      setPaths(nextPaths);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  useEffect(() => {
    void loadStatic();
    void pollLive();
    const timer = window.setInterval(pollLive, POLL_MS);
    return () => window.clearInterval(timer);
  }, [loadStatic, pollLive]);

  const refreshPeers = useCallback(async () => {
    setPeersLoading(true);
    setPeersError(null);
    try {
      setPeers(await discoverKnownPeers());
    } catch (cause) {
      setPeersError(String(cause));
    } finally {
      setPeersLoading(false);
    }
  }, []);

  const requestSend = useCallback(
    async (filePath: string, host?: string) => {
      // Creates a PENDING request and emits transfer_request_created; the modal
      // (driven by the App-level event listener) handles approve/reject. No
      // transfer starts here.
      await createTransferRequest(filePath, host);
    },
    [],
  );

  const approveRequest = useCallback(
    async (requestId: string) => {
      await approveTransferRequest(requestId);
      await pollLive();
    },
    [pollLive],
  );

  const rejectRequest = useCallback(async (requestId: string) => {
    await rejectTransferRequest(requestId);
  }, []);

  const receive = useCallback(async () => {
    await startReceive();
    await pollLive();
  }, [pollLive]);

  const startStress = useCallback(
    async (filePath: string, iterations: number, host?: string) => {
      await startStressRun(filePath, iterations, host);
      await pollLive();
    },
    [pollLive],
  );

  const clearJobs = useCallback(async () => {
    await resetCompletedJobs();
    await pollLive();
  }, [pollLive]);

  const clearStress = useCallback(async () => {
    await resetCompletedStressRuns();
    await pollLive();
  }, [pollLive]);

  return {
    settings,
    paths,
    status,
    receiver,
    jobs,
    stress,
    peers,
    peersLoading,
    peersError,
    error,
    ready,
    refreshPeers,
    requestSend,
    approveRequest,
    rejectRequest,
    receive,
    startStress,
    clearJobs,
    clearStress,
  };
}
