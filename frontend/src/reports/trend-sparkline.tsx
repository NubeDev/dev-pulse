/**
 * Minimal SVG sparkline — no chart-lib dependency.
 *
 * Renders a polyline over the `points` array; an empty / single-point
 * series falls back to a flat baseline so the trend column is
 * dimensionally stable across rows in the table.
 */

export interface SparklineProps {
  points: ReadonlyArray<{ key: string; value: number }>;
  width?: number;
  height?: number;
  ariaLabel?: string;
}

export function Sparkline({
  points,
  width = 120,
  height = 28,
  ariaLabel,
}: SparklineProps): JSX.Element {
  if (points.length === 0) {
    return (
      <svg
        role="img"
        aria-label={ariaLabel ?? "Trend (no data)"}
        width={width}
        height={height}
        viewBox={`0 0 ${width} ${height}`}
      >
        <line
          x1={0}
          x2={width}
          y1={height / 2}
          y2={height / 2}
          stroke="var(--muted-foreground)"
          strokeOpacity={0.3}
          strokeDasharray="2 3"
        />
      </svg>
    );
  }

  const values = points.map((p) => p.value);
  const max = Math.max(1, ...values);
  const min = Math.min(0, ...values);
  const range = max - min || 1;
  const pad = 2;
  const innerW = width - pad * 2;
  const innerH = height - pad * 2;

  const step = points.length === 1 ? 0 : innerW / (points.length - 1);
  const coords = points.map((p, i) => {
    const x = pad + (points.length === 1 ? innerW / 2 : i * step);
    const y = pad + innerH - ((p.value - min) / range) * innerH;
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });

  const total = values.reduce((a, b) => a + b, 0);
  return (
    <svg
      role="img"
      aria-label={ariaLabel ?? `Trend, ${points.length} buckets, total ${total}`}
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
    >
      <polyline
        points={coords.join(" ")}
        fill="none"
        stroke="var(--primary)"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {points.length === 1 && (
        <circle
          cx={pad + innerW / 2}
          cy={pad + innerH - ((values[0]! - min) / range) * innerH}
          r={2}
          fill="var(--primary)"
        />
      )}
    </svg>
  );
}
