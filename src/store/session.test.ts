import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent } from '@/lib/events';
import { flushFrames, resetFrames } from '@/test/setup';

/**
 * 脚本里的一步：投递一个事件、推进一帧、或插入一次快照。
 *
 * 之所以能插函数，是因为轮次内的中间状态（比如「增量还没提交」）
 * 在 `send()` resolve 之后已经看不到了，只能在事件流里当场记录。
 */
type Step = AgentEvent | 'frame' | (() => void);

const ipc = vi.hoisted(() => {
  type Summary = {
    id: string;
    title: string;
    workspace: string;
    model: string;
    created_at: number;
    updated_at: number;
    message_count: number;
  };
  type Entry =
    | { role: 'system' | 'user' | 'summary'; text: string }
    | { role: 'assistant'; segments: { kind: 'text'; text: string }[]; status: string };
  return {
    steps: [] as unknown[],
    sentTexts: [] as string[],
    cancelled: 0,
    preempted: 0,
    created: 0,
    deleted: [] as string[],
    switched: [] as string[],
    rewinded: [] as number[],
    sessionEntries: { entries: [] as Entry[] },
    sessionList: { current_id: '', sessions: [] as Summary[] },
  };
});

vi.mock('@/lib/ipc', () => ({
  sendMessage: async (text: string, onEvent: (e: AgentEvent) => void) => {
    ipc.sentTexts.push(text);
    for (const step of ipc.steps) {
      if (step === 'frame') flushFrames();
      else if (typeof step === 'function') (step as () => void)();
      else onEvent(step as AgentEvent);
    }
  },
  cancelTurn: async () => {
    ipc.cancelled += 1;
  },
  preemptTurn: async () => {
    ipc.preempted += 1;
  },
  getSession: async () => ipc.sessionEntries,
  listSessions: async () => ipc.sessionList,
  switchSession: async (id: string) => {
    ipc.switched.push(id);
  },
  newSession: async () => {
    ipc.created += 1;
  },
  deleteSession: async (id: string) => {
    ipc.deleted.push(id);
  },
  rewindSession: async (entryIndex: number) => {
    ipc.rewinded.push(entryIndex);
    return ipc.sessionList;
  },
}));

const { useSession } = await import('./session');
type Message = ReturnType<typeof useSession.getState>['messages'][number];

function lastMessage(): Message {
  const { messages } = useSession.getState();
  const message = messages[messages.length - 1];
  if (!message) throw new Error('消息列表是空的');
  return message;
}

const turnStart: AgentEvent = { type: 'turn_start', turn_id: 't1', model: 'gpt-4o' };
const turnEnd: AgentEvent = {
  type: 'turn_end',
  turn_id: 't1',
  finish_reason: 'stop',
  elapsed_ms: 120,
};

function delta(text: string): AgentEvent {
  return { type: 'assistant_delta', turn_id: 't1', text };
}

async function run(steps: Step[]): Promise<void> {
  ipc.steps = steps;
  await useSession.getState().send('列一下当前目录');
}

beforeEach(() => {
  resetFrames();
  ipc.steps = [];
  ipc.sentTexts = [];
  ipc.cancelled = 0;
  ipc.preempted = 0;
  ipc.created = 0;
  ipc.deleted = [];
  ipc.switched = [];
  ipc.rewinded = [];
  ipc.sessionList = { current_id: 'cur', sessions: [] };
  useSession.setState({
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
  });
});

