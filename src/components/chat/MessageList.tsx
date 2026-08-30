import { useEffect, useRef } from 'react';
import { GitBranch } from 'lucide-react';
import { useSession, type Message } from '@/store/session';
import { KEY_ACTION_LABEL, KEY_COMBO_LABEL, useConfig } from '@/store/config';
import { AssistantMessage } from './AssistantMessage';
import { UserMessage } from './UserMessage';

export function MessageList() {
  const messages = useSession((s) => s.messages);
  const streaming = useSession((s) => s.streaming);
  const rewindTo = useSession((s) => s.rewindTo);
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
              {m.role === 'summary' ? (
                <SummaryNotice message={m} />
              ) : (
                <Branchable
                  message={m}
                  streaming={streaming}
                  onRewind={() => void rewindTo(m.entryIndex!)}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

/**
 * 历史消息包裹层：hover 时在右上角露出「从这里分叉」按钮。
 * 只有带 entryIndex（已落盘）且不在流式中的消息才有意义 ——
 * 分叉点是后端会话里的真实条目，临时占位消息还没落盘，无从截断。
 */
function Branchable({
  message,
  streaming,
  onRewind,
}: {
  message: Message;
  streaming: boolean;
  onRewind: () => void;
}) {
  const canRewind = !streaming && message.entryIndex !== undefined;
  return (
    <div className="group relative">
      {message.role === 'user' ? (
        <UserMessage message={message} />
      ) : (
        <AssistantMessage message={message} />
      )}
      {canRewind && (
        <button
          type="button"
          title="从这里分叉：保留此前内容，丢弃之后的消息"
          onClick={onRewind}
          className="absolute right-0 top-2 z-10 flex items-center gap-1 rounded-md border border-(--border) bg-(--bg-elevated) px-2 py-1 text-[11px] text-(--fg-subtle) opacity-0 transition-opacity hover:text-(--fg) group-hover:opacity-100"
        >
          <GitBranch className="size-3" />
          分叉
        </button>
      )}
    </div>
  );
}

/** 自动压缩产生的摘要气泡。点击展开看摘要全文。 */
function SummaryNotice({ message }: { message: { summary?: string } }) {
  return (
    <div className="px-6 py-2">
      <div className="rounded-card border border-dashed border-(--border) bg-(--bg-elevated)/50 px-4 py-2 text-[12px] text-(--fg-subtle)">
        <span className="font-medium text-(--fg-muted)">上下文已自动压缩</span>
        {message.summary ? (
          <p className="mt-1 selectable whitespace-pre-wrap">{message.summary}</p>
        ) : null}
      </div>
    </div>
  );
}

function EmptyState() {
  const keybindings = useConfig((s) => s.keybindings);
  const hint = [
    `${KEY_COMBO_LABEL.enter} ${KEY_ACTION_LABEL[keybindings.enter]}`,
    `${KEY_COMBO_LABEL.shift_enter} ${KEY_ACTION_LABEL[keybindings.shift_enter]}`,
    `${KEY_COMBO_LABEL.ctrl_enter} ${KEY_ACTION_LABEL[keybindings.ctrl_enter]}`,
  ].join(' · ');

  return (
    <div className="flex flex-1 items-center justify-center">
      <div className="text-center">
        <div className="mx-auto mb-4 flex size-12 items-center justify-center rounded-card border border-(--border) bg-(--bg-elevated) font-mono text-lg text-accent">
          {'>_'}
        </div>
        <p className="text-sm text-(--fg-muted)">开始一段对话</p>
        <p className="mt-1 text-xs text-(--fg-subtle)">{hint}</p>
      </div>
    </div>
  );
}
