import { useEffect, useRef, useState } from 'react';
import { ArrowUp, Square } from 'lucide-react';
import { useSession } from '@/store/session';

export function Composer() {
  const [text, setText] = useState('');
  const streaming = useSession((s) => s.streaming);
  const send = useSession((s) => s.send);
  const stop = useSession((s) => s.stop);
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

  const submit = () => {
    const value = text.trim();
    if (!value || streaming) return;
    setText('');
    void send(value);
  };

  return (
    <div className="shrink-0 px-6 pb-5">
      <div className="mx-auto max-w-3xl">
        <div className="flex items-end gap-2 rounded-card border border-(--border) bg-(--bg-elevated) p-2 transition-colors focus-within:border-(--border-strong)">
          <textarea
            ref={textareaRef}
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                submit();
              }
            }}
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
              onClick={submit}
              disabled={!text.trim()}
              title="发送 (Enter)"
              className="flex size-8 shrink-0 items-center justify-center rounded-md bg-accent text-base-950 transition-opacity hover:opacity-90 disabled:opacity-25"
            >
              <ArrowUp className="size-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
