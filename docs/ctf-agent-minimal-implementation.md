# CTF Agent（BUUCTF）ReAct 工作流：最小可用实现设计思路

> 关键原则：**Agent 只负责对接与编排**；每轮循环只做一件可验证的小事；所有观测结构化入库。

---

## 1. 实现范围与成功标准

### 1.1 实现范围

1. **一次会话**：针对一道题/一次任务，维护状态（题面、证据、计划栈、产物）。
2. **ReAct 主循环**：
   - 输入：当前 `State`、可用工具列表（tool registry）、历史工具输出摘要。
   - 输出：下一步动作（tool call）或完成结论。
3. **证据库**：将每轮的输入/动作/输出/结论写入 JSONL 或 Markdown。
4. **产物归档**：脚本、payload、下载文件、日志等放入 session 目录。

### 1.3 成功标准

- 给定一题输入，Agent 能：
  - 创建 session 工作目录；
  - 调用工具完成侦察；
  - 生成结构化证据记录；
  - 输出“下一步可执行动作”或“已满足成功判据”。

---

## 2. 最小系统架构

### 2.1 模块划分

- `agent/orchestrator.py`
  - ReAct 循环驱动
  - 维护/更新 `State`
  - 调用 LLM 决策下一步
- `agent/state.py`
  - `State`、`EvidenceItem`、`PlanItem` 数据结构
- `utils/tools.py`
  - 工具注册（读取配置文件）
  - 工具调用适配（统一输出格式）
- `storage/evidence_store.py`
  - JSONL/Markdown 证据写入
  - 会话回放（可选）
- `storage/artifacts.py`
  - session 目录结构创建、文件落盘、哈希
- `cli.py`
  - `ctf-agent run --target ...`（题面在命令行交互输入）

> 只要这 5 个点闭环，就能进入“可用、可迭代”的状态。

---

## 3. 目录与数据约定

### 3.1 Session 目录

建议：`runs/<timestamp>/`

- `input/`：题面原文、目标信息
- `work/`：中间文件（HTTP 响应落盘、字典、临时日志等）
- `artifacts/`：最终脚本、payload、flag 证明材料
- `logs/`：
  - `evidence.jsonl`（每条 evidence 一行）
  - `tool_calls.jsonl`（原始工具输出，可大）
  - `summary.md`（人类可读时间线）

### 3.2 Evidence 结构
每轮至少写入一条 `EvidenceItem`：

- `ts`: ISO 时间
- `step_id`: 自增
- `goal`: 本轮小目标（可验证）
- `action`:
  - `tool_name`
  - `arguments`
- `observation`:
  - `raw_output_ref`（指向 tool_calls.jsonl 行号或文件）
  - `summary`
  - `artifacts`（文件路径、hash、关键字符串等）
- `conclusion`: 本轮结论/下一步分支理由

---

## 4. ReAct 循环：提示词与控制面

### 4.1 “Reason”的处理

文档里 Reason 可在内部进行。**对用户只输出可执行下一步**，但在证据库里保留：

- 本轮目标（success criteria）
- 选择该工具的理由（低成本信息增益）
- 失败时的备选分支（Plan B）

### 4.2 “Act”：统一 Tool Call 协议

统一让模型输出结构化 tool call，例如：

```json
{
  "tool_calls": [
    {
      "tool_name": "http_request",
      "arguments": {"method": "GET", "url": "http://target/"}
    }
  ],
  "success_criteria": "确认首页是否可访问，并获取关键响应片段（标题/指纹/跳转）"
}
```

编排层执行后把结果回填到下一轮上下文：

- `tool_name`
- `arguments`
- `summary`（强制短摘要）
- `artifacts`（路径+hash+关键字段）

### 4.3 “Observe”：把长输出降噪

工具原始输出往往很长。采取两层存储：
- 原始输出：落到 `tool_calls.jsonl`
- 进入上下文的只有：**摘要 + 关键证据**

---
## 5. 工具层：HTTP

工具层只保留一个通用的 `http_request`，用于：

- Web 题目：访问目标 URL、拉取页面/接口响应
- 远程服务：健康检查、获取 banner / 简单探测
- 保存关键响应：将响应 body 落盘到 `work/` 以便复现

> 原则不变：**Agent 只负责对接与编排**；工具输出必须统一格式并结构化写入证据库。

### 5.1 `http_request` 的最小协议

工具名：`http_request`

参数（建议最小集合）：

