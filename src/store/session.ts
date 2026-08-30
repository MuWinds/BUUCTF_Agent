import { create } from 'zustand';
import {
  cancelTurn,
  deleteSession,
  getSession,
  listSessions,
  newSession,
  preemptTurn,
  rewindSession,
  sendMessage,
  switchSession,
} from '@/lib/ipc';
import { errorMessage, type AgentEvent, type ToolResultBody } from '@/lib/events';
import type { Session as PersistedSession, SessionSummary } from '@/lib/session';

export type MessageStatus = 'streaming' | 'done' | 'cancelled' | 'error';
export type ToolStatus = 'pending' | 'running' | 'ok' | 'error';
export type SendMode = 'normal' | 'queue' | 'preempt';

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
  role: 'user' | 'assistant' | 'summary';
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
  /** 对应的后端 Session.entries 索引。历史消息有，流式创建的无（尚未落盘）。 */
  entryIndex?: number;
  /** 自动压缩的摘要正文。role === 'summary' 时存在。 */
  summary?: string;
  /** 排队/插队中、尚未交给后端轮次的消息标记。 */
  pending?: 'queue' | 'preempt';
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

/** 排队/插队消息。排队时先落一条用户气泡，轮次结束后再按顺序交给后端。 */
interface QueuedSend {
  text: string;
  mode: 'queue' | 'preempt';
  messageId: string;
}

interface SessionState {
  messages: Message[];
  streaming: boolean;
  model: string;
  usage: Usage | null;
  /** 是否已从 Rust 侧加载过历史会话。 */
  restored: boolean;
  /** 当前工作区的会话列表，按最近更新排序。 */
  sessions: SessionSummary[];
  /** 正在编辑的会话 id，用于列表里标出当前项。 */
  currentSessionId: string;
  /** 等待当前轮次结束后依次发送的消息。 */
  queue: QueuedSend[];
  /** 当前正在流式更新的助手消息 id。事件据此定位，而不是永远打最后一条。 */
  activeAssistantId: string | null;
  /** 当前轮次的 turn_id。用来拦截上一轮迟到的事件，避免打错消息。 */
  activeTurnId: string | null;
  /** 插队信号是否已对当前轮次发出过。turn_start 到达前入队的插队消息靠它补发。 */
  preemptSignaled: boolean;
  restore: () => Promise<void>;
  /** 拉取会话列表。轮次结束、切换工作区后调用，保持列表与磁盘一致。 */
  refreshSessions: () => Promise<void>;
  /** 切换到某段历史会话并载入其内容。 */
  switchTo: (id: string) => Promise<void>;
  /** 新建一段空会话并切换过去，旧会话保留在磁盘上。 */
  createNew: () => Promise<void>;
  /** 删除一段会话。删除当前会话时界面回到空会话。 */
  remove: (id: string) => Promise<void>;
  /** 回退/分叉：截断到第 entryIndex 条消息，旧分支保留在会话列表里。 */
  rewindTo: (entryIndex: number) => Promise<void>;
  send: (text: string, mode?: SendMode) => Promise<void>;
  stop: () => Promise<void>;
}

/**
 * 把持久化的会话还原成界面消息。
 *
 * `system` 条目不显示 —— 它是给模型的指令，不是对话内容。
 * `summary` 条目渲染成折叠的压缩提示。每条消息记录对应的
 * `Session.entries` 索引，回退/分叉时据此定位截断点。
 */
