import { memo } from 'react';
import type { Message } from '@/store/session';

export const UserMessage = memo(function UserMessage({ message }: { message: Message }) {
  // 用户消息只会有一个文本片段
  const text = message.segments.map((s) => (s.kind === 'text' ? s.text : '')).join('');

  return (
    <div className="flex justify-end px-6 py-4">
      <div className="selectable max-w-[80%] rounded-card bg-(--bg-inset) px-4 py-2.5 text-[15px] whitespace-pre-wrap wrap-break-word">
        {text}
        {message.pending && (
          <div className="mt-1.5 text-right text-[11px] text-(--fg-subtle)">
            {message.pending === 'preempt'
              ? '插队中：当前工具调用结束后发送'
              : '排队中：当前回复结束后发送'}
          </div>
        )}
      </div>
    </div>
  );
});
