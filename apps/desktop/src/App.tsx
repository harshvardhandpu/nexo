import { useState } from "react";
import { AlertTriangle, Wifi, WifiOff } from "lucide-react";
import { Sidebar } from "./components/Sidebar";
import { TopBar } from "./components/TopBar";
import { Banner, StatusPill } from "./components/ui";
import { useDesktopData } from "./lib/useDesktopData";
import { NAV, TITLES, type Screen } from "./screens/nav";
import { Dashboard } from "./screens/Dashboard";
import { SendScreen } from "./screens/SendScreen";
import { ReceiveScreen } from "./screens/ReceiveScreen";
import { MonitorScreen } from "./screens/MonitorScreen";
import { StressScreen } from "./screens/StressScreen";
import { SettingsScreen } from "./screens/SettingsScreen";

export default function App() {
  const [screen, setScreen] = useState<Screen>("dashboard");
  const data = useDesktopData();

  const activeCount = data.jobs.filter((job) => job.state === "running").length;
  const meta = TITLES[screen];

  const renderScreen = () => {
    switch (screen) {
      case "send":
        return <SendScreen data={data} onNavigate={setScreen} />;
      case "receive":
        return <ReceiveScreen data={data} />;
      case "monitor":
        return <MonitorScreen data={data} />;
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
    </div>
  );
}