describe('useSession.send', () => {
  it('把用户输入原样交给 Rust 侧，并先落一条用户消息', async () => {
    await run([turnStart, delta('好的'), turnEnd]);

    expect(ipc.sentTexts).toEqual(['列一下当前目录']);
    const { messages } = useSession.getState();
    expect(messages).toHaveLength(2);
    expect(messages[0]?.role).toBe('user');
    expect(messages[0]?.segments).toEqual([{ kind: 'text', text: '列一下当前目录' }]);
  });

  it('增量先攒在 rAF 缓冲里，一帧只提交一次并合并成同一段', async () => {
    const snapshots: unknown[][] = [];
    const snap = () => snapshots.push(lastMessage().segments);

    await run([turnStart, delta('思'), delta('考'), snap, 'frame', snap, turnEnd]);

    // 帧未推进时一个字都不该落到 state
    expect(snapshots[0]).toEqual([]);
    // 两次增量在同一帧内合并，产生一段而非两段
    expect(snapshots[1]).toEqual([{ kind: 'text', text: '思考' }]);
  });

  it('文本与工具卡片按到达顺序交错，不被拍平成两个数组', async () => {
    await run([
      turnStart,
      delta('先看看目录'),
      { type: 'tool_call_start', turn_id: 't1', call_id: 'c1', name: 'Bash' },
      {
        type: 'tool_call_ready',
        turn_id: 't1',
        call_id: 'c1',
        name: 'Bash',
        args: { command: 'ls' },
        preview: 'ls',
      },
      { type: 'tool_progress', turn_id: 't1', call_id: 'c1', stream: 'stdout', chunk: 'src\n' },
      { type: 'tool_progress', turn_id: 't1', call_id: 'c1', stream: 'stdout', chunk: 'README\n' },
      {
        type: 'tool_result',
        turn_id: 't1',
        call_id: 'c1',
        ok: true,
        duration_ms: 42,
        result: { kind: 'text', content: 'src\nREADME\n', truncated: false },
      },
      delta('目录里有两项'),
      turnEnd,
    ]);

    const message = lastMessage();
    expect(message.segments).toEqual([
      { kind: 'text', text: '先看看目录' },
      { kind: 'tool', callId: 'c1' },
      { kind: 'text', text: '目录里有两项' },
    ]);

    const call = message.tools['c1'];
    expect(call?.preview).toBe('ls');
    expect(call?.status).toBe('ok');
    expect(call?.durationMs).toBe(42);
    // 流式输出按到达顺序累加，命令跑完前就能看到
    expect(call?.liveOutput).toBe('src\nREADME\n');
  });

  it('把 usage 的 snake_case 字段映射成前端的 camelCase', async () => {
    await run([
      turnStart,
      {
        type: 'usage',
        turn_id: 't1',
        prompt_tokens: 1200,
        completion_tokens: 300,
        total_tokens: 1500,
        context_used: 1200,
        context_limit: 128000,
        elapsed_ms: 2400,
        tps: 12.5,
      },
      delta('好'),
      turnEnd,
    ]);

    expect(useSession.getState().usage).toEqual({
      promptTokens: 1200,
      completionTokens: 300,
      totalTokens: 1500,
      contextUsed: 1200,
      contextLimit: 128000,
      elapsedMs: 2400,
      tps: 12.5,
    });
  });

  it('取消结束的轮次标成 cancelled 而不是 done', async () => {
    await run([
      turnStart,
      delta('刚说了一半'),
      { type: 'turn_end', turn_id: 't1', finish_reason: 'cancelled', elapsed_ms: 30 },
    ]);

    const message = lastMessage();
    expect(message.status).toBe('cancelled');
    // 已经说出口的内容要保留，不能连同取消一起抹掉
    expect(message.segments).toEqual([{ kind: 'text', text: '刚说了一半' }]);
  });

  it('error 事件让消息进入 error 态并带上原因', async () => {
    await run([
      turnStart,
      { type: 'error', turn_id: 't1', code: 'llm', message: '上游返回 429', retryable: true },
    ]);

    expect(lastMessage().status).toBe('error');
    expect(lastMessage().error).toBe('上游返回 429');
  });

  it('error 后跟 turn_end 不得把错误状态洗成 done', async () => {
    await run([
      turnStart,
      { type: 'error', turn_id: 't1', code: 'llm', message: '上游返回 429', retryable: true },
      { type: 'turn_end', turn_id: 't1', finish_reason: 'error', elapsed_ms: 500 },
    ]);

    // 错误原因必须保留，否则用户看到的就是「静默停止、没有任何提示」
    expect(lastMessage().status).toBe('error');
    expect(lastMessage().error).toBe('上游返回 429');
  });

  it('取消后跟 turn_end 保持 cancelled 状态', async () => {
    await run([
      turnStart,
      delta('刚说了一半'),
      { type: 'turn_end', turn_id: 't1', finish_reason: 'cancelled', elapsed_ms: 30 },
    ]);

    expect(lastMessage().status).toBe('cancelled');
    expect(lastMessage().segments).toEqual([{ kind: 'text', text: '刚说了一半' }]);
  });

  it('retry 事件记录失败原因与次数，正文一到就清除横幅', async () => {
    const seen: unknown[] = [];
    await run([
      turnStart,
      {
        type: 'retry',
        turn_id: 't1',
        attempt: 2,
        max_retries: 3,
        message: 'HTTP 503：上游繁忙',
        retry_after_ms: 4000,
      },
      () => seen.push(lastMessage().retrying),
      delta('重试成功'),
      'frame',
      () => seen.push(lastMessage().retrying),
      turnEnd,
    ]);

    expect(seen[0]).toEqual({
      attempt: 2,
      maxRetries: 3,
      message: 'HTTP 503：上游繁忙',
      retryAfterMs: 4000,
    });
    // 正文只能在重试的连接成功之后到达，横幅不该再挂着
    expect(seen[1]).toBeNull();
    expect(lastMessage().status).toBe('done');
    expect(lastMessage().segments).toEqual([{ kind: 'text', text: '重试成功' }]);
  });

  it('无限重试时 maxRetries 为 null 原样保留', async () => {
    const seen: unknown[] = [];
    await run([
      turnStart,
      {
        type: 'retry',
        turn_id: 't1',
        attempt: 1,
        max_retries: null,
        message: '连接被拒绝',
        retry_after_ms: 1000,
      },
      () => seen.push(lastMessage().retrying),
      turnEnd,
    ]);

    expect(seen[0]).toMatchObject({ attempt: 1, maxRetries: null });
    // 轮次结束（含取消）也必须清掉横幅
    expect(lastMessage().retrying).toBeNull();
  });

  it('重试耗尽后的 error 事件清掉横幅并进入错误态', async () => {
    await run([
      turnStart,
      {
        type: 'retry',
        turn_id: 't1',
        attempt: 1,
        max_retries: 1,
        message: 'HTTP 503：service busy',
        retry_after_ms: 2000,
      },
      {
        type: 'error',
        turn_id: 't1',
        code: 'retryable',
        message: '请求失败：HTTP 503：service busy',
        retryable: true,
      },
    ]);

    expect(lastMessage().retrying).toBeNull();
    expect(lastMessage().status).toBe('error');
    expect(lastMessage().error).toContain('service busy');
  });

  it('一个事件都没收到时明确报错，而不是永远停在思考中', async () => {
    await run([]);

    const message = lastMessage();
    expect(message.status).toBe('error');
    expect(message.error).toContain('没有收到任何响应');
    expect(useSession.getState().streaming).toBe(false);
  });

  it('轮次进行中重复发送会被忽略', async () => {
    useSession.setState({ streaming: true });
    await run([turnStart, delta('x'), turnEnd]);

    expect(ipc.sentTexts).toEqual([]);
    expect(useSession.getState().messages).toEqual([]);
  });

  it('流式进行中排队：立即落 pending 气泡，不打断当前回复', async () => {
    useSession.setState({ streaming: true });
    await useSession.getState().send('第二条', 'queue');

    const s = useSession.getState();
    expect(s.messages).toHaveLength(1);
    expect(s.messages[0]).toMatchObject({ role: 'user', pending: 'queue' });
    expect(s.messages[0]?.segments).toEqual([{ kind: 'text', text: '第二条' }]);
    expect(s.queue).toHaveLength(1);
    expect(s.streaming).toBe(true);
    // 还没交给后端，等当前轮次结束才发
    expect(ipc.sentTexts).toEqual([]);
  });

  it('流式进行中插队：turn_start 到达前入队，到达后补发 preemptTurn 信号', async () => {
    const seen: number[] = [];
    let queuedOnce = false;
    ipc.steps = [
      () => {
        // 在 turn_start 事件到达之前插入插队消息 —— turn_id 未知，信号只能延后
        if (!queuedOnce) {
          queuedOnce = true;
          void useSession.getState().send('急事', 'preempt');
        }
        seen.push(ipc.preempted);
      },
      turnStart,
      delta('第一轮'),
      turnEnd,
    ];
    const first = useSession.getState().send('第一条');

    expect(seen[0]).toBe(0);

    await first;
    // turn_start 到达时补发插队信号，且队列自动消化插队消息
    expect(ipc.preempted).toBe(1);
    expect(ipc.sentTexts).toEqual(['第一条', '急事']);
  });
});

