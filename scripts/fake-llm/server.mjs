#!/usr/bin/env node
// OpenAI 兼容协议的假服务端，用固定 fixture 回放流式响应。
//
// 存在的意义是让功能测试有确定性的真实数据：同样的输入永远得到同样的
// 事件序列，既能被 Rust 集成测试断言，也能让人对着 GUI 肉眼核对。
//
// 用法：
//   node scripts/fake-llm/server.mjs              # 回放模式，监听 8787
//   node scripts/fake-llm/server.mjs --port 0     # 随机端口，就绪后打印到 stdout
//   node scripts/fake-llm/server.mjs --record <id> [--title ..] [--match a,b]
//
// 回放模式的场景选择优先级：
//   1. 请求里的 model 恰好等于某个场景 id  —— 自动化测试用，绝对确定
//   2. 最后一条 user 消息命中某场景的 match 关键词 —— 手动测试用，说人话即可
//   3. 都不中则回落到 basic-chat
//
// 录制模式把请求转发给 config.json 里的真实 LLM，**逐字透传**响应给应用，
// 同时旁路抓成 fixture。透传而非缓冲是关键：应用照常流式渲染、照常执行真工具、
// 照常发起下一轮，于是多轮工具流程和真实的分片节奏都被原样记录下来。

import { createServer } from 'node:http';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURE_DIR = path.join(HERE, 'fixtures');
const CONFIG_FILE = path.join(HERE, 'config.json');
const TEMPLATE_FILE = path.join(HERE, 'config.template.json');
const FALLBACK_SCENARIO = 'basic-chat';

/** 读入全部 fixture，按 id 建索引。启动时一次性完成，请求路径上不碰磁盘。 */
function loadScenarios() {
  const scenarios = new Map();
  for (const file of readdirSync(FIXTURE_DIR)) {
    if (!file.endsWith('.json')) continue;
    const scenario = JSON.parse(readFileSync(path.join(FIXTURE_DIR, file), 'utf8'));
    scenarios.set(scenario.id, scenario);
  }
  if (!scenarios.has(FALLBACK_SCENARIO)) {
    throw new Error(`缺少兜底场景 ${FALLBACK_SCENARIO}.json`);
  }
  return scenarios;
}

const SCENARIOS = loadScenarios();

// 「先失败 N 次再成功」场景的失败计数器，按场景 id 记。
// 服务端因此带了一点状态 —— 但只在这个 fixture 显式配置 httpBeforeOk 时启用，
// 其他场景完全不受影响；自动化测试每个用例起独立进程，计数器必然从 0 开始，
// 不存在跨用例污染。手动测试重启 server 即可归零。
const FAIL_COUNTS = new Map();

function parseArgs(argv) {
  const options = { port: 8787, host: '127.0.0.1', record: null, title: '', match: [] };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--port') options.port = Number(argv[i + 1]);
    if (argv[i] === '--host') options.host = String(argv[i + 1]);
    if (argv[i] === '--record') options.record = String(argv[i + 1] ?? '').trim();
    if (argv[i] === '--title') options.title = String(argv[i + 1] ?? '');
    if (argv[i] === '--match') {
      options.match = String(argv[i + 1] ?? '')
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean);
    }
  }
  return options;
}

/**
 * 读取连真实 LLM 的配置。
 *
 * 只在录制模式下调用 —— 回放模式不该因为没配密钥就跑不起来，
 * 那会让 CI 和刚 clone 仓库的人平白多一步。
 */
function loadConfig() {
  if (!existsSync(CONFIG_FILE)) {
    throw new Error(
      `缺少 ${path.relative(process.cwd(), CONFIG_FILE)}。\n` +
        `先复制模板再填密钥：\n` +
        `  cp ${path.relative(process.cwd(), TEMPLATE_FILE)} ${path.relative(process.cwd(), CONFIG_FILE)}`,
    );
  }
  const config = JSON.parse(readFileSync(CONFIG_FILE, 'utf8'));
  for (const key of ['base_url', 'model']) {
    if (!String(config[key] ?? '').trim()) throw new Error(`config.json 缺少 ${key}`);
  }
  if (!String(config.api_key ?? '').trim()) {
    console.error('[fake-llm] 警告：config.json 的 api_key 为空，仅本地网关能用');
  }
  return config;
}

/** 与应用侧 `LlmConfig::endpoint()` 保持同样的归一化规则。 */
function endpointOf(config) {
  const base = String(config.base_url).trim().replace(/\/+$/, '');
  return base.endsWith('/chat/completions') ? base : `${base}/chat/completions`;
}

