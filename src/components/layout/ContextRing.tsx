/**
 * 上下文占用环。
 *
 * 显示的是最后一次请求的输入 token 占窗口的比例 —— 不是本轮累计值，
 * 后者每次请求都重发完整历史，累加起来会超过 100%。
 */
export function ContextRing({ used, limit }: { used: number; limit: number }) {
  const ratio = limit > 0 ? Math.min(used / limit, 1) : 0;
  const percent = Math.round(ratio * 100);

  const size = 12;
  const stroke = 2;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;

  return (
    <span
      className="flex items-center gap-1.5"
      title={`上下文：${used.toLocaleString()} / ${limit.toLocaleString()} tokens（${percent}%）`}
    >
      <svg width={size} height={size} className="-rotate-90 shrink-0">
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="var(--border-strong)"
          strokeWidth={stroke}
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke={toneColor(ratio)}
          strokeWidth={stroke}
          strokeDasharray={circumference}
          strokeDashoffset={circumference * (1 - ratio)}
          strokeLinecap="round"
          className="transition-[stroke-dashoffset] duration-500"
        />
      </svg>
      <span className="tabular-nums" style={{ color: toneColor(ratio) }}>
        {percent}%
      </span>
    </span>
  );
}

/** 接近窗口上限时变色告警 —— 用户该考虑开新对话了。 */
function toneColor(ratio: number): string {
  if (ratio >= 0.9) return 'var(--color-danger)';
  if (ratio >= 0.7) return 'var(--color-warn)';
  return 'var(--fg-subtle)';
}
