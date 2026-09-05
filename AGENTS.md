# AGENTS.md

通用 Coding Agent。Tauri v2 外壳，Rust 内核 + React 前端，只支持 OpenAI 兼容协议
（可配 base_url / api_key / model）。

---

## Commands

Rust 工具链装在 `~/.cargo`，未加入全局 PATH。跑 cargo 前先：

```bash
export PATH="$USERPROFILE/.cargo/bin:$PATH"
```

### 日常开发

```bash
pnpm install                 # 装前端依赖
pnpm tauri dev               # 开发模式（Rust 改动自动重编）
pnpm tauri build             # 出 NSIS 安装包
```

### 提交前必须全绿

前端和 Rust 各一条，两条都得跑：

```bash
pnpm check                   # = typecheck + format:check + test
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

### 单项命令

| 命令 | 作用 |
| --- | --- |
| `pnpm typecheck` | `tsc --noEmit`，严格模式类型检查 |
| `pnpm format` | Prettier 就地格式化 |
| `pnpm format:check` | 只检查不改写，用于 CI 与提交前 |
| `pnpm test` | Vitest 跑一次 |
| `pnpm test:watch` | Vitest watch 模式 |
| `pnpm fake-llm` | 起假 LLM 服务端（127.0.0.1:8787），手动测 GUI 用 |
| `pnpm fake-llm:record <id>` | 录制模式：转发到真实 LLM 并把回答抓成 fixture |
| `cargo test --test e2e` | 只跑端到端测试 |
| `cargo fmt --all` | rustfmt 就地格式化 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 静态检查，告警即失败 |
| `cargo test --workspace` | 全部 Rust 测试 |
| `cargo test --doc -p agent-core` | 只验证 core 的文档示例 |
| `cargo check --workspace` | 快速编译检查，不产出二进制 |

---

## Testing

### 现状

| 层 | 框架 | 数量 | 覆盖什么 |
| --- | --- | --- | --- |
| `agent-core` | `cargo test` | 56 | SSE 累加、轮次循环、会话投影、配置校验、回退截断、自动压缩 |
| `agent-host` | `cargo test` | 107 | 各工具（bash / edit / grep / glob / read / write / diff）、路径边界、持久化、凭据、上下文文件 |
| `agent-tui` | `cargo test` | 16 | 事件→展示模型映射、工具卡片生命周期、流式 markdown、输入编辑、真服务端到端链路、取消 |
| `src-tauri` | `cargo test` | 0 | 纯装配层无业务逻辑，工具/持久化测试已随实现迁入 `agent-host` |
| 端到端 | `cargo test --test e2e` | 18 | 真协议 + 真工具 + 真循环，断言推给 UI 的事件序列，含自动压缩链路 |
| 文档示例 | `cargo test --doc` | 1 | `agent-core` 的 `lib.rs` 用例能编译 |
| 前端 | `vitest` | 28 | session store 行为、事件映射、纯函数、样式书写约束 |
| GUI | 人工 | 14 场景 | `docs/manual-gui-checklist.md`，只测自动化测不到的渲染与手感 |

### 测试数据只有一份

假 LLM 服务端在 `scripts/fake-llm/`，详见那里的 README。它同时服务两件事：

- `src-tauri/tests/e2e.rs` 起 `--port 0` 的独占实例，断言事件序列
- 手动测 GUI 时 `pnpm fake-llm`，把应用指过去照着清单点

**fixture 和沙箱数据两边共用**，才不会出现「自动化通过但手动点出来是另一回事」。
改 `scripts/fake-llm/sandbox/` 里的文件，`e2e.rs` 的断言要一起改。

fixture 可以手写，也可以**从真实 LLM 录**：`pnpm fake-llm:record <id>` 会把请求
转发给 `scripts/fake-llm/config.json` 里配的真实 API，逐字透传给应用的同时抓成
fixture。透传而非缓冲，应用才会照常执行真工具、照常发起下一轮 —— 多轮流程和真实
分片节奏就都被原样录下来了。用前先 `cp config.template.json config.json` 填密钥，
那个文件已在 `.gitignore` 里。

端到端测试**断言事件而不是断言返回值** —— 事件流才是前端唯一能看到的东西，
返回值对了但事件序列错了，界面照样是坏的。

### 写测试的规矩

- **Rust 测试与被测代码同文件**，放在文件末尾的 `#[cfg(test)] mod tests` 里。
  测试名用陈述句描述行为（`kills_the_whole_process_tree`），不写 `test_xxx`。
