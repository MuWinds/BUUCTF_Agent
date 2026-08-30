import { create } from 'zustand';
import { cancelTurn, clearHistory, getSession, sendMessage } from '@/lib/ipc';
import { errorMessage, type AgentEvent, type ToolResultBody } from '@/lib/events';
import type { Session as PersistedSession } from '@/lib/session';

export type MessageStatus = 'streaming' | 'done' | 'cancelled' | 'error';
export type ToolStatus = 'pending' | 'running' | 'ok' | 'error';

export interface ToolCall {
  id: string;
  name: string;
  /** 折叠态摘要，由 Rust 侧工具生成 —— 前端不该懂工具语义。 */
  preview: string;
  args?: unknown;
  status: ToolStatus;
  durationMs?: number;
  result?: ToolResultBody;
  /** 执行期间流式到达的输出，命令跑完前就能看到。 */
  liveOutput: string;
}

/**
 * 消息内容片段。
 *
 * 一条 assistant 消息里文本和工具调用是交错的（循环里每轮都可能先说话再调工具），
 * 用「文本 + 工具列表」两个扁平数组渲染会丢失顺序，所以按到达顺序存成片段。
 */
export type Segment = { kind: 'text'; text: string } | { kind: 'tool'; callId: string };

export interface Message {
  id: string;
  role: 'user' | 'assistant';
  segments: Segment[];
  tools: Record<string, ToolCall>;
  reasoning: string;
  status: MessageStatus;
  error?: string;
  /** 请求失败后正在自动重试：失败原因 + 第几次重试。null 表示未在重试。 */
  retrying: {
    attempt: number;
    maxRetries: number | null;
    message: string;
    /** 距下一次重试的等待毫秒数，供 UI 提示「N 秒后重试」。 */
    retryAfterMs: number;
  } | null;
}

export interface Usage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  /** 当前上下文实际占用（最后一次请求的输入 token）。 */
  contextUsed: number;
  contextLimit: number;
  elapsedMs: number;
  tps: number;
}

interface SessionState {
  messages: Message[];
  streaming: boolean;
  model: string;
  usage: Usage | null;
  /** 是否已从 Rust 侧加载过历史会话。 */
  restored: boolean;
  restore: () => Promise<void>;
  send: (text: string) => Promise<void>;
  stop: () => Promise<void>;
  reset: () => Promise<void>;
}

/**
 * 把持久化的会话还原成界面消息。
 *
 * `system` 条目不显示 —— 它是给模型的指令，不是对话内容。
 */
function toMessages(session: PersistedSession): Message[] {
  const messages: Message[] = [];

  for (const entry of session.entries) {
    if (entry.role === 'system') continue;

    if (entry.role === 'user') {
      messages.push({
        id: crypto.randomUUID(),
        role: 'user',
        segments: [{ kind: 'text', text: entry.text }],
        tools: {},
        reasoning: '',
        status: 'done',
        retrying: null,
      });
      continue;
    }

    const tools: Record<string, ToolCall> = {};
    const segments: Segment[] = entry.segments.map((segment) => {
      if (segment.kind === 'text') {
        return { kind: 'text', text: segment.text };
      }
      const record = segment.call;
      tools[record.call_id] = {
        id: record.call_id,
        name: record.name,
        preview: record.preview,
        args: record.args,
        status: record.ok ? 'ok' : 'error',
        durationMs: record.duration_ms,
        result: record.ui,
        liveOutput: '',
      };
      return { kind: 'tool', callId: record.call_id };
    });

    messages.push({
      id: crypto.randomUUID(),
      role: 'assistant',
      segments,
      tools,
      reasoning: entry.reasoning ?? '',
      status: entry.status,
      retrying: null,
    });
  }

  return messages;
}

// 流式增量的 rAF 缓冲。
// Rust 侧已按 33ms 聚合，这里是第二层兜底：即便事件密集到来，也最多
// 每帧提交一次 state，不会出现一帧内多次 React 重渲染。
let pendingText = '';
let pendingReasoning = '';
let rafId: number | null = null;

