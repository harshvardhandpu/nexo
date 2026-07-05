import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Loader2, type LucideIcon } from "lucide-react";
import { formatPercent } from "../utils";

/* ---- Glass panel --------------------------------------------------------- */
export function GlassPanel({
  children,
  strong,
  className,
}: {
  children: ReactNode;
  strong?: boolean;
  className?: string;
}) {
  return (
    <section
      className={["glass", strong ? "glass--strong" : "", className ?? ""]
        .filter(Boolean)
        .join(" ")}
    >
      {children}
    </section>
  );
}

export function PanelHead({
  icon: Icon,
  title,
  action,
}: {
  icon?: LucideIcon;
  title: string;
  action?: ReactNode;
}) {
  return (
    <div className="panel-head">
      <div className="panel-title">
        {Icon ? <Icon size={17} className="icon" /> : null}
        <h3>{title}</h3>
      </div>
      {action}
    </div>
  );
}

/* ---- Buttons ------------------------------------------------------------- */
type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "ghost" | "danger";
  icon?: LucideIcon;
  loading?: boolean;
  block?: boolean;
};

export function NeonButton({
  variant = "primary",
  icon: Icon,
  loading,
  block,
  children,
  className,
  disabled,
  ...rest
}: ButtonProps) {
  return (
    <button
      className={[
        "btn",
        `btn--${variant}`,
        block ? "btn--block" : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
      disabled={disabled || loading}
      {...rest}
    >
      {loading ? (
        <Loader2 size={16} className="spin" />
      ) : Icon ? (
        <Icon size={16} />
      ) : null}
      {children}
    </button>
  );
}

/* ---- Status pill --------------------------------------------------------- */
export function StatusPill({
  variant = "idle",
  children,
}: {
  variant?: "ok" | "live" | "warn" | "danger" | "idle";
  children: ReactNode;
}) {
  return (
    <span className={`pill ${variant === "idle" ? "" : `pill--${variant}`}`}>
      <span className="pill__dot" />
      {children}
    </span>
  );
}

/* ---- Stat card ----------------------------------------------------------- */
export function StatCard({
  label,
  value,
  sub,
  icon: Icon,
}: {
  label: string;
  value: ReactNode;
  sub?: ReactNode;
  icon?: LucideIcon;
}) {
  return (
    <GlassPanel>
      <div className="stat">
        <span className="stat__label">
          {Icon ? <Icon size={13} /> : null}
          {label}
        </span>
        <span className="stat__value mono">{value}</span>
        {sub ? <span className="stat__sub">{sub}</span> : null}
      </div>
    </GlassPanel>
  );
}

/* ---- Liquid progress ----------------------------------------------------- */
export function LiquidProgress({
  ratio,
  label,
  tall,
}: {
  ratio: number;
  label?: string;
  tall?: boolean;
}) {
  const pct = Math.max(0, Math.min(1, ratio));
  return (
    <div className={`liquid ${tall ? "liquid--tall" : ""}`}>
      <div className="liquid__fill" style={{ width: `${pct * 100}%` }} />
      {label !== undefined ? (
        <span className="liquid__label">{label || formatPercent(pct)}</span>
      ) : null}
    </div>
  );
}

/* ---- Chunk grid (live transfer visualization) ---------------------------- */
export function ChunkGrid({
  total,
  completed,
  active,
  maxCells = 240,
}: {
  total: number;
  completed: number;
  active?: boolean;
  maxCells?: number;
}) {
  if (total <= 0) {
    return null;
  }
  const cells = Math.min(total, maxCells);
  const ratio = Math.max(0, Math.min(1, completed / total));
  const doneCells = Math.floor(ratio * cells);
  return (
    <div className="chunk-grid" aria-hidden>
      {Array.from({ length: cells }, (_, index) => {
        const state =
          index < doneCells
            ? "is-done"
            : active && index === doneCells
              ? "is-active"
              : "";
        return <span key={index} className={`chunk ${state}`} />;
      })}
    </div>
  );
}

/* ---- Field / input ------------------------------------------------------- */
export function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

/* ---- Banner -------------------------------------------------------------- */
export function Banner({
  variant = "info",
  icon: Icon,
  children,
}: {
  variant?: "info" | "error";
  icon?: LucideIcon;
  children: ReactNode;
}) {
  return (
    <div className={`banner banner--${variant}`}>
      {Icon ? <Icon size={16} /> : null}
      <span>{children}</span>
    </div>
  );
}

/* ---- Empty state --------------------------------------------------------- */
export function Empty({
  icon: Icon,
  children,
}: {
  icon?: LucideIcon;
  children: ReactNode;
}) {
  return (
    <div className="empty">
      {Icon ? <Icon size={26} /> : null}
      <span>{children}</span>
    </div>
  );
}

export function Spinner() {
  return <Loader2 size={16} className="spin" />;
}