describe('useSession 会话管理', () => {
  it('refreshSessions 把摘要写入 state 并记下当前会话 id', async () => {
    ipc.sessionList = {
      current_id: 'b',
      sessions: [
        {
          id: 'a',
          title: '旧',
          workspace: '/w',
          model: 'm',
          created_at: 1,
          updated_at: 2,
          message_count: 3,
        },
        {
          id: 'b',
          title: '新',
          workspace: '/w',
          model: 'm',
          created_at: 4,
          updated_at: 5,
          message_count: 1,
        },
      ],
    };

    await useSession.getState().refreshSessions();

    expect(useSession.getState().currentSessionId).toBe('b');
    expect(useSession.getState().sessions).toHaveLength(2);
    expect(useSession.getState().sessions[1]?.title).toBe('新');
  });

  it('switchTo 通知 Rust 切换并重拉会话内容', async () => {
    await useSession.getState().switchTo('a');

    expect(ipc.switched).toEqual(['a']);
  });

  it('createNew 通知 Rust 新建并清空界面', async () => {
    await run([turnStart, delta('旧内容'), turnEnd]);
    await useSession.getState().createNew();

    expect(ipc.created).toBe(1);
    expect(useSession.getState().messages).toEqual([]);
  });

  it('删除当前会话时清空界面，删除非当前会话则保留', async () => {
    ipc.sessionList = { current_id: 'cur', sessions: [] };
    await useSession.getState().refreshSessions();
    await run([turnStart, delta('当前会话内容'), turnEnd]);
    expect(useSession.getState().currentSessionId).toBe('cur');

    await useSession.getState().remove('cur');
    expect(ipc.deleted).toEqual(['cur']);
    expect(useSession.getState().messages).toEqual([]);

    await useSession.getState().remove('other');
    expect(ipc.deleted).toEqual(['cur', 'other']);
  });

  it('rewindTo 通知 Rust 截断，并用返回的会话重建界面', async () => {
    // 后端截断后的会话：system + 保留到分叉点的消息
    ipc.sessionEntries = {
      entries: [
        { role: 'system', text: 'sys' },
        { role: 'user', text: '第一问' },
        { role: 'assistant', segments: [{ kind: 'text', text: '第一答' }], status: 'done' },
      ],
    };
    await useSession.getState().rewindTo(2);

    expect(ipc.rewinded).toEqual([2]);
    const { messages } = useSession.getState();
    expect(messages).toHaveLength(2);
    expect(messages[0]?.role).toBe('user');
    expect(messages[0]?.entryIndex).toBe(1);
    expect(messages[1]?.entryIndex).toBe(2);
  });

  it('restore 把 summary 条目还原成摘要气泡', async () => {
    ipc.sessionEntries = {
      entries: [
        { role: 'system', text: 'sys' },
        { role: 'summary', text: '（历史摘要）' },
        { role: 'user', text: '继续' },
      ],
    };
    await useSession.getState().restore();

    const { messages } = useSession.getState();
    expect(messages).toHaveLength(2);
    expect(messages[0]?.role).toBe('summary');
    expect(messages[0]?.summary).toBe('（历史摘要）');
  });
});

