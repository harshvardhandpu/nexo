type ProgressBarProps = {
  value: number;
  max: number;
};

export function ProgressBar({ value, max }: ProgressBarProps) {
  const percent = max <= 0 ? 0 : Math.min(100, Math.round((value / max) * 100));

  return (
    <div className="progress" aria-label="Transfer progress">
      <div className="progress__fill" style={{ width: `${percent}%` }} />
    </div>
  );
}
