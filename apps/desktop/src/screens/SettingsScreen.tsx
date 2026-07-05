import { Cpu, Database, FolderOpen, HardDrive, ShieldCheck } from "lucide-react";
import type { DesktopData } from "../lib/useDesktopData";
import { GlassPanel, PanelHead } from "../components/ui";
import { formatBytes } from "../utils";

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="kv">
      <span className="kv__k">{k}</span>
      <span className="kv__v">{v}</span>
    </div>
  );
}

export function SettingsScreen({ data }: { data: DesktopData }) {
  const { settings, paths } = data;

  return (
    <div className="page" key="settings">
      <div className="grid grid--2">
        <GlassPanel>
          <PanelHead icon={HardDrive} title="Storage" />
          <div>
            <Row k="Chunk size" v={settings ? formatBytes(settings.chunkSize) : "—"} />
            <Row k="State directory" v={settings?.stateDir ?? "—"} />
            <Row k="Receive directory" v={settings?.receiveDir ?? "—"} />
          </div>
        </GlassPanel>

        <GlassPanel>
          <PanelHead icon={Database} title="State files" />
          <div>
            <Row k="Database" v={paths?.database ?? "—"} />
            <Row k="Receiver advert" v={paths?.receiverPeer ?? "—"} />
            <Row k="Latest transfer" v={paths?.latestTransfer ?? "—"} />
          </div>
        </GlassPanel>
      </div>

      <GlassPanel>
        <PanelHead icon={Cpu} title="Device identity" />
        <div>
          <Row k="Peer id file" v={paths?.peerId ?? "—"} />
          <Row k="State root" v={paths?.stateDir ?? "—"} />
        </div>
      </GlassPanel>

      <GlassPanel>
        <PanelHead icon={ShieldCheck} title="About Nexo" />
        <p className="text-muted" style={{ margin: 0 }}>
          Nexo Desktop is a thin, animated UI over the unchanged Nexo core:
          encrypted QUIC transport, SHA-256 chunk + whole-file verification, and
          crash-safe incremental resume. This layer only calls existing core APIs
          through the Tauri bridge — networking, storage, and resume logic are
          untouched.
        </p>
        <div className="row row--wrap" style={{ marginTop: 14, gap: 10 }}>
          <span className="pill">
            <FolderOpen size={13} /> Local-first
          </span>
          <span className="pill">
            <ShieldCheck size={13} /> End-to-end encrypted
          </span>
          <span className="pill">
            <HardDrive size={13} /> Crash-safe resume
          </span>
        </div>
      </GlassPanel>
    </div>
  );
}
