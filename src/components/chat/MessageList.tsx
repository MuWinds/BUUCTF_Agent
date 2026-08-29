import { useEffect, useRef } from 'react';
import { useSession } from '@/store/session';
import { AssistantMessage } from './AssistantMessage';
import { UserMessage } from './UserMessage';

export function MessageList() {
  const messages = useSession((s) => s.messages);
  const containerRef = useRef<HTMLDivElement>(null);
  /** 用户是否仍贴在底部。上滑查看历史时就不该被流式输出拽回去。 */
  const stuckToBottom = useRef(true);

  useEffect(() => {
    const el = containerRef.current;
    if (!el || !stuckToBottom.current) return;
    el.scrollTop = el.scrollHeight;
  });

  const onScroll = () => {
    const el = containerRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    stuckToBottom.current = distance < 40;
  };

  if (messages.length === 0) return <EmptyState />;

  return (
    <div ref={containerRef} onScroll={onScroll} className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl py-4">
        {messages.map((m, i) => {
          // 最后一条正在流式更新，跳过屏外优化以免闪烁
          const skippable = i < messages.length - 1;
          return (
            <div key={m.id} className={skippable ? 'offscreen-skip' : undefined}>
              {m.role === 'user' ? <UserMessage message={m} /> : <AssistantMessage message={m} />}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex flex-1 items-center justify-center">
      <div className="text-center">
        <div className="mx-auto mb-4 flex size-12 items-center justify-center rounded-(--radius-card) border border-(--border) bg-(--bg-elevated) font-mono text-lg text-(--color-accent)">
          {'>_'}
        </div>
        <p className="text-sm text-(--fg-muted)">开始一段对话</p>
        <p className="mt-1 text-xs text-(--fg-subtle)">Enter 发送 · Shift + Enter 换行</p>
      </div>
    </div>
  );
}