function scheduleFlush() {
  if (rafId !== null) return;
  rafId = requestAnimationFrame(() => {
    rafId = null;
    commitPending();
  });
}

function commitPending() {
  const text = pendingText;
  const reasoning = pendingReasoning;
  pendingText = '';
  pendingReasoning = '';
  if (!text && !reasoning) return;

  patchLastMessage((m) => ({
    ...m,
    reasoning: m.reasoning + reasoning,
    segments: text ? appendText(m.segments, text) : m.segments,
  }));
}

/** 把文本并入最后一个文本片段；末尾是工具卡片时另起一段。 */
function appendText(segments: Segment[], text: string): Segment[] {
  const last = segments[segments.length - 1];
  if (last?.kind === 'text') {
    return [...segments.slice(0, -1), { kind: 'text', text: last.text + text }];
  }
  return [...segments, { kind: 'text', text }];
}

/** 丢弃尚未提交的增量。轮次结束/取消时调用，防止残留内容落到下一条消息上。 */
function discardPending() {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  pendingText = '';
  pendingReasoning = '';
}

/** 立刻提交缓冲。轮次结束前调用，不丢最后一帧。 */
function flushNow() {
  if (rafId !== null) {
    cancelAnimationFrame(rafId);
    rafId = null;
  }
  commitPending();
}

/** 替换消息列表的最后一条，返回新数组。 */
function replaceLast(messages: Message[], fn: (m: Message) => Message): Message[] {
  if (messages.length === 0) return messages;
  const last = messages[messages.length - 1]!;
  return [...messages.slice(0, -1), fn(last)];
}

function patchLastMessage(fn: (m: Message) => Message) {
  useSession.setState((s) => ({ messages: replaceLast(s.messages, fn) }));
}

/** 更新最后一条消息里的某个工具调用。 */
function patchTool(callId: string, fn: (t: ToolCall) => ToolCall) {
  patchLastMessage((m) => {
    const existing = m.tools[callId];
    if (!existing) return m;
    return { ...m, tools: { ...m.tools, [callId]: fn(existing) } };
  });
}

function setLastStatus(status: MessageStatus, error?: string) {
  patchLastMessage((m) => ({ ...m, status, error, retrying: null }));
}

/**
 * 清除重试横幅。
 *
 * 任何正文/思维链/工具调用增量都只可能在重试的连接成功之后到达 ——
 * 收到它们说明这次请求活了，横幅再挂下去会与正在流出的回答自相矛盾
 * （一边说「正在重试」一边已经在出字）。
 */
function clearRetrying() {
  const { messages } = useSession.getState();
  if (!messages[messages.length - 1]?.retrying) return;
  patchLastMessage((m) => ({ ...m, retrying: null }));
}

export const useSession = create<SessionState>((set, get) => ({
  messages: [],
  streaming: false,
  model: '',
  usage: null,
  restored: false,

  /** 启动时从 Rust 侧还原上次的会话。 */
  async restore() {
    try {
      const session = await getSession();
      // 期间用户可能已经开始对话了。全量覆盖会把刚发的消息连同流式回复
      // 一起冲掉，所以只在界面仍是空的时候才写入。
      const { messages, streaming } = get();
      if (messages.length > 0 || streaming) {
        set({ restored: true });
        return;
      }
      set({ messages: toMessages(session), restored: true });
    } catch (e) {
      console.warn('恢复会话失败', e);
      set({ restored: true });
    }
  },

  async send(text: string) {
    if (get().streaming) return;

    const userMsg: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      segments: [{ kind: 'text', text }],
      tools: {},
      reasoning: '',
      status: 'done',
      retrying: null,
    };
    const assistantMsg: Message = {
      id: crypto.randomUUID(),
      role: 'assistant',
      segments: [],
      tools: {},
      reasoning: '',
      status: 'streaming',
      retrying: null,
    };

    discardPending();
    set((s) => ({
      messages: [...s.messages, userMsg, assistantMsg],
      streaming: true,
      usage: null,
    }));

    try {
      await sendMessage(text, handleEvent);
    } catch (e) {
      // command 本身失败（配置无效、IPC 断开等），此时可能一个事件都没收到
      flushNow();
      setLastStatus('error', errorMessage(e));
    } finally {
      discardPending();
      // 兜底：轮次结束事件万一没送达，也不能让消息永远停在"思考中"转圈。
      // 什么都没收到时明确说出来，比一个静默的空气泡有用得多。
      set((s) => ({
        streaming: false,
        messages: replaceLast(s.messages, (m) => {
          if (m.status !== 'streaming') return m;
          const empty = m.segments.length === 0 && !m.reasoning;
          return empty
            ? { ...m, status: 'error' as const, error: '没有收到任何响应。请查看日志或重试。' }
            : { ...m, status: 'done' as const };
        }),
      }));
    }
  },

  async stop() {
    await cancelTurn();
  },

  async reset() {
    await clearHistory();
    discardPending();
    set({ messages: [], streaming: false, usage: null });
  },
}));