- **测试文档字符串用中文 `///`**，写清楚「为什么这个行为必须成立」，而不是复述断言。
- **前端只测纯逻辑，不渲染组件**。测试环境是 `node` 而非 happy-dom —— 少一个依赖，
  且 `src/test/setup.ts` 手动接管 `requestAnimationFrame` 后帧时机完全可控，
  「攒了几帧」「取消是否真的丢弃增量」都能确定性断言。
- **测 store 走公开接口**，用 `vi.mock('@/lib/ipc')` 投递脚本化事件流，
  不要为了测试把内部函数导出 —— 导出面变大比测不到更糟。
- 文件命名 `*.test.ts`，与被测模块同目录。
- **能用测试守住的约定就别只写进文档**。`src/test/styles.test.ts` 就是这么一个
  「lint 型」测试：扫源码找违规写法，断言失败时直接给出 `文件:行号`。
  新增这类守卫时先故意注入一次违规，确认它真的会红。

### 每类改动的验证方式

| 改了什么 | 至少要跑 |
| --- | --- |
| 工具实现（`crates/agent-host/src/tools/`） | `cargo test --workspace` |
| 事件协议（`events.rs` + `events.ts`） | 两侧都改 + `pnpm check` + `cargo test` |
| 前端 store / 纯函数 | `pnpm check` |
| LLM 请求响应结构 | `cargo test -p agent-core` + `cargo test --test e2e` |
| TUI 事件映射 / 状态机 | `cargo test -p agent-tui` |
| GUI 呈现 | `pnpm fake-llm` + `pnpm tauri dev`，照 `docs/manual-gui-checklist.md` 走 |
| TUI 呈现 | `pnpm fake-llm` + `cargo run -p agent-tui`，真终端里看 |

---

## Project Structure

```
crates/agent-core/        # 可复用核心，不依赖任何 GUI 框架
  llm/                    # OpenAI 兼容流式客户端（reqwest + eventsource-stream）
    accumulator.rs        # 工具调用分片的累加器
    types.rs              # 请求/响应结构
  turn.rs                 # 轮次循环，无状态函数，产出 TurnOutcome
  compact.rs              # 长对话自动压缩：估算 token、折叠最老条目为摘要
  session.rs              # 会话唯一数据源；发给模型的消息由它投影得出
  sink.rs                 # EventSink trait + ThrottledSink（33ms 帧聚合）
  events.rs               # AgentEvent —— 前后端协议真源
  config.rs  error.rs  tools.rs

crates/agent-host/        # 宿主共享层：与 GUI 无关，桌面版与终端版共用
  tools/                  # 工具实现（bash / edit / read / write / grep / glob / diff）
  persist.rs              # 按工作区归档的多会话落盘
  context_files.rs        # 从工作区向上收集 AGENTS.md / CLAUDE.md 注入系统提示词
  secret.rs               # 系统凭据管理器（keyring，三平台）
  lib.rs                  # 系统提示词组装

crates/agent-tui/         # 终端版（参考 openai/codex 的 TUI 架构，同进程简化版）
  main.rs                 # 入口：参数解析、终端初始化、日志
  app.rs                  # 主循环（select! 三路事件源）+ 事件→展示模型 + 渲染
  markdown.rs             # 流式增量渲染（stable/tail 两区域模型的简化版）
  terminal.rs             # crossterm 生命周期（raw mode / 备屏 / 键盘增强）
  sink.rs                 # EventSink → mpsc channel
  config.rs               # TOML 配置加载 + 凭据解析

src-tauri/src/            # Tauri 装配层：只做 command/channel 接线，无业务逻辑
  channel_sink.rs         # 把 core 的 EventSink 接到 Tauri Channel
  state.rs                # 会话历史、配置、取消令牌
  commands/               # Tauri command 薄层
src-tauri/tests/e2e.rs    # 端到端测试：真协议 + 真工具 + 真循环

scripts/fake-llm/         # 假 LLM 服务端（零依赖 node:http）
  server.mjs
  fixtures/*.json         # 场景数据
  sandbox/                # 工具操作的工作区数据

docs/manual-gui-checklist.md   # GUI 人工测试清单

src/                      # React 前端（Tauri 版）
  lib/events.ts           # 与 core 的 events.rs 手工对齐
  lib/ipc.ts              # invoke 封装
  store/                  # zustand：session（含 rAF 缓冲）、config、workspace
  test/setup.ts           # 测试环境的 rAF 接管
  components/  pages/  styles/
```

