import { useCallback, useState } from "react";
import {
  CheckCircle2,
  FileUp,
  Layers,
  Play,
  Trash2,
  XCircle,
  Zap,
} from "lucide-react";
import type { StressIteration, StressRun } from "../api/desktop";
import type { DesktopData } from "../lib/useDesktopData";
import { useFileDrop } from "../lib/dragdrop";
import {
  Banner,
  Empty,
  Field,
  GlassPanel,
  LiquidProgress,
  NeonButton,
  PanelHead,
  StatusPill,
} from "../components/ui";
import { fileName, formatCount } from "../utils";

function cellColor(state: StressIteration["state"] | "pending"): string {
  switch (state) {
    case "completed":
      return "var(--success)";
    case "failed":
      return "var(--danger)";
    case "running":
      return "var(--cyan)";
    default:
      return "rgba(146, 170, 255, 0.1)";
  }
}

function IterationGrid({ run }: { run: StressRun }) {
  const cells = Array.from({ length: run.targetIterations }, (_, index) => {
    const iteration = run.iterations[index];
    const state = iteration ? iteration.state : "pending";
    return (
      <span
        key={index}
        title={iteration?.error ?? `iteration ${index + 1}`}
        className={`chunk ${state === "running" ? "is-active" : ""}`}
        style={{ background: cellColor(state) }}
      />
    );
  });
  return <div className="chunk-grid">{cells}</div>;
}

function RunCard({ run }: { run: StressRun }) {
  const attempts = run.completed + run.failed;
  const ratio = run.targetIterations > 0 ? attempts / run.targetIterations : 0;
  const variant =
    run.state === "completed" ? "ok" : run.state === "failed" ? "danger" : "live";
  const stateClass =
    run.state === "completed"
      ? "is-success"
      : run.state === "failed"
        ? "is-error"
        : "";

  return (
    <article className={`job ${stateClass}`}>
      <div className="job__head">
        <div className="job__title">
          <Layers size={16} className="icon" />
          <span className="mono">{fileName(run.filePath)}</span>
          <span className="text-faint">×{run.targetIterations}</span>
        </div>
        <StatusPill variant={variant}>
          {run.state === "running" ? "Running" : run.state}
        </StatusPill>
      </div>

      <LiquidProgress
        ratio={ratio}
        label={`${attempts}/${run.targetIterations}`}
      />
      <IterationGrid run={run} />

      <div className="row row--wrap" style={{ gap: 16, fontSize: 13 }}>
        <span className="row" style={{ color: "var(--success)" }}>
          <CheckCircle2 size={14} /> {formatCount(run.completed)} passed
        </span>
        <span className="row" style={{ color: "var(--danger)" }}>
          <XCircle size={14} /> {formatCount(run.failed)} failed
        </span>
      </div>

      {run.lastOutput.length > 0 ? (
        <div className="log">
          {run.lastOutput.slice(-4).map((line, index, all) => (
            <div
              key={index}
              className={`log__line ${index === all.length - 1 ? "log__line--last" : ""}`}
            >
              {line}
            </div>
          ))}
        </div>
      ) : null}
    </article>
  );
}

export function StressScreen({ data }: { data: DesktopData }) {
  const [filePath, setFilePath] = useState("");
  const [iterations, setIterations] = useState(5);
  const [host, setHost] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const onDrop = useCallback((paths: string[]) => setFilePath(paths[0]), []);
  const dragging = useFileDrop(onDrop);

  const runs = [...data.stress].sort((a, b) => b.runId - a.runId);
  const anyFinished = runs.some((run) => run.state !== "running");

  const submit = async () => {
    setError(null);
    if (!filePath.trim()) {
      setError("Choose the file to hammer (e.g. a 5 GB test file).");
      return;
    }
    setSubmitting(true);
    try {
      await data.startStress(
        filePath.trim(),
        Math.max(1, Math.floor(iterations)),
        host.trim() || data.receiver?.address,
      );
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="page" key="stress">
      <GlassPanel strong>
        <PanelHead icon={Zap} title="Automated stress run" />
        <div className="stack">
          <p className="text-muted" style={{ margin: 0 }}>
            Send one file back-to-back N times through the real transfer engine to
            validate reliability under repeated large transfers. Ensure a receiver
            is running and reachable first.
          </p>

          <div
            className={`dropzone ${dragging ? "is-drag" : ""}`}
            onClick={() => document.getElementById("stress-path")?.focus()}
          >
            <div className="dropzone__icon">
              <FileUp size={24} />
            </div>
            <strong>{dragging ? "Release to select" : "Drop the test file"}</strong>
            <span className="text-faint">large files (5 GB+) recommended</span>
          </div>

          <Field label="File path">
            <input
              id="stress-path"
              className="input"
              placeholder="/home/you/5gb.bin"
              value={filePath}
              onChange={(event) => setFilePath(event.target.value)}
              spellCheck={false}
            />
          </Field>

          <div className="field-row">
            <Field label="Iterations">
              <input
                className="input"
                type="number"
                min={1}
                max={1000}
                value={iterations}
                onChange={(event) =>
                  setIterations(Number(event.target.value) || 1)
                }
              />
            </Field>
            <Field label="Receiver (optional)">
              <input
                className="input"
                placeholder={data.receiver?.address ?? "127.0.0.1:41000"}
                value={host}
                onChange={(event) => setHost(event.target.value)}
                spellCheck={false}
              />
            </Field>
          </div>

          {error ? <Banner variant="error">{error}</Banner> : null}

          <NeonButton
            icon={Play}
            onClick={submit}
            loading={submitting}
            disabled={!filePath.trim()}
          >
            Launch {Math.max(1, Math.floor(iterations))}× run
          </NeonButton>
        </div>
      </GlassPanel>

      <GlassPanel>
        <PanelHead
          icon={Layers}
          title="Stress runs"
          action={
            <NeonButton
              variant="ghost"
              icon={Trash2}
              onClick={data.clearStress}
              disabled={!anyFinished}
            >
              Clear finished
            </NeonButton>
          }
        />
        {runs.length === 0 ? (
          <Empty icon={Zap}>No stress runs yet.</Empty>
        ) : (
          <div className="stack">
            {runs.map((run) => (
              <RunCard key={run.runId} run={run} />
            ))}
          </div>
        )}
      </GlassPanel>
    </div>
  );
}