function handleEvent(event: AgentEvent) {
  // 事件链路的可观测性：Rust 侧的日志看不到前端有没有真的收到。
  // dev 模式下按 F12 打开控制台即可核对。
  if (import.meta.env.DEV) {
    console.debug('[agent]', event.type, event);
  }

  switch (event.type) {
    case 'turn_start':
      useSession.setState({ model: event.model });
      break;

    case 'assistant_delta':
      clearRetrying();
      pendingText += event.text;
      scheduleFlush();
      break;

    case 'reasoning_delta':
      clearRetrying();
      pendingReasoning += event.text;
      scheduleFlush();
      break;

    case 'tool_call_start': {
      // 先把缓冲的文本落地，工具卡片才能排在它后面而不是前面
      clearRetrying();
      flushNow();
      const call: ToolCall = {
        id: event.call_id,
        name: event.name,
        preview: event.name,
        status: 'pending',
        liveOutput: '',
      };
      patchLastMessage((m) => ({
        ...m,
        segments: [...m.segments, { kind: 'tool', callId: event.call_id }],
        tools: { ...m.tools, [event.call_id]: call },
      }));
      break;
    }

    case 'tool_call_ready':
      patchTool(event.call_id, (t) => ({
        ...t,
        preview: event.preview,
        args: event.args,
        status: 'running',
      }));
      break;

    case 'tool_progress':
      patchTool(event.call_id, (t) => ({
        ...t,
        liveOutput: t.liveOutput + event.chunk,
      }));
      break;

    case 'tool_result':
      patchTool(event.call_id, (t) => ({
        ...t,
        status: event.ok ? 'ok' : 'error',
        durationMs: event.duration_ms,
        result: event.result,
      }));
      break;

    case 'usage':
      useSession.setState({
        usage: {
          promptTokens: event.prompt_tokens,
          completionTokens: event.completion_tokens,
          totalTokens: event.total_tokens,
          contextUsed: event.context_used,
          contextLimit: event.context_limit,
          elapsedMs: event.elapsed_ms,
          tps: event.tps,
        },
      });
      break;

    case 'retry':
      flushNow();
      patchLastMessage((m) => ({
        ...m,
        retrying: {
          attempt: event.attempt,
          maxRetries: event.max_retries,
          message: event.message,
          retryAfterMs: event.retry_after_ms,
        },
      }));
      break;

    case 'turn_end':
      flushNow();
      patchLastMessage((m) => {
        // 出错/取消的轮次，状态与原因由 error 事件决定，turn_end 不得把它
        // 洗成 done —— 否则错误横幅刚出现就被抹掉，用户看到的就是
        // 「请求失败后静默停止，没有任何提示」。
        if (m.status === 'error' || m.status === 'cancelled') {
          return { ...m, retrying: null };
        }
        return {
          ...m,
          status: event.finish_reason === 'cancelled' ? 'cancelled' : 'done',
          retrying: null,
        };
      });
      break;

    case 'error':
      flushNow();
      setLastStatus('error', event.message);
      break;
  }
}