function toMessages(session: PersistedSession): Message[] {
  const messages: Message[] = [];

  session.entries.forEach((entry, entryIndex) => {
    if (entry.role === 'system') return;

    if (entry.role === 'user') {
      messages.push({
        id: crypto.randomUUID(),
        role: 'user',
        segments: [{ kind: 'text', text: entry.text }],
        tools: {},
        reasoning: '',
        status: 'done',
        retrying: null,
        entryIndex,
      });
      return;
    }

    if (entry.role === 'summary') {
      messages.push({
        id: crypto.randomUUID(),
        role: 'summary',
        segments: [],
        tools: {},
        reasoning: '',
        status: 'done',
        retrying: null,
        entryIndex,
        summary: entry.text,
      });
      return;
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
      entryIndex,
    });
  });

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

  patchActive((m) => ({
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

function makeUserMessage(text: string): Message {
  return {
    id: crypto.randomUUID(),
    role: 'user',
    segments: [{ kind: 'text', text }],
    tools: {},
    reasoning: '',
    status: 'done',
    retrying: null,
  };
}

/** 更新当前流式助手消息，而不是「最后一条」—— 排队消息可能跟在它后面。 */
function patchActive(fn: (m: Message) => Message) {
  const id = useSession.getState().activeAssistantId;
  if (!id) return;
  useSession.setState((s) => ({
    messages: s.messages.map((m) => (m.id === id ? fn(m) : m)),
  }));
}

/** 更新当前流式助手消息里的某个工具调用。 */
function patchTool(callId: string, fn: (t: ToolCall) => ToolCall) {
  patchActive((m) => {
    const existing = m.tools[callId];
    if (!existing) return m;
    return { ...m, tools: { ...m.tools, [callId]: fn(existing) } };
  });
}

function setActiveStatus(status: MessageStatus, error?: string) {
  patchActive((m) => ({ ...m, status, error, retrying: null }));
}

/**
 * 清除重试横幅。
 *
 * 任何正文/思维链/工具调用增量都只可能在重试的连接成功之后到达 ——
 * 收到它们说明这次请求活了，横幅再挂下去会与正在流出的回答自相矛盾
 * （一边说「正在重试」一边已经在出字）。
 */
function clearRetrying() {
  const { activeAssistantId, messages } = useSession.getState();
  const active = messages.find((m) => m.id === activeAssistantId);
  if (!active?.retrying) return;
  patchActive((m) => ({ ...m, retrying: null }));
}

/**
 * 启动一个真实轮次：乐观地落好占位后交给 Rust 侧，事件驱动更新。
 *
 * `queuedMessageId` 存在时说明用户气泡早已排队落盘，这里只需把它标记为
 * 已发送并在其后插入助手占位 —— 排队消息可能不止一条，助手占位必须插在
 * 自己那条用户消息后面，而不是列表末尾。
 */
async function runTurn(text: string, queuedMessageId?: string) {
  const assistantId = crypto.randomUUID();
  const assistantMsg: Message = {
    id: assistantId,
    role: 'assistant',
    segments: [],
    tools: {},
    reasoning: '',
    status: 'streaming',
    retrying: null,
  };

  discardPending();
  // 新轮次开始时清掉插队信号标记：本轮还没有发过插队信号
  useSession.setState({ preemptSignaled: false });
  useSession.setState((s) => {
    if (queuedMessageId) {
      const idx = s.messages.findIndex((m) => m.id === queuedMessageId);
      if (idx < 0) {
        // 排队气泡意外丢失（例如排队期间切了会话）：退化成普通发送
        return {
          messages: [...s.messages, makeUserMessage(text), assistantMsg],
          streaming: true,
          usage: null,
          activeAssistantId: assistantId,
          activeTurnId: null,
        };
      }
      const current = s.messages[idx];
      if (!current) {
        return {
          messages: [...s.messages, makeUserMessage(text), assistantMsg],
          streaming: true,
          usage: null,
          activeAssistantId: assistantId,
          activeTurnId: null,
        };
      }
      const messages = [...s.messages];
      messages[idx] = { ...current, pending: undefined };
      return {
        messages: [...messages.slice(0, idx + 1), assistantMsg, ...messages.slice(idx + 1)],
        streaming: true,
        usage: null,
        activeAssistantId: assistantId,
        activeTurnId: null,
      };
    }
    return {
      messages: [...s.messages, makeUserMessage(text), assistantMsg],
      streaming: true,
      usage: null,
      activeAssistantId: assistantId,
      activeTurnId: null,
    };
  });

  try {
    await sendMessage(text, handleEvent);
  } catch (e) {
    // command 本身失败（配置无效、IPC 断开等），此时可能一个事件都没收到
    flushNow();
    setActiveStatus('error', errorMessage(e));
  } finally {
    discardPending();
    // 兜底：轮次结束事件万一没送达，也不能让消息永远停在"思考中"转圈。
    // 什么都没收到时明确说出来，比一个静默的空气泡有用得多。
    useSession.setState((s) => ({
      streaming: false,
      activeAssistantId: null,
      activeTurnId: null,
      preemptSignaled: false,
      messages: s.messages.map((m) => {
        if (m.id !== assistantId) return m;
        if (m.status !== 'streaming') return m;
        const empty = m.segments.length === 0 && !m.reasoning;
        return empty
          ? { ...m, status: 'error' as const, error: '没有收到任何响应。请查看日志或重试。' }
          : { ...m, status: 'done' as const };
      }),
    }));
    // 下一条排队消息立即开跑：runTurn 会同步把 streaming 重新置真，
    // 不给「流已结束但队列未启动」的空窗留机会。
    void drainQueue();
    // 标题、消息数、更新时间都变了，同步一下列表
    await useSession.getState().refreshSessions();
  }
}

/** 依次消化排队消息。轮次刚结束时由 runTurn 的 finally 触发。 */
async function drainQueue() {
  const state = useSession.getState();
  if (state.streaming || state.queue.length === 0) return;
  const next = state.queue[0];
  if (!next) return;
  useSession.setState((s) => ({ queue: s.queue.slice(1) }));
  await runTurn(next.text, next.messageId);
}

export const useSession = create<SessionState>((set, get) => ({
  messages: [],
  streaming: false,
  model: '',
  usage: null,
  restored: false,
  sessions: [],
  currentSessionId: '',
  queue: [],
  activeAssistantId: null,
  activeTurnId: null,
  preemptSignaled: false,

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
      set({
        messages: toMessages(session),
        restored: true,
        queue: [],
        activeAssistantId: null,
        activeTurnId: null,
      });
    } catch (e) {
      console.warn('恢复会话失败', e);
      set({ restored: true });
    }
    // 无论成不成都刷新列表：会话可能为空，但工作区里也许有历史
    await get().refreshSessions();
  },

  async refreshSessions() {
    try {
      const list = await listSessions();
      set({ sessions: list.sessions, currentSessionId: list.current_id });
    } catch (e) {
      console.warn('拉取会话列表失败', e);
    }
  },

  async switchTo(id: string) {
    await switchSession(id);
    const session = await getSession();
    discardPending();
    set({
      messages: toMessages(session),
      streaming: false,
      usage: null,
      queue: [],
      activeAssistantId: null,
      activeTurnId: null,
    });
    await get().refreshSessions();
  },

  async createNew() {
    await newSession();
    discardPending();
    set({
      messages: [],
      streaming: false,
      usage: null,
      queue: [],
      activeAssistantId: null,
      activeTurnId: null,
    });
    await get().refreshSessions();
  },

  async remove(id: string) {
    const wasCurrent = id === get().currentSessionId;
    await deleteSession(id);
    if (wasCurrent) {
      discardPending();
      set({
        messages: [],
        streaming: false,
        usage: null,
        queue: [],
        activeAssistantId: null,
        activeTurnId: null,
      });
    }
    await get().refreshSessions();
  },

  async rewindTo(entryIndex: number) {
    await rewindSession(entryIndex);
    const session = await getSession();
    discardPending();
    set({
      messages: toMessages(session),
      streaming: false,
      usage: null,
      queue: [],
      activeAssistantId: null,
      activeTurnId: null,
    });
    await get().refreshSessions();
  },

  async send(text: string, mode: SendMode = 'normal') {
    const trimmed = text.trim();
    if (!trimmed) return;

    const { streaming, queue } = get();
    if (streaming || queue.length > 0) {
      // 流式进行中：normal 发送保持旧行为（忽略），排队/插队则先落一条
      // 用户气泡，等当前轮次结束后再真正交给后端。
      if (mode === 'normal') return;
      const userMsg = makeUserMessage(trimmed);
      userMsg.pending = mode;
      set((s) => ({
        messages: [...s.messages, userMsg],
        queue: [...s.queue, { text: trimmed, mode, messageId: userMsg.id }],
      }));
      if (mode === 'preempt') {
        const turnId = get().activeTurnId;
        if (turnId) {
          // turn_id 已知：立即让当前轮次在下一个工具边界停下
          set({ preemptSignaled: true });
          try {
            await preemptTurn(turnId);
          } catch (e) {
            console.warn('发送插队信号失败', e);
          }
        }
        // turn_id 未知（turn_start 还没到）：turn_start 事件到达时补发
      }
      return;
    }

    await runTurn(trimmed);
  },

  async stop() {
    await cancelTurn();
  },
}));

function handleEvent(event: AgentEvent) {
  // 事件链路的可观测性：Rust 侧的日志看不到前端有没有真的收到。
  // dev 模式下按 F12 打开控制台即可核对。
  if (import.meta.env.DEV) {
    console.debug('[agent]', event.type, event);
  }

  // 排队/插队会连续启动多个轮次，旧轮次的迟到事件绝不能打到新一轮上。
  // turn_start 负责写入当前 turn_id，context_compacted 在 turn_start 之前到达，
  // 两者放行；其余事件一律校验 turn_id。
  if (event.type !== 'turn_start' && event.type !== 'context_compacted') {
    if (useSession.getState().activeTurnId !== event.turn_id) return;
  }

  switch (event.type) {
    case 'turn_start': {
      useSession.setState({ model: event.model, activeTurnId: event.turn_id });
      // 排队里的插队消息若在 turn_start 到达前入队，插队信号还没发出去：
      // 现在补发，否则插队退化成排队，失去「提前打断」的意义。
      const { queue, preemptSignaled } = useSession.getState();
      if (!preemptSignaled && queue.some((q) => q.mode === 'preempt')) {
        useSession.setState({ preemptSignaled: true });
        void preemptTurn(event.turn_id).catch((e) => console.warn('发送插队信号失败', e));
      }
      break;
    }

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
      patchActive((m) => ({
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
      patchActive((m) => ({
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
      patchActive((m) => {
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
      setActiveStatus('error', event.message);
      break;

    case 'context_compacted': {
      // 压缩发生在 Rust 侧、TurnStart 之前。被压缩掉的是「当前轮次用户消息」
      // 之前的历史条目；排队消息可能跟在当前助手占位后面，所以这里按助手
      // 占位定位本轮边界，而不是假设末尾恰好是 user + assistant 两条。
      useSession.setState((s) => {
        const activeId = s.activeAssistantId;
        const activeIdx = activeId ? s.messages.findIndex((m) => m.id === activeId) : -1;
        if (activeIdx <= 0) return {};
        const userIdx = activeIdx - 1;
        const history = s.messages.slice(0, userIdx);
        const suffix = s.messages.slice(userIdx);
        const kept =
          history.length <= event.removed_entries ? [] : history.slice(event.removed_entries);
        const bubble: Message = {
          id: crypto.randomUUID(),
          role: 'summary',
          segments: [],
          tools: {},
          reasoning: '',
          status: 'done',
          retrying: null,
          summary: event.summary,
        };
        return { messages: [bubble, ...kept, ...suffix] };
      });
      break;
    }
  }
}
