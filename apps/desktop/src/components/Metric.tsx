type MetricProps = {
  label: string;
  value: string;
  detail?: string;
};

export function Metric({ label, value, detail }: MetricProps) {
  return (
    <div className="metric">
      <span className="metric__label">{label}</span>
      <strong className="metric__value">{value}</strong>
      {detail ? <span className="metric__detail">{detail}</span> : null}
    </div>
  );
}