- `method`: `GET|POST|PUT|DELETE|HEAD`（默认 `GET`）
- `url`: 目标 URL（必填）
- `headers`: 可选（键值对）
- `params`: 可选（query 参数）
- `data`: 可选（表单/原始 body）
- `json`: 可选（JSON body，若提供则自动设置 `Content-Type: application/json`）
- `timeout_sec`: 可选（默认 10）
- `allow_redirects`: 可选（默认 true）
- `save_as`: 可选（将响应 body 落盘到 session 目录的相对路径，例如 `work/resp.html`）

返回（统一输出格式，供写入 `tool_calls.jsonl` 与 `evidence.jsonl`）：

- `status_code`
- `headers`（可截断/白名单）
- `text_preview`（响应文本前 N 字符，避免把大响应塞进上下文）
- `bytes_len`
- `artifacts`（如果 `save_as`，则记录路径 + sha256）

### 5.2 最小加载/发现/调用（不依赖 MCP）

1. **注册**：在工具注册表中硬编码/配置化注册 `http_request`（提供 name/description/args schema）。
2. **发现/检索**：MVP 直接把可用工具列表（此时只有 `http_request`）提供给模型。
3. **调用**：解析模型 `tool_calls` → 调用本地实现的 `http_request`。
4. **汇总**：对响应做短摘要（状态码、关键 header、关键片段），并把原始响应按需落盘引用。

---

## 6. 落地方式

参考 LangChain/LangGraph 的常见模式（详见官方文档索引：<https://docs.langchain.com/llms.txt>），

- 使用 Chat Model + 工具调用（tool calling / function calling）
- 你自己写 while 循环驱动 ReAct

优点：实现快、调试直观。

## 7. 最小“Judge/成功判据”设计

为避免无效探索，强制每轮给出 `success_criteria`，并在 Observe 后判断：

- `met`: 已满足（例如确认是 ELF 64-bit、找到 flag 格式字符串、定位溢出偏移）
- `not_met`: 未满足但有增量（继续）
- `stuck`: 连续 N 轮无增量（切换路线/回滚）

其中“增量”判定可以很朴素：

- 新 artifacts 数量 > 0
- 新关键字符串/地址/端点出现
- 新文件类型/协议识别成功

---

## 8. 最小可用的运行方式（CLI 交互）

提供两种模式：

1. **自动模式**：循环至满足成功判据或达到最大步数（如 20）。
2. **半自动模式**：每轮给出下一步 tool call，用户确认后执行（更安全）。

输出给用户的内容保持简洁：

- 本轮目标
- 将调用的工具与参数
- 上轮观测摘要
- 下一步建议

---

## 9. 题目输入与过程输出（命令行约定）

为保证“可跑起来 + 可回放”，MVP 使用命令行作为输入入口，并将**题目输入**与**过程输出**做最小规范化。

### 9.1 命令行入口（建议形态）

建议提供一个 CLI：

- `ctf-agent run ...`

参数集合：

- `--target <url>`：目标地址（必填，Web 靶机入口 URL）
- `--mode auto|confirm`：自动/半自动（默认 `confirm`）
- `--max-steps <n>`：最大循环步数（默认 20）

题面输入方式：

- `ctf-agent run` 启动后，从 stdin 交互式读取题面（支持多行），以**一行仅包含 `EOF`** 作为结束标记。
- 读取到的题面会被原样写入 session 的 `input/prompt.md`。

### 9.2 输入落盘

启动 session 时必须创建目录并写入输入：

- `runs/<timestamp>/input/prompt.md`

并写入第一条 `EvidenceItem`（step_id=0），作为证据链起点。

### 9.3 过程输出（stdout + logs）

MVP 过程输出分两层：

1. **stdout（给人看，短）**：
  - 当前 step_id
  - 本轮 goal（success criteria）
  - 将执行的 tool_calls（name + arguments 关键字段）
  - 上轮 observation summary（若有）
  - Judge 结果：`met | not_met | stuck`

2. **logs（给机器回放，结构化）**：
  - `logs/evidence.jsonl`：每轮至少 1 条 EvidenceItem（包含 goal/action/observation/conclusion）
  - `logs/tool_calls.jsonl`：每次工具调用一条（含原始输出或其引用、错误信息、耗时）
  - `logs/summary.md`：人类可读时间线（每 step 一段，引用 step_id 与关键 artifacts）

> 约束：stdout 只打印“摘要”，任何大输出都必须写入 `tool_calls.jsonl` 或落盘为文件后在 evidence 里引用。

### 9.4 与工具输出的衔接（以 `http_request` 为例）

- `http_request` 的响应正文默认不直接进入上下文/终端
- 仅打印：`status_code`、`bytes_len`、`text_preview`（前 N 字符）