数据流（Tauri 版）：`Composer` → `send_message` command → `turn::run` → `ThrottledSink` →
`ChannelSink` → Tauri Channel → `session.ts` 的 rAF 缓冲 → React。

数据流（TUI 版）：`App::send_input` → spawn task 跑 `turn::run` → `ChannelSink` →
mpsc → 主循环 `select!` → 事件应用到展示模型 → ratatui 渲染。
`turn::run` 独占 `&mut Session` 一整轮，期间 UI 从事件流维护展示条目，
轮次结束 task 把最终 Session 送回、重建 —— 与 Tauri 版前端从事件流
维护 React 状态同构。

### 不可动摇的架构约束

改动前先读懂这三条，它们的代价都在后期才显现：

1. **给 LLM 的文本与给 UI 的结构必须分离**。工具产出 `{ llm_text, ui }` 两份，
   UI 要彩色 diff 结构体，LLM 只要一句摘要。混在一起会让「好看」和「省 token」互相拉扯。
2. **流式期间前端不解析 Markdown**。`RichText` 只识别代码块。完整 Markdown 只能在
   轮次结束后对定稿文本做一次。这是同类应用掉帧的头号原因。
3. **Windows 进程树要用 Job Object 回收**。`child.kill()` 只杀 shell，不杀它派生的
   子进程。Bash 工具用 `command-group` 落地这一点，别改回裸 `Command`。

### 分层约定

- **core 不依赖 GUI**：`agent-core` 里出现 `tauri::` 就是错的。事件出口走 `EventSink` trait。
- **工具实现留在宿主层**：权限边界与 UI 呈现和宿主强相关。现在有两个宿主
  （`src-tauri` 与 `agent-tui`），共享实现放 `agent-host`，宿主各自做装配。
- **事件协议双向手工同步**：改 `events.rs` 必须同步改 `src/lib/events.ts`。
  TUI 直接消费 Rust 侧事件，不经过序列化，无同步负担。
  serde 的 `rename_all` 只作用于变体名，字段保持 snake_case。
- **响应侧字段一律 `Option`**：兼容网关（vLLM/DeepSeek/各类中转）缺字段是常态，
  不该让整条流解析失败。

---

## Code Style

两侧都由工具强制，不靠人记：Rust 用 rustfmt 默认配置，TS 用 Prettier。
**仓库刻意不放 `rustfmt.toml`** —— Rust 风格指南推荐使用格式化工具的默认设置，
社区一致性本身就是收益，别为了少几行换行去调 `struct_lit_width`。

### Rust

遵循 <https://doc.rust-lang.org/style-guide/>。4 空格缩进，`max_width = 100`，
块缩进而非视觉缩进，多行列表带尾随逗号。这些交给 `cargo fmt`，以下是它管不到的：

**公开 API 必须有 `///` 中文文档，讲清楚「为什么」而不是复述签名：**

```rust
// 好：解释了选型代价
/// 按优先级探测可用的 shell。
///
/// Windows 上优先 PowerShell —— 它一定存在，且能处理绝大多数命令。
/// Git Bash 虽然更贴近模型的习惯，但未必装了。
fn detect() -> Option<Self> { ... }

// 差：只是把函数名念了一遍
/// 探测 shell。
fn detect() -> Option<Self> { ... }
```

**注释用 `//`，不用 `/* */`；单行块注释的两侧各留一个空格：**

```rust
// 好
// 多收一些用于判断是否真的超限，但不至于无限增长
if hits.len() > MAX_RESULTS * 4 { ... }

// 差
/* 多收一些用于判断是否真的超限 */
```

**lint 抑制必须就地写明理由，不要全局关：**

```rust
/// 可用的 shell。
// PowerShell 是产品的正式名称，改名迁就 lint 只会让人对不上号
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell { Pwsh, PowerShell, Bash }
```

**MSRV 是 `rust-version = "1.82"`**（`Option::is_none_or` 起于该版本）。
用更新的标准库 API 时，clippy 的 `incompatible_msrv` 会拦下来 —— 要么换写法，
要么连同 `Cargo.toml` 里的 MSRV 一起抬，并在注释里写明是哪个 API 逼的。

### TypeScript

遵循 <https://google.github.io/styleguide/tsguide.html>。
Prettier 配置见 `.prettierrc.json`：`printWidth: 100`（与 Rust 侧 `max_width` 对齐）、
单引号、必写分号、尾随逗号全开。

**字符串用单引号；语句必须以分号结尾，不依赖 ASI：**

