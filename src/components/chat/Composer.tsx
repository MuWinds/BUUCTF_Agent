import { useEffect, useRef, useState } from 'react';
import { ArrowUp, Square } from 'lucide-react';
import { useSession, type SendMode } from '@/store/session';
import { useConfig, type KeyAction, type KeyCombo } from '@/store/config';

/** 把一次 Enter 按键归到可配置的三种组合之一；不认识的修饰组合返回 null。 */
function detectCombo(e: React.KeyboardEvent<HTMLTextAreaElement>): KeyCombo | null {
  if (e.key !== 'Enter') return null;
  const { ctrlKey, shiftKey, altKey, metaKey } = e;
  if (altKey || metaKey) return null;
  if (ctrlKey && !shiftKey) return 'ctrl_enter';
  if (shiftKey && !ctrlKey) return 'shift_enter';
  if (!ctrlKey && !shiftKey) return 'enter';
  return null;
}

function actionToMode(action: KeyAction): SendMode {
  switch (action) {
    case 'send':
      return 'normal';
    case 'queue':
      return 'queue';
    case 'preempt':
      return 'preempt';
    case 'newline':
      // 只有 onKeyDown 会在 newline 时提前 return，这里不会走到
      return 'normal';
  }
}

export function Composer() {
  const [text, setText] = useState('');
  const streaming = useSession((s) => s.streaming);
  const queued = useSession((s) => s.queue.length);
  const send = useSession((s) => s.send);
  const stop = useSession((s) => s.stop);
  const keybindings = useConfig((s) => s.keybindings);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // 高度自适应：先归零再按 scrollHeight 撑开，否则删字时不会回缩
  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = '0px';
    el.style.height = `${Math.min(el.scrollHeight, 240)}px`;
  }, [text]);

  // Esc 中止当前轮次
  useEffect(() => {
    if (!streaming) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void stop();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [streaming, stop]);

  const submit = (mode: SendMode) => {
    const value = text.trim();
    if (!value) return;
    setText('');
    void send(value, mode);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // 中文输入法候选态下的 Enter 是「选字」，不是发送/换行
    if (e.nativeEvent.isComposing) return;
    const combo = detectCombo(e);
    if (!combo) return;
    const action = keybindings[combo];
    if (action === 'newline') return; // 交给 textarea 默认行为换行
    e.preventDefault();
    submit(actionToMode(action));
  };

  return (
    <div className="shrink-0 px-6 pb-5">
      <div className="mx-auto max-w-3xl">
        <div className="flex items-end gap-2 rounded-card border border-(--border) bg-(--bg-elevated) p-2 transition-colors focus-within:border-(--border-strong)">
          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKeyDown}
            rows={1}
            placeholder={streaming ? '生成中… 按 Esc 中止' : '输入消息'}
            className="selectable max-h-60 flex-1 resize-none bg-transparent px-2 py-1.5 text-[15px] outline-none placeholder:text-(--fg-subtle)"
          />

          {streaming ? (
            <button
              type="button"
              onClick={() => void stop()}
              title="中止 (Esc)"
              className="flex size-8 shrink-0 items-center justify-center rounded-md bg-(--bg-inset) text-(--fg-muted) transition-colors hover:text-(--fg)"
            >
              <Square className="size-3.5 fill-current" />
            </button>
          ) : (
            <button
              type="button"
              onClick={() => submit('normal')}
              disabled={!text.trim()}
              title="发送"
              className="flex size-8 shrink-0 items-center justify-center rounded-md bg-accent text-base-950 transition-opacity hover:opacity-90 disabled:opacity-25"
            >
              <ArrowUp className="size-4" />
            </button>
          )}
        </div>

        {queued > 0 && (
          <p className="mt-1.5 text-center text-[11px] text-(--fg-subtle)">
            已排队 {queued} 条消息，将在当前回复结束后依次发送
          </p>
        )}
      </div>
    </div>
  );
}
