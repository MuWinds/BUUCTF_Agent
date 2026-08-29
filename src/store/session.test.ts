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

const ipc = vi.hoisted(() => ({
  steps: [] as unknown[],
  sentTexts: [] as string[],
  cancelled: 0,
  cleared: 0,
}));

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
  clearHistory: async () => {
    ipc.cleared += 1;
  },
  getSession: async () => ({ entries: [] }),
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
  ipc.cleared = 0;
  useSession.setState({ messages: [], streaming: false, model: '', usage: null, restored: false });
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
});

describe('useSession.reset', () => {
  it('清空界面并通知 Rust 侧丢弃历史', async () => {
    await run([turnStart, delta('先说点什么'), turnEnd]);
    await useSession.getState().reset();

    expect(ipc.cleared).toBe(1);
    expect(useSession.getState().messages).toEqual([]);
    expect(useSession.getState().usage).toBeNull();
  });
});