```ts
const KEY = 'llm';        // 好
const KEY = "llm"         // 差：双引号 + 缺分号
```

**对象类型用 `interface`，不用 type 字面量别名；联合/映射类型才用 `type`：**

```ts
// 好
export interface DiffSegment {
  text: string;
  emphasis: boolean;
}
export type ToolStatus = 'pending' | 'running' | 'ok' | 'error';
type StoredConfig = Omit<LlmConfig, 'api_key'>;

// 差：对象结构不该用 type 别名
type DiffSegment = { text: string; emphasis: boolean };
```

**禁止 `any`。表达「不知道是什么」用 `unknown`，再靠类型守卫收窄：**

```ts
// 好：unknown 逼调用方先判别再用
export function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) {
    return String((e as AppError).message);
  }
  return String(e);
}

// 差：any 让后续所有属性访问都不受检
function errorMessage(e: any): string {
  return e.message;
}
```

**标识符不得用 `_` 作前缀或后缀**，包括「表示这个变量没用到」的场合。
TypeScript 的 `noUnusedLocals` 本来就放过 rest 解构的兄弟绑定：

```ts
// 好
const { api_key: omittedKey, ...persistable } = config;

// 差：Google 风格禁止 _ 前缀
const { api_key: _omitted, ...persistable } = config;
```

**`/** */` 只用于给使用者看的文档，实现注释一律 `//`（多行就多写几行 `//`）：**

```ts
// 好
/** 折叠态摘要，由 Rust 侧工具生成 —— 前端不该懂工具语义。 */
preview: string;

// 流式增量的 rAF 缓冲。
// Rust 侧已按 33ms 聚合，这里是第二层兜底：即便事件密集到来，也最多
// 每帧提交一次 state，不会出现一帧内多次 React 重渲染。
let pendingText = '';

// 差：多行实现注释用了块注释
/* ------------------------------
   流式增量的 rAF 缓冲。
   ------------------------------ */
```

**平凡类型不加注解，复杂表达式才加：**

```ts
const model = '';                          // 好：类型显而易见
const snapshots: Segment[][] = [];         // 好：空数组不加会推成 never[]
const loaded: boolean = true;              // 差：boolean 没有增加任何信息
```

**其余硬性项**：命名按 `UpperCamelCase`（类型/组件）、`lowerCamelCase`（变量/函数）、
`CONSTANT_CASE`（模块级常量）；数组写 `T[]` 不写 `Array<T>`；相等一律 `===`；
不用 `var`、`const enum`、`debugger`、`eval`；不用 `new String/Boolean/Number`。

### 样式（Tailwind v4）

**引用 CSS 变量用 v4 的 `x-(--var)` 简写，不写 `x-[var(--var)]`。**
两者编译出的声明完全相同，但长形式会让工具链对每一处报
`can be written as`，几十条噪音会盖住真正的问题。
由 `src/test/styles.test.ts` 把关，`pnpm test` 会拦下来。

```tsx
// 好
<div className="bg-(--bg-elevated) border-(--border) text-(--fg-muted)" />
<div className="bg-(--color-danger)/10 border-(--color-danger)/30" />

非变量的任意值仍用方括号，这不在约束范围内：`text-[13px]`、`rounded-[2px]`、
`leading-[1.55]`。

**只引用 `globals.css` 里的语义变量**（`--bg`、`--fg-muted`、`--border` 等），
不直接引用色阶 `--color-base-*` —— 语义变量在深浅色主题下会各自重绑定，
色阶不会。唯一的合理例外是叠在强调色上的文字需要固定明度
（`bg-(--color-accent) text-(--color-base-950)`）。

### 跨语言的共同约定

- 注释与文档字符串**一律中文**，写「为什么这么做」和「不这么做会怎样」。
- 路径别名 `@/` 指向 `src/`。

---

## Git Workflow

### 分支

- `main` 是主干，PR 默认基于它。
- 当前工作分支 `new-agent`。日常改动开新分支，不直接往 `main` 推。

### 提交信息

格式：`<类型>: <中文描述>`

| 类型 | 用于 |
| --- | --- |
| `feat:` | 新功能 |
| `fix:` | 修复缺陷 |
| `refactor:` | 不改变外部行为的重构 |
| `docs:` | 只动文档 |
| `test:` | 只动测试 |
| `chore:` | 构建、依赖、工具链 |

描述用中文祈使句，说清楚**改了什么**，必要时在正文补**为什么**：

```git
fix: bash 工具用 Job Object 回收进程树