function pickScenario(body) {
  const byModel = SCENARIOS.get(String(body.model ?? '').trim());
  if (byModel) return byModel;

  const messages = Array.isArray(body.messages) ? body.messages : [];
  const lastUser = [...messages].reverse().find((m) => m.role === 'user');
  const text = String(lastUser?.content ?? '').toLowerCase();
  for (const scenario of SCENARIOS.values()) {
    if ((scenario.match ?? []).some((kw) => text.includes(String(kw).toLowerCase()))) {
      return scenario;
    }
  }
  return SCENARIOS.get(FALLBACK_SCENARIO);
}

/**
 * 该回放第几轮。
 *
 * 用历史里的 assistant 工具调用条数推算，而不是在服务端存计数器 ——
 * 服务端一旦有状态，测试就得关心执行顺序，重试也会错位。
 */
function roundIndex(body) {
  const messages = Array.isArray(body.messages) ? body.messages : [];
  const lastUserAt = messages.map((m) => m.role).lastIndexOf('user');
  return messages
    .slice(lastUserAt + 1)
    .filter((m) => m.role === 'assistant' && Array.isArray(m.tool_calls)).length;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 按 style 把一个工具调用拆成若干 delta 分片。 */
function toolCallDeltas(call, index, style) {
  const micro = style === 'no-index-micro';
  const withIndex = (delta) => (micro ? delta : { index, ...delta });

  const head = withIndex({
    id: call.id,
    type: 'function',
    function: { name: call.name, arguments: '' },
  });

  const args = String(call.arguments ?? '');
  // 规范只保证 index 稳定：id 与 name 通常只在首帧出现，之后各帧只带 arguments 片段。
  // micro 模式逐字符切，专门用来压 accumulator 的拼接逻辑。
  const pieces = micro
    ? [...args]
    : [args.slice(0, Math.ceil(args.length / 3)), args.slice(Math.ceil(args.length / 3))];

  return [head, ...pieces.filter(Boolean).map((p) => withIndex({ function: { arguments: p } }))];
}

function chunk(model, delta, finishReason = null) {
  return {
    id: 'chatcmpl-fake',
    object: 'chat.completion.chunk',
    created: 1700000000,
    model,
    choices: [{ index: 0, delta, finish_reason: finishReason }],
  };
}

async function streamRound(res, model, scenario, round) {
  const delay = Number(scenario.delayMs ?? 0);
  // 客户端取消时连接会断，必须停止推送 —— 否则定时器会一直挂着，
  // cancellable 场景跑完一次就会泄漏几十个 timer。
  let aborted = false;
  res.on('close', () => {
    aborted = true;
  });

  const send = async (payload) => {
    if (aborted) return false;
    res.write(`data: ${JSON.stringify(payload)}\n\n`);
    if (delay > 0) await sleep(delay);
    return !aborted;
  };

  if (!(await send(chunk(model, { role: 'assistant', content: '' })))) return;

  for (const text of round.reasoning ?? []) {
    if (!(await send(chunk(model, { reasoning_content: text })))) return;
  }

  let injected = false;
  for (const text of round.content ?? []) {
    if (!(await send(chunk(model, { content: text })))) return;
    if (scenario.injectBadFrame && !injected) {
      injected = true;
      // 故意的非法 JSON：客户端应当跳过这一帧并继续，而不是终止整条流
      res.write('data: {"choices": [ THIS IS NOT JSON\n\n');
    }
  }

  const calls = round.toolCalls ?? [];
  for (const [index, call] of calls.entries()) {
    for (const delta of toolCallDeltas(call, index, scenario.toolCallStyle)) {
      if (!(await send(chunk(model, { tool_calls: [delta] })))) return;
    }
  }

  if (!(await send(chunk(model, {}, round.finishReason ?? 'stop')))) return;

  if (round.usage) {
    const payload = { ...chunk(model, {}), choices: [], usage: round.usage };
    if (!(await send(payload))) return;
  }

  if (!aborted) res.write('data: [DONE]\n\n');
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let raw = '';
    req.on('data', (piece) => {
      raw += piece;
    });
    req.on('end', () => resolve(raw));
    req.on('error', reject);
  });
}

function sendJson(res, status, payload) {
  const body = JSON.stringify(payload);
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': Buffer.byteLength(body),
  });
  res.end(body);
}

// ---------- 录制 ----------

function newCapture() {
  return { reasoning: [], content: [], calls: new Map(), finishReason: null, usage: null };
}

