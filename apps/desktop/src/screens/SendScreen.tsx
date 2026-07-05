import { useCallback, useState } from "react";
import { FileUp, Send, Server, UploadCloud } from "lucide-react";
import type { DesktopData } from "../lib/useDesktopData";
import type { Screen } from "./nav";
import { useFileDrop } from "../lib/dragdrop";
import { JobCard } from "../components/JobCard";
import {
  Banner,
  Empty,
  Field,
  GlassPanel,
  NeonButton,
  PanelHead,
} from "../components/ui";
import { fileName } from "../utils";

export function SendScreen({
  data,
  onNavigate,
}: {
  data: DesktopData;
  onNavigate: (screen: Screen) => void;
}) {
  const [filePath, setFilePath] = useState("");
  const [host, setHost] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const onDrop = useCallback((paths: string[]) => {
    setFilePath(paths[0]);
    setError(null);
  }, []);
  const dragging = useFileDrop(onDrop);

  const sendJobs = data.jobs
    .filter((job) => job.kind === "send")
    .sort((a, b) => b.jobId - a.jobId);

  const submit = async () => {
    setError(null);
    const path = filePath.trim();
    if (!path) {
      setError("Drop a file or paste its full path first.");
      return;
    }
    setSubmitting(true);
    try {
      await data.send(path, host.trim() || data.receiver?.address);
      onNavigate("monitor");
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="page" key="send">
      <GlassPanel strong>
        <PanelHead icon={UploadCloud} title="Send a file" />
        <div className="stack">
          <div
            className={`dropzone ${dragging ? "is-drag" : ""}`}
            onClick={() => document.getElementById("send-path")?.focus()}
          >
            <div className="dropzone__icon">
              <FileUp size={26} />
            </div>
            <strong>{dragging ? "Release to select" : "Drag & drop a file"}</strong>
            <span className="text-faint">
              or paste an absolute path below — Nexo chunks, encrypts and streams it
            </span>
          </div>

          {filePath ? (
            <div className="file-chip">
              <Send size={15} className="icon" />
              <span className="file-chip__name">{fileName(filePath)}</span>
              <span className="text-faint mono" style={{ marginLeft: "auto" }}>
                {filePath}
              </span>
            </div>
          ) : null}

          <Field label="File path">
            <input
              id="send-path"
              className="input"
              placeholder="/home/you/movie.5gb.bin"
              value={filePath}
              onChange={(event) => setFilePath(event.target.value)}
              spellCheck={false}
            />
          </Field>

          <Field label="Receiver address (optional)">
            <input
              className="input"
              placeholder={data.receiver?.address ?? "127.0.0.1:41000"}
              value={host}
              onChange={(event) => setHost(event.target.value)}
              spellCheck={false}
            />
          </Field>

          {error ? <Banner variant="error">{error}</Banner> : null}

          <div className="row row--between row--wrap">
            <span className="text-faint row" style={{ fontSize: 12.5 }}>
              <Server size={14} />
              {data.receiver
                ? `Trusted receiver: ${data.receiver.address}`
                : "No receiver advertised yet — start Receive on the target device."}
            </span>
            <NeonButton
              icon={Send}
              onClick={submit}
              loading={submitting}
              disabled={!filePath.trim()}
            >
              Start transfer
            </NeonButton>
          </div>
        </div>
      </GlassPanel>

      <GlassPanel>
        <PanelHead icon={Send} title="Send activity" />
        {sendJobs.length === 0 ? (
          <Empty icon={UploadCloud}>Sent transfers will appear here.</Empty>
        ) : (
          <div className="stack">
            {sendJobs.map((job) => (
              <JobCard key={job.jobId} job={job} showLog />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
