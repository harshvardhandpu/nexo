import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, Wifi, WifiOff } from "lucide-react";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { Banner, StatusPill } from "./components/ui";
import { useDesktopData } from "./lib/useDesktopData";
import {
  TRANSFER_REQUEST_EVENT,
  type TransferRequest,
} from "./api/desktop";
import { NAV, TITLES, type Screen } from "./screens/nav";
import { Dashboard } from "./screens/Dashboard";
import { DevicesScreen } from "./screens/DevicesScreen";
import { SendScreen } from "./screens/SendScreen";
import { ReceiveScreen } from "./screens/ReceiveScreen";
import { MonitorScreen } from "./screens/MonitorScreen";
import { HistoryScreen } from "./screens/HistoryScreen";
import { TrustedScreen } from "./screens/TrustedScreen";
import { StressScreen } from "./screens/StressScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

export default function App() {
  const [screen, setScreen] = useState<Screen>("dashboard");
  const data = useDesktopData();
  const [pendingRequest, setPendingRequest] = useState<TransferRequest | null>(
    null,
  );
  const [confirmBusy, setConfirmBusy] = useState(false);

  // AirDrop: the backend emits `transfer_request_created` for every send intent.
  // We show the mandatory confirmation modal; no transfer runs until approved.
  useEffect(() => {
    const unlisten = listen<TransferRequest>(TRANSFER_REQUEST_EVENT, (event) => {
      setPendingRequest(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const approve = async () => {
    if (!pendingRequest) return;
    setConfirmBusy(true);
    try {
      await data.approveRequest(pendingRequest.id);
      setPendingRequest(null);
      setScreen("monitor");
    } finally {
      setConfirmBusy(false);
    }
  };

  const reject = async () => {
    if (!pendingRequest) return;
    setConfirmBusy(true);
    try {
      await data.rejectRequest(pendingRequest.id);
    } finally {
      setPendingRequest(null);
      setConfirmBusy(false);
    }
  };

  const activeCount = data.jobs.filter((job) => job.state === "running").length;
  const meta = TITLES[screen];

  const renderScreen = () => {
    switch (screen) {
      case "devices":
        return <DevicesScreen />;
      case "send":
        return <SendScreen data={data} />;
      case "receive":
        return <ReceiveScreen data={data} />;
      case "monitor":
        return <MonitorScreen data={data} />;
      case "history":
        return <HistoryScreen />;
      case "trusted":
        return <TrustedScreen />;
      case "stress":
        return <StressScreen data={data} />;
      case "settings":
        return <SettingsScreen data={data} />;
      case "dashboard":
      default:
        return <Dashboard data={data} onNavigate={setScreen} />;
    }
  };

  return (
    <div className="app">
      <Sidebar items={NAV} active={screen} onSelect={setScreen} />
      <main className="main">
        <TopBar
          title={meta.title}
          subtitle={meta.subtitle}
          right={
            <>
              {activeCount > 0 ? (
                <StatusPill variant="live">{activeCount} active</StatusPill>
              ) : null}
              <StatusPill variant={data.receiver ? "ok" : "idle"}>
                {data.receiver ? (
                  <>
                    <Wifi size={13} /> {data.receiver.address}
                  </>
                ) : (
                  <>
                    <WifiOff size={13} /> offline
                  </>
                )}
              </StatusPill>
            </>
          }
        />
        <div className="content">
          {data.error ? (
            <div style={{ maxWidth: 1080, margin: "0 auto 16px" }}>
              <Banner variant="error" icon={AlertTriangle}>
                {data.error}
              </Banner>
            </div>
          ) : null}
          {renderScreen()}
        </div>
      </main>
      {pendingRequest ? (
        <ConfirmDialog
          request={pendingRequest}
          busy={confirmBusy}
          onApprove={approve}
          onReject={reject}
        />
      ) : null}
    </div>
  );
}