/** 把上游的一帧并进抓取结果。分片规则与 `llm/accumulator.rs` 保持一致。 */
function absorb(capture, chunk) {
  if (chunk.usage) capture.usage = chunk.usage;
  for (const choice of chunk.choices ?? []) {
    const delta = choice.delta ?? {};
    if (delta.content) capture.content.push(delta.content);
    if (delta.reasoning_content) capture.reasoning.push(delta.reasoning_content);
    for (const piece of delta.tool_calls ?? []) {
      // 规范只保证 index 稳定；少数网关不发 index，隐含单个调用
      const key = piece.index ?? 0;
      const call = capture.calls.get(key) ?? { id: '', name: '', arguments: '' };
      if (piece.id) call.id = piece.id;
      if (piece.function?.name) call.name = piece.function.name;
      if (piece.function?.arguments) call.arguments += piece.function.arguments;
      capture.calls.set(key, call);
    }
    if (choice.finish_reason) capture.finishReason = choice.finish_reason;
  }
}

function toRound(capture) {
  const round = {};
  if (capture.reasoning.length) round.reasoning = capture.reasoning;
  if (capture.content.length) round.content = capture.content;
  const calls = [...capture.calls.entries()].sort((a, b) => a[0] - b[0]).map(([, c]) => c);
  if (calls.length) round.toolCalls = calls;
  round.finishReason = capture.finishReason ?? 'stop';
  if (capture.usage) round.usage = capture.usage;
  return round;
}

/**
 * 写入一轮。每轮结束就落盘，中途 Ctrl-C 也能留下已录到的部分。
 *
 * 第 0 轮会清空旧的 rounds —— 重录时若只覆盖前几轮，尾巴上会残留上一次的轮次，
 * 回放时表现成「模型突然聊起了别的」。
 */
function saveRound(id, options, index, round) {
  const file = path.join(FIXTURE_DIR, `${id}.json`);
  const existing = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : {};

  const scenario = { ...existing, id };
  if (options.title) scenario.title = options.title;
  else scenario.title ??= id;
  if (options.match.length) scenario.match = options.match;
  else scenario.match ??= [];

  scenario.rounds = index === 0 ? [] : (scenario.rounds ?? []);
  scenario.rounds[index] = round;

  writeFileSync(file, `${JSON.stringify(scenario, null, 2)}\n`, 'utf8');
}

/**
 * 转发到真实 LLM 并旁路录制。
 *
 * 应用发来的请求体原样转发 —— 工具定义和完整历史都在里面，服务端不必自己拼。
 * 只把 model 换成 config 里的，免得受设置页填的场景名影响。
 */
async function proxyAndRecord(res, body, config, options) {
  const index = roundIndex(body);
  const payload = { ...body, model: config.model };
  if (config.temperature === null || config.temperature === undefined) delete payload.temperature;
  else payload.temperature = config.temperature;

  const upstream = await fetch(endpointOf(config), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(String(config.api_key ?? '').trim()
        ? { Authorization: `Bearer ${String(config.api_key).trim()}` }
        : {}),
    },
    body: JSON.stringify(payload),
  });

  if (!upstream.ok) {
    const text = await upstream.text();
    console.error(`[fake-llm] 上游 HTTP ${upstream.status}：${text.slice(0, 300)}`);
    res.writeHead(upstream.status, { 'Content-Type': 'application/json; charset=utf-8' });
    res.end(text);
    return;
  }

  res.writeHead(200, {
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
  });

  let aborted = false;
  res.on('close', () => {
    aborted = true;
  });

  const capture = newCapture();
  const reader = upstream.body.getReader();
  const decoder = new TextDecoder();
  let pending = '';

  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (aborted) {
      await reader.cancel();
      break;
    }
    // 逐字透传：不改分片、不改时序，应用看到的与直连真实 API 完全一致
    res.write(Buffer.from(value));

    pending += decoder.decode(value, { stream: true });
    const events = pending.split('\n\n');
    pending = events.pop() ?? '';
    for (const event of events) {
      const line = event.split('\n').find((l) => l.startsWith('data:'));
      if (!line) continue;
      const data = line.slice(5).trim();
      if (!data || data === '[DONE]') continue;
      try {
        absorb(capture, JSON.parse(data));
      } catch {
        // 上游偶发非标准帧（心跳、注释），跳过即可 —— 客户端也是这么做的
      }
    }
  }

  if (aborted) {
    console.error(`[fake-llm] ${options.record} 第 ${index + 1} 轮被取消，不落盘`);
    return;
  }

  // 必须在这里收尾：调用方走的是提前 return 的分支，够不到末尾那个 res.end()。
  // 少了它客户端会一直等下去 —— 表现成「应用发完消息就卡住」，很难往这里想。
  res.end();

  saveRound(options.record, options, index, toRound(capture));
  console.error(
    `[fake-llm] 已录制 ${options.record} 第 ${index + 1} 轮 → fixtures/${options.record}.json`,
  );
}

