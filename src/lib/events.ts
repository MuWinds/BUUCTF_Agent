/**
 * 与 Rust 侧 `crates/agent-core/src/events.rs` 手工对齐的事件类型。
 *
 * 字段是 snake_case：serde 的 `rename_all` 只作用于枚举变体名（即 `type` 的值），
 * 结构体字段保持原样。改 Rust 那边记得同步改这里。
 */

/**
 * 行内的一段文本。
 *
 * Rust 侧切好片段传过来，而不是给 (start, end) 索引 —— Rust 按字符计数、
 * JS 按 UTF-16 码元计数，中文和 emoji 会让索引对不上。前端只管拼接上色。
 */
export interface DiffSegment {
  text: string;
  /** 是否是本行实际变化的部分，做更强的高亮。 */
  emphasis: boolean;
}

export interface DiffLine {
  tag: 'eq' | 'del' | 'ins';
  old_line: number | null;
  new_line: number | null;
  segments: DiffSegment[];
}

export interface DiffHunk {
  lines: DiffLine[];
}

/** 工具结果载荷，对应 Rust 的 `ToolResultBody`。 */
export type ToolResultBody =
  | { kind: 'text'; content: string; truncated: boolean }
  | {
      kind: 'diff';
      path: string;
      hunks: DiffHunk[];
      added: number;
      removed: number;
    }
  | {
      kind: 'exec';
      command: string;
      exit_code: number | null;
      output: string;
      truncated: boolean;
      timed_out: boolean;
      killed: boolean;
    }
  | { kind: 'error'; message: string };

export type AgentEvent =
  | { type: 'turn_start'; turn_id: string; model: string }
  | { type: 'assistant_delta'; turn_id: string; text: string }
  | { type: 'reasoning_delta'; turn_id: string; text: string }
  | { type: 'tool_call_start'; turn_id: string; call_id: string; name: string }
  | {
      type: 'tool_call_ready';
      turn_id: string;
      call_id: string;
      name: string;
      args: unknown;
      preview: string;
    }
  | {
      type: 'tool_progress';
      turn_id: string;
      call_id: string;
      stream: 'stdout' | 'stderr';
      chunk: string;
    }
  | {
      type: 'tool_result';
      turn_id: string;
      call_id: string;
      ok: boolean;
      duration_ms: number;
      result: ToolResultBody;
    }
  | {
      type: 'usage';
      turn_id: string;
      prompt_tokens: number;
      completion_tokens: number;
      total_tokens: number;
      context_used: number;
      context_limit: number;
      elapsed_ms: number;
      tps: number;
    }
  | {
      type: 'turn_end';
      turn_id: string;
      finish_reason: string;
      elapsed_ms: number;
    }
  | {
      type: 'retry';
      turn_id: string;
      attempt: number;
      max_retries: number | null;
      message: string;
      retry_after_ms: number;
    }
  | {
      type: 'context_compacted';
      turn_id: string;
      /** 被压缩掉的条目数（= 替换成一条 Summary 的条目数）。 */
      removed_entries: number;
      /** LLM 生成的摘要正文。 */
      summary: string;
    }
  | {
      type: 'error';
      turn_id: string;
      code: string;
      message: string;
      retryable: boolean;
    };

/** Tauri command 返回的错误体，对应 Rust 的 `Error`。 */
export interface AppError {
  code: string;
  message: string;
  retryable: boolean;
}

/** invoke 抛出的东西不一定是 AppError（也可能是字符串或反序列化失败），统一成一句话。 */
export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) {
    return String((e as AppError).message);
  }
  return String(e);
}
