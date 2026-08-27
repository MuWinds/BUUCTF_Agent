# BUUCTF_Agent 开放式 Agent 工作流重构设计

## 概述

将 BUUCTF_Agent 从当前的强编排工作流（固定 think→execute→analyze 循环）重构为完全自主的 LLM Agent 模式。LLM 自行决定何时调用工具、调用几次、何时停止，系统仅提供工具和上下文管理。

## 动机

当前架构的核心问题：
- `SolveAgent` 硬编码 `while True` 循环，每步强制 `tool_choice="required"`
- `Analyzer` 在每步后强制运行，要求 LLM 输出结构化 JSON
- 工具调用顺序执行，即使 LLM 返回多个 tool_calls
- `Memory` 类的压缩逻辑与步数阈值绑定，频繁触发额外 LLM 调用
- LLM 没有自主决策权——不能选择不调工具、不能跳过分析、不能一次做多步推理

## 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 开放程度 | 完全自主 Agent | 让 LLM 发挥最大能力 |
| 工具执行 | 并行执行 | 提高效率，减少等待 |
| 停止机制 | LLM 自行停止 | 简单直接，无额外复杂度 |
| 提示组织 | 单一系统提示 | 减少模板切换开销 |
| 记忆管理 | 原生对话上下文 | 去掉自定义压缩，让 LLM 自行判断信息重要性 |
| Analyzer | 完全删除 | 分析能力内化到 LLM 思考过程 |
| 架构方案 | 自建最小循环 | 零额外依赖，完全可控 |

## 架构设计

### Skills 系统

参考 OpenCode 的 skill 设计，为 agent 提供可扩展的专业技能加载机制。

**核心思路**：每个 skill 是一个 Markdown 文件（`SKILL.md`），包含 YAML frontmatter 和专业指令内容。LLM 通过调用 `load_skill` 工具按需将技能知识加载到对话上下文中。

#### Skill 文件格式

```markdown
---
name: web-exploitation
description: Web 安全漏洞利用技术，涵盖 SQL 注入、XSS、文件包含、命令注入等常见 Web 攻击手法
---

# Web 漏洞利用指南

## SQL 注入
...

## XSS
...
```

- `name`：技能名称，小写字母+数字+连字符，1-64 字符
- `description`：技能描述，1-1024 字符，用于 LLM 判断是否需要加载
- Markdown 正文：专业指令内容，加载后作为 LLM 的上下文知识

#### 发现目录

按优先级扫描以下目录中的 `*/SKILL.md`：

1. 项目本地：`.buuctf_agent/skills/*/SKILL.md`
2. 全局：`~/.buuctf_agent/skills/*/SKILL.md`
3. 配置自定义路径：`config.json` 中 `skills.paths` 数组

同名 skill 以先发现的为准。

#### load_skill 工具

作为内置工具注册到工具列表中，与 BashShell 并列：

- 工具名：`load_skill`
- 参数：`name`（string）—— 要加载的 skill 名称
- 行为：查找对应 skill，将其 Markdown 内容作为工具结果返回给 LLM
- LLM 在系统提示中看到所有可用 skill 的名称和描述，自行决定何时调用

#### 系统提示集成

在系统提示模板中追加可用 skill 列表：

```
## 可用技能
你可以通过 load_skill 工具加载以下专业技能：
- web-exploitation: Web 安全漏洞利用技术
- crypto-analysis: 密码学分析与破解方法
- binary-reverse: 二进制逆向分析技术
...
当你遇到需要专业知识的题目时，先加载相关技能再行动。
```

#### 配置

`config_template.json` 新增：

```json
{
  "skills": {
    "paths": []
  }
}
```

### 核心 Agent 循环

`agent/agent_core.py` 实现极简消息循环：

```python
def run(question: str, tools: list, system_prompt: str, llm: LLMRequest,
        on_message=None, checkpoint_mgr=None) -> str:
    messages = [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": question}
    ]
    tool_defs = [t.function_config for t in tools]

    while True:
        response = llm.client.chat.completions.create(
            model=llm.model,
            messages=messages,
            tools=tool_defs,
            tool_choice="auto"  # LLM 自主决定是否调工具
        )
        msg = response.choices[0].message
        messages.append(msg)

        # 回调：用于 UI 显示 assistant 消息
        if on_message:
            on_message(msg)

        if not msg.tool_calls:
            break  # LLM 选择停止

        # 并行执行所有 tool_calls，每个结果作为独立的 tool 消息
        tool_result_msgs = parallel_execute_tools(tools, msg.tool_calls)
        messages.extend(tool_result_msgs)  # 每个是 {"role": "tool", "tool_call_id": ..., "content": ...}

        # 回调：用于 UI 显示工具执行结果
        if on_message:
            on_message(tool_results)

        # 检查点保存
        if checkpoint_mgr:
            checkpoint_mgr.save(messages)

        # 上下文窗口管理
        messages = trim_context(messages, max_tokens)

    return extract_result(messages)
```

关键设计点：
- `tool_choice="auto"`：LLM 可以选择输出文本而非调工具
- 无 Analyzer、无 Memory 压缩、无步计数
- 并行执行用 `concurrent.futures.ThreadPoolExecutor`
- 上下文截断保留系统提示 + 最近消息