describe('useSession 上下文压缩', () => {
  it('context_compacted 把历史消息替换成摘要气泡，保留流式占位', async () => {
    // 先跑一轮造出历史（2 条消息：user + assistant）
    await run([turnStart, delta('旧回答'), turnEnd]);
    expect(useSession.getState().messages).toHaveLength(2);

    // 第二轮：压缩事件先到（Rust 侧在 TurnStart 之前发），
    // 末尾两条是 user + assistant 流式占位，前 2 条历史被压缩
    ipc.steps = [
      {
        type: 'context_compacted',
        turn_id: 't2',
        removed_entries: 2,
        summary: '（压缩摘要）',
      },
      turnStart,
      delta('新回答'),
      turnEnd,
    ];
    await useSession.getState().send('第二条');

    const { messages, streaming } = useSession.getState();
    expect(streaming).toBe(false);
    expect(messages).toHaveLength(3);
    expect(messages[0]?.role).toBe('summary');
    expect(messages[0]?.summary).toBe('（压缩摘要）');
    // 末尾两条是刚发的 user 和 assistant 回答
    expect(messages[1]?.role).toBe('user');
    expect(messages[1]?.segments[0]).toEqual({ kind: 'text', text: '第二条' });
    expect(messages[2]?.role).toBe('assistant');
    expect(messages[2]?.segments[0]).toEqual({ kind: 'text', text: '新回答' });
  });
});
