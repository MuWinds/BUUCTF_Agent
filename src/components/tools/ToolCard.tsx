import { memo, useState } from 'react';
import { ChevronRight, Loader2, Check, X } from 'lucide-react';
import { DiffView } from './DiffView';
import { TerminalView } from './TerminalView';
import type { ToolCall, ToolStatus } from '@/store/session';

/**
 * 工具调用卡片。
 *
 * 折叠态只显示一行摘要（由 Rust 侧工具生成），展开才渲染结果 ——
 * 一次会话可能有几十个工具调用，全部展开会把对话淹掉。
 *
 * 例外：正在产生输出的命令默认展开，否则用户盯着一个折叠的卡片
 * 完全不知道发生了什么。
 */
export const ToolCard = memo(function ToolCard({ call }: { call: ToolCall }) {
  const running = call.status === 'running' || call.status === 'pending';
  const streaming = running && call.liveOutput.length > 0;
  const [manual, setManual] = useState<boolean | null>(null);

  const hasBody = call.result !== undefined || streaming;
  // 用户点过就听用户的，否则流式期间自动展开
  const open = manual ?? streaming;

  return (
    <div className="my-2 overflow-hidden rounded-card border border-(--border) bg-(--bg-elevated)">
      <button
        type="button"
        onClick={() => hasBody && setManual(!open)}
        disabled={!hasBody}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-(--bg-inset) disabled:cursor-default disabled:hover:bg-transparent"
      >
        <StatusIcon status={call.status} />

        <span className="flex-1 truncate font-mono text-[12.5px] text-(--fg-muted)">
          {call.preview}
        </span>

        {call.durationMs !== undefined && call.durationMs > 100 && (
          <span className="shrink-0 font-mono text-[11px] text-(--fg-subtle) tabular-nums">
            {formatDuration(call.durationMs)}
          </span>
        )}

        {hasBody && (
          <ChevronRight
            className={`size-3.5 shrink-0 text-(--fg-subtle) transition-transform ${
              open ? 'rotate-90' : ''
            }`}
          />
        )}
      </button>

      {open && (
        <div className="border-t border-(--border)">
          {call.result ? (
            <ResultBody result={call.result} />
          ) : (
            <TerminalView output={call.liveOutput} running />
          )}
        </div>
      )}
    </div>
  );
});

function StatusIcon({ status }: { status: ToolStatus }) {
  switch (status) {
    case 'pending':
    case 'running':
      return <Loader2 className="size-3.5 shrink-0 animate-spin text-warn" />;
    case 'ok':
      return <Check className="size-3.5 shrink-0 text-(--color-ok)" />;
    case 'error':
      return <X className="size-3.5 shrink-0 text-(--color-danger)" />;
  }
}

function ResultBody({ result }: { result: NonNullable<ToolCall['result']> }) {
  switch (result.kind) {
    case 'error':
      return (
        <div className="selectable px-3 py-2 text-[12.5px] leading-relaxed text-(--color-danger)">
          {result.message}
        </div>
      );

    case 'diff':
      return (
        <DiffView
          path={result.path}
          hunks={result.hunks}
          added={result.added}
          removed={result.removed}
        />
      );

    case 'exec':
      return (
        <TerminalView
          command={result.command}
          output={result.output}
          exitCode={result.exit_code}
          timedOut={result.timed_out}
          killed={result.killed}
          truncated={result.truncated}
        />
      );

    case 'text':
      return (
        <>
          <pre className="selectable max-h-96 overflow-auto px-3 py-2 font-mono text-[12px] leading-[1.55]">
            {result.content}
          </pre>
          {result.truncated && (
            <div className="border-t border-(--border) px-3 py-1.5 text-[11px] text-(--fg-subtle)">
              结果已截断
            </div>
          )}
        </>
      );
  }
}

function formatDuration(ms: number): string {
  return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`;
}
