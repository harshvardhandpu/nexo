import { useCallback, useEffect, useState } from "react";
import {
  ArrowDownToLine,
  CheckCircle2,
  Clock,
  Send,
  ShieldCheck,
  Trash2,
  XCircle,
} from "lucide-react";
import {
  type TransferRecord,
  clearTransferHistory,
  listTransferHistory,
} from "../api/desktop";
import {
  Empty,
  GlassPanel,
  NeonButton,
  PanelHead,
  StatusPill,
} from "../components/ui";
import { formatBytes, formatDuration, formatTimestamp } from "../utils";

const POLL_MS = 2000;

function statusVariant(status: string) {
  if (status === "completed") return "ok" as const;
  if (status === "cancelled") return "warn" as const;
  return "danger" as const;
}

function HistoryRow({ record }: { record: TransferRecord }) {
  const isSend = record.direction === "send";
  const Icon = isSend ? Send : ArrowDownToLine;
  return (
    <div className="timeline__item">
      <div className={`timeline__dot timeline__dot--${statusVariant(record.status)}`}>
        <Icon size={14} />
      </div>
      <div className="timeline__body">
        <div className="row row--between row--wrap">
          <strong className="mono">{record.filename}</strong>
          <StatusPill variant={statusVariant(record.status)}>
            {record.status === "completed" ? (
              <>
                <CheckCircle2 size={13} /> completed
              </>
            ) : record.status === "cancelled" ? (
              "cancelled"
            ) : (
              <>
                <XCircle size={13} /> {record.status}
              </>
            )}
          </StatusPill>
        </div>
        <div className="row row--wrap timeline__meta">
          <span>{isSend ? "to" : "from"} {record.peer}</span>
          <span>{formatBytes(record.size)}</span>
          <span>{formatDuration(record.durationMs)}</span>
          {record.checksumOk ? (
            <span className="row" style={{ color: "var(--success)", gap: 5 }}>
              <ShieldCheck size={12} /> SHA-256 ok
            </span>
          ) : null}
          <span className="text-faint">{formatTimestamp(record.timestamp)}</span>
        </div>
      </div>
    </div>
  );
}

export function HistoryScreen() {
  const [records, setRecords] = useState<TransferRecord[]>([]);

  const load = useCallback(async () => {
    try {
      setRecords(await listTransferHistory());
    } catch {
      /* history is best-effort */
    }
  }, []);

  useEffect(() => {
    void load();
    const timer = window.setInterval(load, POLL_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  const completed = records.filter((r) => r.status === "completed").length;

  return (
    <div className="page" key="history">
      <GlassPanel strong>
        <PanelHead
          icon={Clock}
          title={`Transfer history · ${completed}/${records.length} completed`}
          action={
            <NeonButton
              variant="ghost"
              icon={Trash2}
              onClick={async () => {
                await clearTransferHistory();
                await load();
              }}
              disabled={records.length === 0}
            >
              Clear
            </NeonButton>
          }
        />
        {records.length === 0 ? (
          <Empty icon={Clock}>No transfers recorded yet.</Empty>
        ) : (
          <div className="timeline">
            {records.map((record) => (
              <HistoryRow key={record.id + record.timestamp} record={record} />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
