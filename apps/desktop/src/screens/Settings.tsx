import { Database, FolderDown, HardDrive } from "lucide-react";
import type { ReactNode } from "react";
import type { DesktopSettings, StatePaths } from "../api/desktop";
import { formatBytes } from "../utils";

type SettingsProps = {
  settings: DesktopSettings | null;
  paths: StatePaths | null;
};

export function Settings({ settings, paths }: SettingsProps) {
  return (
    <section className="screen">
      <div className="screenHeader">
        <div>
          <h1>Settings</h1>
        </div>
      </div>

      <div className="settingsList">
        <SettingRow
          icon={<HardDrive size={18} />}
          label="State directory"
          value={settings?.stateDir ?? "Loading"}
        />
        <SettingRow
          icon={<FolderDown size={18} />}
          label="Receive directory"
          value={settings?.receiveDir ?? "Loading"}
        />
        <SettingRow
          icon={<Database size={18} />}
          label="Chunk size"
          value={settings ? formatBytes(settings.chunkSize) : "Loading"}
        />
        <SettingRow
          icon={<Database size={18} />}
          label="SQLite database"
          value={paths?.database ?? "Loading"}
        />
        <SettingRow
          icon={<Database size={18} />}
          label="Receiver advert"
          value={paths?.receiverPeer ?? "Loading"}
        />
      </div>
    </section>
  );
}

type SettingRowProps = {
  icon: ReactNode;
  label: string;
  value: string;
};

function SettingRow({ icon, label, value }: SettingRowProps) {
  return (
    <div className="settingRow">
      {icon}
      <span>{label}</span>
      <code>{value}</code>
    </div>
  );
}
