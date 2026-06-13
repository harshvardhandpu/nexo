import {
  Activity,
  HomeIcon,
  Radio,
  Search,
  Send,
  SettingsIcon,
  type LucideIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  DesktopSettings,
  Peer,
  ReceiverEndpoint,
  StatePaths,
  TransferJob,
  TransferStatusSnapshot,
  discoverKnownPeers,
  getReceiverEndpoint,
  getSettings,
  getStatePaths,
  getStatus,
  listTransferJobs,
  resetCompletedJobs,
  startReceive,
  startSend,
} from "./api/desktop";
import { DiscoverPeers } from "./screens/DiscoverPeers";
import { Home } from "./screens/Home";
import { ReceiveFiles } from "./screens/ReceiveFiles";
import { SendFile } from "./screens/SendFile";
import { Settings } from "./screens/Settings";
import { TransferProgress } from "./screens/TransferProgress";

type Screen = "home" | "discover" | "send" | "receive" | "progress" | "settings";

const navigation = [
  { id: "home", label: "Home", icon: HomeIcon },
  { id: "discover", label: "Discover", icon: Search },
  { id: "send", label: "Send", icon: Send },
  { id: "receive", label: "Receive", icon: Radio },
  { id: "progress", label: "Progress", icon: Activity },
  { id: "settings", label: "Settings", icon: SettingsIcon },
] satisfies Array<{ id: Screen; label: string; icon: LucideIcon }>;

export default function App() {
  const [screen, setScreen] = useState<Screen>("home");
  const [settings, setSettings] = useState<DesktopSettings | null>(null);
  const [paths, setPaths] = useState<StatePaths | null>(null);
  const [status, setStatus] = useState<TransferStatusSnapshot | null>(null);
  const [receiver, setReceiver] = useState<ReceiverEndpoint | null>(null);
  const [peers, setPeers] = useState<Peer[]>([]);
  const [jobs, setJobs] = useState<TransferJob[]>([]);
  const [filePath, setFilePath] = useState("");
  const [host, setHost] = useState("");
  const [peerLoading, setPeerLoading] = useState(false);
  const [peerError, setPeerError] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [globalError, setGlobalError] = useState<string | null>(null);

  const refreshCore = useCallback(async () => {
    try {
      const [nextSettings, nextPaths, nextStatus, nextReceiver, nextJobs] =
        await Promise.all([
          getSettings(),
          getStatePaths(),
          getStatus(),
          getReceiverEndpoint(),
          listTransferJobs(),
        ]);
      setSettings(nextSettings);
      setPaths(nextPaths);
      setStatus(nextStatus);
      setReceiver(nextReceiver);
      setJobs(nextJobs);
      setGlobalError(null);
    } catch (error) {
      setGlobalError(String(error));
    }
  }, []);

  const refreshPeers = useCallback(async () => {
    setPeerLoading(true);
    setPeerError(null);
    try {
      setPeers(await discoverKnownPeers());
    } catch (error) {
      setPeerError(String(error));
    } finally {
      setPeerLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshCore();
    const timer = window.setInterval(refreshCore, 1500);
    return () => window.clearInterval(timer);
  }, [refreshCore]);

  const handleReceive = useCallback(async () => {
    try {
      await startReceive();
      setScreen("progress");
      await refreshCore();
    } catch (error) {
      setGlobalError(String(error));
    }
  }, [refreshCore]);

  const handleSend = useCallback(async () => {
    setSendError(null);
    try {
      await startSend(filePath.trim(), host.trim() || receiver?.address);
      setScreen("progress");
      await refreshCore();
    } catch (error) {
      setSendError(String(error));
    }
  }, [filePath, host, receiver?.address, refreshCore]);

  const handleClearJobs = useCallback(async () => {
    await resetCompletedJobs();
    await refreshCore();
  }, [refreshCore]);

  const content = useMemo(() => {
    switch (screen) {
      case "discover":
        return (
          <DiscoverPeers
            peers={peers}
            loading={peerLoading}
            error={peerError}
            onRefresh={refreshPeers}
          />
        );
      case "send":
        return (
          <SendFile
            receiver={receiver}
            filePath={filePath}
            host={host}
            error={sendError}
            onFilePathChange={setFilePath}
            onHostChange={setHost}
            onSend={handleSend}
          />
        );
      case "receive":
        return (
          <ReceiveFiles
            receiver={receiver}
            receiveDir={settings?.receiveDir ?? null}
            onReceive={handleReceive}
          />
        );
      case "progress":
        return (
          <TransferProgress
            status={status}
            jobs={jobs}
            onClearJobs={handleClearJobs}
          />
        );
      case "settings":
        return <Settings settings={settings} paths={paths} />;
      case "home":
      default:
        return (
          <Home
            settings={settings}
            status={status}
            onNavigate={(next) => setScreen(next as Screen)}
          />
        );
    }
  }, [
    filePath,
    handleClearJobs,
    handleReceive,
    handleSend,
    host,
    jobs,
    paths,
    peerError,
    peerLoading,
    peers,
    receiver,
    refreshPeers,
    screen,
    sendError,
    settings,
    status,
  ]);

  return (
    <div className="appShell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand__mark">N</div>
          <div>
            <strong>Nexo</strong>
            <span>Desktop MVP</span>
          </div>
        </div>
        <nav>
          {navigation.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.id}
                className={screen === item.id ? "active" : ""}
                onClick={() => {
                  setScreen(item.id);
                  if (item.id === "discover") {
                    void refreshPeers();
                  }
                }}
                title={item.label}
              >
                <Icon size={18} />
                {item.label}
              </button>
            );
          })}
        </nav>
      </aside>
      <main>
        {globalError ? <div className="errorBanner">{globalError}</div> : null}
        {content}
      </main>
    </div>
  );
}
