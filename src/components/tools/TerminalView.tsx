import { memo, useEffect, useRef } from 'react';

/**
 * 终端输出视图。
 *
 * 命令执行期间显示流式到达的 liveOutput，结束后切换到最终结果 ——
 * 两者内容一致，但最终结果经过了尾部截断处理。
 */
export const TerminalView = memo(function TerminalView({
  command,
  output,
  exitCode,
  timedOut,
  killed,
  truncated,
  running,
}: {
  command?: string;
  output: string;
  exitCode?: number | null;
  timedOut?: boolean;
  killed?: boolean;
  truncated?: boolean;
  running?: boolean;
}) {
  const scrollRef = useRef<HTMLPreElement>(null);
  const stuckToBottom = useRef(true);

  // 运行中自动跟随输出，但用户上滑查看时不打断
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !running || !stuckToBottom.current) return;
    el.scrollTop = el.scrollHeight;
  }, [output, running]);

  return (
    <div>
      {command && (
        <div className="flex items-start gap-2 border-b border-(--border) px-3 py-1.5">
          <span className="shrink-0 select-none font-mono text-[11px] text-accent">$</span>
          <span className="selectable flex-1 font-mono text-[11.5px] break-all text-(--fg-muted)">
            {command}
          </span>
        </div>
      )}

      <pre
        ref={scrollRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          stuckToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
        }}
        className="selectable max-h-96 overflow-auto bg-(--bg-inset) px-3 py-2 font-mono text-[12px] leading-[1.55]"
      >
        {output || (running ? '' : '（无输出）')}
        {running && <Cursor />}
      </pre>

      <Status
        exitCode={exitCode}
        timedOut={timedOut}
        killed={killed}
        truncated={truncated}
        running={running}
      />
    </div>
  );
});

function Cursor() {
  return (
    <span className="ml-px inline-block h-[1em] w-1.75 translate-y-[0.15em] animate-pulse bg-accent" />
  );
}

function Status({
  exitCode,
  timedOut,
  killed,
  truncated,
  running,
}: {
  exitCode?: number | null;
  timedOut?: boolean;
  killed?: boolean;
  truncated?: boolean;
  running?: boolean;
}) {
  if (running) return null;

  const notes: { text: string; tone: 'ok' | 'warn' | 'danger' | 'muted' }[] = [];

  if (timedOut) notes.push({ text: '已超时终止', tone: 'danger' });
  else if (killed) notes.push({ text: '已中止', tone: 'warn' });
  else if (exitCode === 0) notes.push({ text: '退出码 0', tone: 'ok' });
  else if (typeof exitCode === 'number') {
    notes.push({ text: `退出码 ${exitCode}`, tone: 'danger' });
  }

  if (truncated) notes.push({ text: '输出已截断，仅显示末尾', tone: 'muted' });

  if (notes.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-3 border-t border-(--border) px-3 py-1.5 font-mono text-[11px]">
      {notes.map((note, i) => (
        <span key={i} className={TONES[note.tone]}>
          {note.text}
        </span>
      ))}
    </div>
  );
}

const TONES = {
  ok: 'text-(--color-ok)',
  warn: 'text-(--color-warn)',
  danger: 'text-(--color-danger)',
  muted: 'text-(--fg-subtle)',
} as const;
