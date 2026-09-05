import { memo, useState } from 'react';
import { ChevronRight, AlertCircle, CircleSlash, RefreshCw } from 'lucide-react';
import { RichText } from './RichText';
import { ToolCard } from '@/components/tools/ToolCard';
import type { Message } from '@/store/session';

/**
 * 助手消息。
 *
 * 用 memo 冻结已定稿的消息：流式期间只有最后一条在变，历史消息不该参与
 * 重渲染。比较函数把「内容和状态都没变」的情况直接短路。
 */
export const AssistantMessage = memo(
  function AssistantMessage({ message }: { message: Message }) {
    const streaming = message.status === 'streaming';
    const empty = message.segments.length === 0 && !message.reasoning;

    return (
      <div className="px-6 py-4">
        <div className="text-[15px] text-(--fg)">
          {message.segments.map((segment, i) => {
            if (segment.kind === 'reasoning') {
              const isLast = i === message.segments.length - 1;
              return <ReasoningBlock key={i} text={segment.text} streaming={streaming && isLast} />;
            }
            if (segment.kind === 'tool') {
              const call = message.tools[segment.callId];
              return call ? <ToolCard key={segment.callId} call={call} /> : null;
            }
            const isLast = i === message.segments.length - 1;
            return (
              <div key={i}>
                <RichText text={segment.text} streaming={streaming} />
                {streaming && isLast && <Caret />}
              </div>
            );
          })}
        </div>

        {streaming && empty && (
          <div className="flex items-center gap-2 text-sm text-(--fg-subtle)">
            <Spinner />
            思考中
          </div>
        )}

        {message.retrying && (
          <div className="mt-2 flex items-start gap-2 rounded-card border border-(--color-warn)/30 bg-(--color-warn)/10 px-3 py-2 text-sm">
            <RefreshCw className="mt-0.5 size-4 shrink-0 animate-spin text-(--color-warn)" />
            <span className="selectable text-(--fg-muted)">
              请求失败（{message.retrying.message}），正在第 {message.retrying.attempt} 次重试
              {message.retrying.maxRetries === null
                ? '（无限重试，可点停止退出）'
                : `（最多 ${message.retrying.maxRetries} 次）`}
              ，{retryDelayText(message.retrying.retryAfterMs)} 后重试…
            </span>
          </div>
        )}

        {message.status === 'error' && (
          <div className="mt-2 flex items-start gap-2 rounded-card border border-(--color-danger)/30 bg-(--color-danger)/10 px-3 py-2 text-sm">
            <AlertCircle className="mt-0.5 size-4 shrink-0 text-(--color-danger)" />
            <span className="selectable text-(--fg-muted)">{message.error}</span>
          </div>
        )}

        {message.status === 'cancelled' && (
          <div className="mt-2 flex items-center gap-1.5 text-xs text-(--fg-subtle)">
            <CircleSlash className="size-3.5" />
            已中止
          </div>
        )}
      </div>
    );
  },
  (prev, next) =>
    prev.message.segments === next.message.segments &&
    prev.message.tools === next.message.tools &&
    prev.message.reasoning === next.message.reasoning &&
    prev.message.status === next.message.status &&
    prev.message.error === next.message.error &&
    prev.message.retrying === next.message.retrying,
);

/** 把重试等待的毫秒数说成人话。最小退避是 1s，四舍五入到秒即可。 */
function retryDelayText(ms: number): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  return `${seconds} 秒`;
}

/** 流式光标。用 CSS 动画而非 JS 定时器，不占主线程。 */
function Caret() {
  return (
    <span className="ml-0.5 inline-block h-[1.1em] w-0.5 translate-y-[0.2em] animate-pulse bg-accent" />
  );
}

function Spinner() {
  return (
    <span className="size-3 animate-spin rounded-full border-[1.5px] border-(--border-strong) border-t-accent" />
  );
}

function ReasoningBlock({ text, streaming }: { text: string; streaming: boolean }) {
  const [open, setOpen] = useState(false);

  return (
    <div className="mb-3">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 text-xs text-(--fg-subtle) transition-colors hover:text-(--fg-muted)"
      >
        <ChevronRight className={`size-3.5 transition-transform ${open ? 'rotate-90' : ''}`} />
        思考过程
        {streaming && <span className="ml-1 animate-pulse">…</span>}
      </button>
      {open && (
        <div className="selectable mt-2 border-l-2 border-(--border-strong) pl-3 text-[13px] leading-relaxed whitespace-pre-wrap text-(--fg-subtle)">
          {text}
        </div>
      )}
    </div>
  );
}
