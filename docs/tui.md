# 终端版（agent-tui）

参考 openai/codex 的 TUI 架构做的同进程简化版。核心链路：

```
App::send_input → spawn task 跑 turn::run → ChannelSink → mpsc
  → 主循环 select!（终端事件 / agent 事件 / 动画 tick）→ 展示模型 → ratatui 渲染
```

`turn::run` 独占 `&mut Session` 一整轮，期间 UI 从 `AgentEvent` 增量维护自己的
展示条目（`UiEntry`）；轮次结束 task 把最终 Session 送回、重建展示模型。
与 Tauri 版前端从事件流维护 React 状态同构。

## 运行

```bash
# 先起假 LLM（可选，没真 key 时用）
pnpm fake-llm

# 真终端里运行
cargo run -p agent-tui --workspace <项目目录>
```

配置（TOML，缺省路径为平台配置目录下的 `coding-agent/config.toml`）：

```toml
base_url = "http://127.0.0.1:8787/v1"   # 指向 fake-llm 或真实网关
model = "basic-chat"                     # fake-llm 场景 id；真实模型填模型名
context_limit = 128000
compact_threshold = 0.7
max_retries = -1                          # 无限重试（见下方说明）
```

`max_retries` 的取值：

| 写法 | 含义 |
| --- | --- |
| `max_retries = -1` | **无限重试**，直到成功或用户取消 —— 应对供应商不稳定 |
| `max_retries = 0` | 失败即报错，不重试 |
| `max_retries = 3` | 最多重试 3 次（总尝试 4 次） |
| 省略该字段 | 默认 2 次 |

> **注意**：TOML 没有 `null`，而 core 的语义里 `None` 才表示无限重试。
> 省略字段会被 serde 的容器级默认值补成 `Some(2)`，**不是**无限 ——
> 所以无限重试必须显式写 `-1`。Tauri 前端不受此限（JSON 直接传 `null`）。

api_key 解析顺序：环境变量 `CODING_AGENT_API_KEY` → 系统凭据管理器（keyring）
→ 配置文件明文（不推荐，配置文件会被同步工具带走）。

显式指定配置：`cargo run -p agent-tui -- --config /path/to/config.toml`。
配置文件不存在时直接报错，不静默退回默认 —— 拼错路径不该让用户误以为
自己的配置生效了。

## 终端渲染架构（主屏原生流式终端）

参考 OpenAI Codex CLI 与 Anthropic Claude Code 的现代化原生命令行体验：

- **主屏原生缓冲区（Main Screen Buffer）**：
  - 不劫持终端全屏（无需 `EnterAlternateScreen`），不开启鼠标捕获（无需 `EnableMouseCapture`），彻底移除虚假 ASCII/Unicode 滚动条（`█`、`│`）。
- **原生鼠标左键划选与复制**：
  - 直接鼠标左键拖拽选中文本、双击选词、三击选行，Ctrl+C 或右键直接复制，没有任何干扰。
- **原生平滑滚轮浏览**：
  - 依赖终端仿真器（Windows Terminal、iTerm2、Alacritty、WezTerm 等）原生的回滚缓冲区（Scrollback Buffer），滚轮上下滚动顺畅丝滑，退出应用后历史记录依然保留在终端中。
- **底部自适应 Composer 与实时状态**：
  - 提示符与斜杠命令补全浮层在 Raw Mode 下精确渲染在底部，输入回车后立即以 `\r\n` 锁定进入终端原生历史，永不出现重影打架。
  - 忙碌时使用单行 in-place spinner（`\r\x1b[2K`）实时反馈当前思考状态与工具执行进度（如 `$ cargo test`），轮次收尾时原地清理并输出结构化结果。

## 按键与快捷键

| 键 | 行为 |
| --- | --- |
| `Enter` | 常规模式下发送消息（若有补全菜单且光标处于命令名后则自动填充）；**多行模式下直接换行** |
| `Ctrl+T` | **切换多行输入模式**（开启后底部显示提示，`Enter` 换行，`Ctrl+S`/`Ctrl+D` 发送） |
| `Ctrl+G` | **调起外部编辑器**（Windows 记事本或 `$EDITOR`）编写大段多行文本并自动回填输入框 |
| `Ctrl+S` / `Ctrl+D` | 发送当前输入（特别适用于多行模式下直接提交） |
| `Shift+Enter` / `Alt+Enter` / `Ctrl+Enter` / `Ctrl+J` | 插入换行（常规模式下若终端支持修饰键传递，亦可换行） |
| 行尾 `\` + `Enter` | 行尾反斜杠续行（自动消除 `\` 并换行，任何终端 100% 兼容） |
| `Esc` | 忙碌时取消轮次；空闲时清空当前输入 |
| `Ctrl+C` | 忙碌时取消当前轮次；空闲时：草稿非空先清空草稿，输入为空时提示连续按两次退出 |
| `Tab` / `BackTab` | 补全选中的斜杠命令，或在补全候选菜单中正反向轮转 |
| `↑` / `↓` | 补全菜单弹出时选择候选项；多行输入时行间垂直移动；首行/末行时切换历史输入草稿 |
| `←` / `→` / `Home` / `End` | 输入光标字符级精确移动 |
| `Ctrl+A` / `Ctrl+E` | 快速跳至输入首行首 / 末行尾 |
| `Ctrl+U` / `Ctrl+K` | 清空光标前 / 光标后文本 |
| `Ctrl+W` / `Alt+Backspace` | 快速向前删除一个单词 |
| `Ctrl+O` | 切换工具执行细节（Diff 审查、命令输出）与思考过程的展开 / 精简 |
| `鼠标左键拖选` | **原生直接拖选**，右键或快捷键直接复制 |
| `鼠标滚轮` | **原生平滑滚动** 查看终端完整历史回滚缓冲 |
| 粘贴（`Ctrl+V` 或 `Shift+Insert`） | 经 bracketed-paste 插入多行文本内容（自动归一化 `\r\n`） |

## 斜杠命令

输入 `/` 即可唤起自动补全菜单：
- `/multiline`：切换多行输入模式（等同快捷键 `Ctrl+T`）
- `/editor`：在外部文本编辑器中编辑提示词（等同快捷键 `Ctrl+G`）
- `/clear`：清空当前会话，开启全新上下文
- `/compact`：手动压缩长对话历史，折叠早期消息
- `/model [名称]`：查看或切换当前模型
- `/diff`：查看工作区当前未提交的代码变更（git diff）
- `/sessions`：查看当前工作区的所有历史会话
- `/new`：保存当前会话并开启全新会话
- `/resume <id>`：切换并恢复指定历史会话
- `/detail`：切换工具输出详细展开/折叠
- `/exit` / `/quit`：退出终端应用


## 多平台说明

- 终端层只用 crossterm，Windows / macOS / Linux 通用；键盘增强在不支持的
  终端（如旧版 Windows 控制台）上自动降级，不阻塞启动。
- Bash 工具自带 Windows 三坑适配（Job Object 进程树回收、GBK 输出解码、
  PowerShell/Git Bash 探测），Unix 走 `/bin/sh`。
- keyring 三平台特性已在 `agent-host` 配好（Windows 凭据管理器 /
  macOS 钥匙串 / Linux Secret Service）。

## 测试

- 单元测试覆盖事件→展示模型映射、工具卡片生命周期、流式 markdown 增量、
  输入编辑（CJK 安全）、spinner 轮换。
- 端到端测试（`full_turn_updates_display_model`、`cancel_turn_marks_status`）
  起独占 fake-llm 实例跑真链路，与 `src-tauri/tests/e2e.rs` 共用同一份场景数据。