### 并行工具执行

```python
def parallel_execute_tools(tools, tool_calls) -> list[dict]:
    """并行执行工具调用，返回 OpenAI API 格式的 tool result 消息列表。"""
    with ThreadPoolExecutor(max_workers=len(tool_calls)) as executor:
        futures = {}
        for tc in tool_calls:
            tool = find_tool(tools, tc.function.name)
            args = json.loads(tc.function.arguments)
            futures[tc.id] = executor.submit(tool.execute, **args)

        results = []
        for tc in tool_calls:
            result = futures[tc.id].result(timeout=60)
            results.append({
                "tool_call_id": tc.id,
                "role": "tool",
                "content": format_output(result, max_length=config.max_tool_output)
            })
        return results
```

### 系统提示

单一 Jinja2 模板 `prompt/system_prompt.yaml`，注入 `{question}` 和 `{tools_description}`：

**角色定义**：CTF 解题专家，具备 Web/Crypto/Pwn/Reverse/Misc 全栈能力

**工具使用指引**：
- 可用工具列表及使用场景
- 鼓励一次返回多个 tool_calls 以并行探索
- shell 命令的最佳实践
- 可用 skill 列表（名称 + 描述），LLM 可通过 load_skill 工具按需加载专业技能

**解题策略**：
- 先理解题目类型和附件
- 系统性探索：信息收集 → 分析 → 利用 → 提取 flag
- 遇到死胡同时反思并换思路

**输出约定**：
- 找到 flag 后输出 `FLAG_FOUND: flag{xxx}`
- 确认无法解决时输出 `UNABLE_TO_SOLVE: <原因>`

**停止条件**：找到 flag 或确认无法解决

### 上下文管理

- 删除 `Memory` 类，`messages` 列表本身就是记忆
- 当消息总 token 数接近 `context_window` 时，截断最早的消息
- 保留策略：系统提示（始终保留） + 最近 N 轮对话
- `max_tool_output` 配置项控制单个工具输出的最大字符数，超过则截断

### Checkpoint

- 序列化整个 `messages` 列表 + 元数据（问题文本、创建时间）
- 文件名格式：`ckpt_{timestamp}.json`
- 恢复时反序列化 messages 继续循环

## 文件变更

### 新增文件

| 文件 | 说明 |
|------|------|
| `agent/agent_core.py` | 核心 agent 循环，~80 行 |
| `prompt/system_prompt.yaml` | 单一系统提示模板 |
| `agent/skill.py` | Skill 发现、加载、注册模块 |
| `ctf_tool/load_skill.py` | load_skill 工具实现 |
| `.buuctf_agent/skills/` | 项目本地 skill 目录 |

### 删除文件

| 文件 | 替代方案 |
|------|----------|
| `agent/solve_agent.py` | 被 `agent_core.py` 取代 |
| `agent/workflow.py` | 逻辑上移至 `workflow_runner.py` |
| `agent/analyzer.py` | 完全删除，LLM 自行分析 |
| `agent/memory.py` | 完全删除，用原生对话上下文 |
| `prompt.yaml` | 被 `prompt/system_prompt.yaml` 取代 |

### 修改文件

| 文件 | 变更 |
|------|------|
| `utils/tools.py` | `execute_tools()` 改为并行执行，删除 `output_summary()` |
| `utils/text.py` | 删除 `fix_json_with_llm()`（不再需要结构化 JSON） |
| `cli/adapters/workflow_runner.py` | 简化为：加载配置 → 构建提示 → 调用 `agent_core.run()` |
| `cli/commands/solve.py` | 去掉 auto/manual 模式选择 |
| `config_template.json` | 增加 `max_tool_output` 配置项 |

### 保留不变

| 模块 | 说明 |
|------|------|
| `ctf_tool/bash_shell.py` | Bash 工具保留 |
| `ctf_tool/mcp_adapter.py` | MCP 适配器保留 |
| `ctf_platform/` | 平台层完全保留 |
| `utils/llm_request.py` | 保留，agent_core 直接调用 |
| `cli/ui/interface.py` | 保留，用于流式输出 |
| `config.py` | 保留 |

## 交互流程

重构后的用户交互流程：

1. 用户运行 `python main.py`
2. CLI 加载配置，发现并注册可用 skills
3. 构建系统提示（注入题目、工具描述、可用 skill 列表）
4. 调用 `agent_core.run()`，进入自主循环
5. LLM 判断题目类型，按需调用 `load_skill` 加载专业技能
6. LLM 自主探索解题，期间用户无需介入
7. LLM 输出 `FLAG_FOUND: flag{xxx}` 或 `UNABLE_TO_SOLVE: ...`
8. 系统提取结果，调用 FlagSubmitter（如有），结束

## 兼容性

- 保留 `--resume` 功能（通过 checkpoint 恢复 messages）
- 保留 `--plain` 模式（无颜色输出）
- 保留 `--show-think` 模式（显示 LLM 的思考过程）
- 保留 MCP 工具集成
- 保留 QuestionInputer / FlagSubmitter 插件体系