async function handleChat(req, res) {
  const raw = await readBody(req);
  let body;
  try {
    body = JSON.parse(raw);
  } catch {
    sendJson(res, 400, { error: { message: '请求体不是合法 JSON' } });
    return;
  }

  if (OPTIONS.record) {
    await proxyAndRecord(res, body, REAL_CONFIG, OPTIONS);
    return;
  }

  const scenario = pickScenario(body);
  const model = String(body.model ?? 'fake-model');

  // 重试测试专用：前 `failures` 次请求一律返回可重试的 HTTP 错误，
  // 之后才正常回放 —— 客户端必须按配置退避重试才能拿到正文。
  if (scenario.httpBeforeOk) {
    const failed = FAIL_COUNTS.get(scenario.id) ?? 0;
    if (failed < scenario.httpBeforeOk.failures) {
      FAIL_COUNTS.set(scenario.id, failed + 1);
      console.error(
        `[fake-llm] ${scenario.id} 第 ${failed + 1} 次请求返回 HTTP ${scenario.httpBeforeOk.status}`,
      );
      const body = scenario.httpBeforeOk.body ?? {
        error: { message: 'temporary upstream failure' },
      };
      sendJson(res, scenario.httpBeforeOk.status, body);
      return;
    }
  }

  if (scenario.http) {
    console.error(`[fake-llm] ${scenario.id} → HTTP ${scenario.http.status}`);
    sendJson(res, scenario.http.status, scenario.http.body);
    return;
  }

  const index = roundIndex(body);
  const round = scenario.rounds?.[index];
  console.error(`[fake-llm] ${scenario.id} 第 ${index + 1} 轮`);

  res.writeHead(200, {
    'Content-Type': 'text/event-stream; charset=utf-8',
    'Cache-Control': 'no-cache',
    Connection: 'keep-alive',
  });

  if (!round) {
    // 轮次用尽仍被追问：给一句收口的话，而不是空流让客户端干等
    await streamRound(res, model, scenario, {
      content: ['（fixture 的轮次已用尽）'],
      finishReason: 'stop',
    });
  } else {
    await streamRound(res, model, scenario, round);
  }
  res.end();
}

const OPTIONS = parseArgs(process.argv.slice(2));
const { port, host } = OPTIONS;
// 录制模式才需要真实密钥。回放模式不该因为没配 config.json 就跑不起来 ——
// 那会让 CI 和刚 clone 仓库的人平白多一步。
const REAL_CONFIG = OPTIONS.record ? loadConfig() : null;

const server = createServer((req, res) => {
  const url = new URL(req.url ?? '/', `http://${host}`);

  if (req.method === 'GET' && url.pathname === '/scenarios') {
    sendJson(
      res,
      200,
      [...SCENARIOS.values()].map((s) => ({
        id: s.id,
        title: s.title,
        match: s.match ?? [],
        rounds: s.rounds?.length ?? 0,
      })),
    );
    return;
  }

  if (req.method === 'POST' && url.pathname.endsWith('/chat/completions')) {
    handleChat(req, res).catch((e) => {
      console.error('[fake-llm] 处理失败', e);
      if (!res.headersSent) sendJson(res, 500, { error: { message: String(e) } });
      else res.end();
    });
    return;
  }

  sendJson(res, 404, { error: { message: `未处理的路径 ${req.method} ${url.pathname}` } });
});

server.listen(port, host, () => {
  const actual = server.address();
  if (OPTIONS.record) {
    console.error(
      `[fake-llm] 录制模式：转发到 ${endpointOf(REAL_CONFIG)}（模型 ${REAL_CONFIG.model}）\n` +
        `[fake-llm] 结果写入 fixtures/${OPTIONS.record}.json` +
        (OPTIONS.match.length ? '' : '，match 关键词为空，回放时只能用模型名选中'),
    );
  }
  // 这一行是与 Rust 集成测试的约定：--port 0 时对方靠它拿到真实端口。
  // 走 stdout 且立刻 flush，其余日志一律走 stderr，避免污染。
  process.stdout.write(`FAKE_LLM_READY http://${host}:${actual.port}/v1\n`);
});
