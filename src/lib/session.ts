/**
 * 与 Rust 侧 `crates/agent-core/src/session.rs` 对齐的会话结构。
 *
 * 这是会话的唯一数据源：发给模型的消息由它投影得出，界面也由它还原。
 * 恢复时不会丢失 diff、终端输出这些只属于 UI 的信息。
 */

import type { ToolResultBody } from './events';

export interface SessionToolRecord {
  call_id: string;
  name: string;
  args: unknown;
  preview: string;
  llm_text: string;
  ui: ToolResultBody;
  ok: boolean;
  duration_ms: number;
}

export type SessionSegment =
  { kind: 'text'; text: string } | { kind: 'tool'; call: SessionToolRecord };

export type SessionEntry =
  | { role: 'system'; text: string }
  | { role: 'user'; text: string }
  | {
      role: 'assistant';
      segments: SessionSegment[];
      reasoning?: string;
      status: 'done' | 'cancelled' | 'error';
    };

export interface Session {
  entries: SessionEntry[];
}
