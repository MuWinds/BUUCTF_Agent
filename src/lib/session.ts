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
  | { kind: 'reasoning'; text: string }
  | { kind: 'text'; text: string }
  | { kind: 'tool'; call: SessionToolRecord };

export type SessionEntry =
  | { role: 'system'; text: string }
  | { role: 'user'; text: string }
  | { role: 'summary'; text: string }
  | {
      role: 'assistant';
      segments: SessionSegment[];
      reasoning?: string;
      /** 每次请求一段的思维链，投影回传给服务端用；前端只用累加的 reasoning。 */
      reasoning_rounds?: string[];
      status: 'done' | 'cancelled' | 'error';
    };

export interface Session {
  entries: SessionEntry[];
}

/** 列表页用到的会话摘要，不含正文。与 Rust 侧 `SessionSummary` 对齐。 */
export interface SessionSummary {
  id: string;
  title: string;
  workspace: string;
  model: string;
  /** Unix 毫秒。 */
  created_at: number;
  updated_at: number;
  message_count: number;
}

/** `list_sessions` 的返回：当前会话 id + 全部摘要。 */
export interface SessionList {
  current_id: string;
  sessions: SessionSummary[];
}
