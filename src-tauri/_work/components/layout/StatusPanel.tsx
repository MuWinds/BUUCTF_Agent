import { useSession } from '@/store/session';
import { basename, useWorkspace } from '@/store/workspace';
import { ContextRing } from './ContextRing';
import { Settings2, MessageSquarePlus, FolderOpen } from 'lucide-react';

/**
 * 底部状态条：工作区、模型、用量、速度、耗时。
 *
 * 用量来自服务端 `stream_options.include_usage`；部分兼容网关不返回，
 * 此时这些格子留空而不是显示 0 —— 0 会让人以为真的没消耗 token。
 */
export function StatusPanel({ onOpenSettings }: { onOpenSettings: () => void }) {
  const model = useSession((s) => s.model);
  const usage = useSession((s) => s.usage);
  const streaming = useSession((s) => s.streaming);
  const reset = useSession((s) => s.reset);
  const hasMessages = useSession((s) => s.messages.length > 0);

  const workspace = useWorkspace((s) => s.path);
  const pickWorkspace = useWorkspace((s) => s.pick);

  return (
    <div className="flex h-7 shrink-0 items-center justify-between border-t border-(--border) bg-(--bg-elevated) px-3 font-mono text-[11px] text-(--fg-subtle)">
      <div className="flex min-w-0 items-center gap-3">
        <span
          className={`size-1.5 shrink-0 rounded-full ${
            streaming ? 'animate-pulse bg-warn' : 'bg-(--color-ok)'
          }`}
        />

        <button
          type="button"
          onClick={() => void pickWorkspace()}
          title={workspace ? `工作区：${workspace}\n点击切换（会清空当前对话）` : '选择工作区'}
          className="flex min-w-0 items-center gap-1 transition-colors hover:text-(--fg)"
        >
          <FolderOpen className="size-3 shrink-0" />
          <span className="truncate">{workspace ? basename(workspace) : '选择工作区'}</span>
        </button>

        <span className="shrink-0 text-(--fg-muted)">{model || '未连接'}</span>
      </div>

      <div className="flex shrink-0 items-center gap-4">
        {usage && (
          <>
            <ContextRing used={usage.contextUsed} limit={usage.contextLimit} />
            <Metric
              label="tok"
              value={(usage.promptTokens + usage.completionTokens).toLocaleString()}
              title={`本轮累计：输入 ${usage.promptTokens.toLocaleString()} + 输出 ${usage.completionTokens.toLocaleString()}`}
            />
            <Metric label="tok/s" value={usage.tps.toFixed(1)} />
            <Metric label="" value={`${(usage.elapsedMs / 1000).toFixed(1)}s`} />
          </>
        )}

        {hasMessages && !streaming && (
          <button
            type="button"
            onClick={() => void reset()}
            title="新对话"
            className="transition-colors hover:text-(--fg)"
          >
            <MessageSquarePlus className="size-3.5" />
          </button>
        )}

        <button
          type="button"
          onClick={onOpenSettings}
          title="设置"
          className="transition-colors hover:text-(--fg)"
        >
          <Settings2 className="size-3.5" />
        </button>
      </div>
    </div>
  );
}

function Metric({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <span className="tabular-nums" title={title}>
      {value}
      {label && <span className="ml-0.5 text-base-600">{label}</span>}
    </span>
  );
}
