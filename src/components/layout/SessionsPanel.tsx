import { useEffect } from 'react';
import { History, MessageSquarePlus, Trash2, X } from 'lucide-react';
import { useSession } from '@/store/session';
import { basename } from '@/store/workspace';

/**
 * 会话列表面板（覆盖层）。
 *
 * 列出当前工作区的全部历史会话，按最近更新排序。点一行切换到该会话，
 * 右侧按钮删除；顶部「新对话」开一段新的空会话。当前正在编辑的那段
 * 用高亮标出，避免用户在几段历史之间切丢了自己写到哪里。
 */
export function SessionsPanel({ onClose }: { onClose: () => void }) {
  const sessions = useSession((s) => s.sessions);
  const currentSessionId = useSession((s) => s.currentSessionId);
  const switchTo = useSession((s) => s.switchTo);
  const createNew = useSession((s) => s.createNew);
  const remove = useSession((s) => s.remove);
  const refreshSessions = useSession((s) => s.refreshSessions);

  // 打开面板时拉一次最新列表，保证不展示过期数据
  useEffect(() => {
    void refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/50 p-6">
      <div className="flex h-[70vh] w-full max-w-lg flex-col overflow-hidden rounded-(--radius-card) border border-(--border) bg-(--bg-elevated) shadow-2xl">
        <div className="flex shrink-0 items-center justify-between border-b border-(--border) px-5 py-3">
          <h2 className="flex items-center gap-2 text-sm font-medium">
            <History className="size-4" />
            历史会话
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-(--fg-subtle) transition-colors hover:text-(--fg)"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-3">
          {sessions.length === 0 ? (
            <div className="flex h-full items-center justify-center text-[13px] text-(--fg-subtle)">
              还没有历史会话
            </div>
          ) : (
            <ul className="space-y-1.5">
              {sessions.map((s) => {
                const active = s.id === currentSessionId;
                return (
                  <li key={s.id}>
                    <div
                      className={`group flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2 transition-colors ${
                        active
                          ? 'border-(--color-accent)/40 bg-(--bg-inset)'
                          : 'border-transparent hover:bg-(--bg-inset)'
                      }`}
                      onClick={() => {
                        if (!active) void switchTo(s.id);
                        onClose();
                      }}
                    >
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px] text-(--fg)">
                          {s.title}
                          {active && <span className="ml-2 text-[11px] text-accent">当前</span>}
                        </div>
                        <div className="mt-0.5 truncate text-[11px] text-(--fg-subtle)">
                          {relativeTime(s.updated_at)} · {s.message_count} 条 ·{' '}
                          {s.model || '未连接'}
                          {s.workspace && ` · ${basename(s.workspace)}`}
                        </div>
                      </div>
                      <button
                        type="button"
                        title="删除该会话"
                        onClick={(e) => {
                          e.stopPropagation();
                          void remove(s.id);
                        }}
                        className="shrink-0 text-(--fg-subtle) opacity-0 transition-opacity hover:text-(--color-danger) group-hover:opacity-100"
                      >
                        <Trash2 className="size-3.5" />
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}
        </div>

        <div className="flex shrink-0 justify-between border-t border-(--border) px-5 py-3">
          <span className="text-[11px] text-(--fg-subtle)">会话按工作区独立保存</span>
          <button
            type="button"
            onClick={() => {
              void createNew();
              onClose();
            }}
            className="flex items-center gap-1.5 rounded-md bg-(--color-accent) px-3 py-1.5 text-[13px] font-medium text-(--color-base-950) transition-opacity hover:opacity-90"
          >
            <MessageSquarePlus className="size-3.5" />
            新对话
          </button>
        </div>
      </div>
    </div>
  );
}

/** 把 Unix 毫秒时间戳说成人话：1 分钟内刚刚，往远推渐粗，超过 30 天给日期。 */
function relativeTime(ms: number): string {
  const diff = Date.now() - ms;
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (diff < minute) return '刚刚';
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < day) return `${Math.floor(diff / hour)} 小时前`;
  if (diff < 30 * day) return `${Math.floor(diff / day)} 天前`;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}