child.kill() 只杀 shell 本身，派生的子进程会变成孤儿继续占用工作区文件，
导致后续 edit 工具报「文件被占用」。
```

### 提交前

1. `pnpm check` 与 `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace` 全绿。
2. 格式化产生的改动与逻辑改动**分开提交** —— 混在一起 review 就废了。
3. 一次提交只做一件事。改事件协议时 `events.rs` 和 `events.ts` 必须在同一个提交里。

### 不要做的事

- 不 `--no-verify` 跳过钩子；钩子失败就去修根因。
- 不 `git push --force` 到共享分支。
- 优先新建提交，而非 `--amend` 已推送的提交。
- 除非明确要求，不代替用户 commit / push。

---

## Boundaries

### 绝不提交

- `config.json` —— 含 API 密钥，已在 `.gitignore` 里。密钥的正路是系统凭据管理器
  （`src-tauri/src/secret.rs`），不落明文。前端那份 `settings.json` 由 Tauri store
  插件写在应用数据目录，不在仓库内，但同样不许把密钥写进去。
- `scripts/fake-llm/config.json` —— fake-llm 连真实 LLM 用的密钥。
  模板 `config.template.json` 要提交，真配置不许。
- `target/`、`node_modules/`、`dist/`、`src-tauri/gen/`。

### .gitignore 的规则一律加根锚点

写 `/target/` 而不是 `target/`，写全路径而不是裸目录名。

上一版沿用了 Python 模板，其中一条裸 `lib/` 把 `src/lib/` 整个吞掉了 ——
前后端协议真源 `events.ts` 就在里面，`git add` 会**静默失败**，直到有人发现
仓库里少了半个模块。裸目录名在任意层级都匹配，代价远大于省下的那个斜杠。

加新规则后跑一遍确认没误伤：

```bash
git ls-files --others --exclude-standard   # 未跟踪且未忽略的，应当只有你新加的源码
git status --ignored --short               # 被忽略的，应当只有构建产物和密钥
```

### 改动边界

- **只碰任务必需的文件**。顺手「改进」相邻代码、重排 import、调整无关格式，
  都会让 review 失焦。
- **发现无关死代码就指出来，别顺手删**。
- **自己制造的孤儿要清掉**：改动导致失效的 import、变量、函数一并删除。
- **`scripts/fake-llm/sandbox/` 是测试数据，不是示例代码**。`tool-edit` 场景会真的
  改里面的文件 —— 手动测完用 `git checkout scripts/fake-llm/sandbox` 还原。
  自动化测试跑在临时副本上，不会污染它。
- `.venv/` 是 Python 时代的遗留，可以删，但别顺手删别的。

### 需要先确认的操作

- 装新依赖（Rust crate 或 npm 包）—— 每个依赖都是长期负担。
- 改 `Cargo.toml` 的 MSRV、`tsconfig.json` 的严格性开关、`.prettierrc.json`。
- 引入新的架构分层，或让 `agent-core` 依赖任何 GUI 相关的东西。
- 任何会重写大量文件的批量操作（换格式化配置、全局重命名）。

### 环境坑（本机已踩过）

- **端口 1420/1421 不可用**：本机 Windows 保留端口段为 1410-1509，Tauri 默认端口
  正好落在里面。已改用 5173/5174。
  查保留段：`netsh interface ipv4 show excludedportrange protocol=tcp`
- **`localhost` 解析到 IPv6**：Vite 和 `devUrl` 都显式写 `127.0.0.1`。
- **winget 源不可达**：装工具链走直接下载或镜像。
- **rustfmt / clippy 不在默认工具链里**：`rustup component add rustfmt clippy`。
- **`tsconfig.json` 里没有 `baseUrl`，别加回来**。TS 7 移除了这个选项，IDE 会报
  `Option 'baseUrl' has been removed`。`paths` 自 TS 4.1 起在没有 `baseUrl` 时
  相对 `tsconfig.json` 所在目录解析，`"@/*": ["./src/*"]` 单独就能工作。
  也**不要**按 IDE 提示补 `"*": ["./*"]` —— 那是给真正依赖 `baseUrl` 做裸路径解析的
  项目用的通用迁移，本仓库所有裸导入都是真实包名，补上反而会让拼错的导入
  意外解析到项目文件。
- **`reqwest` 走 rustls 而非 schannel**：Windows 的 schannel 对自建 LLM 网关的
  证书链很挑剔，`default-features = false` 是故意的，别为了省编译时间改回去。
