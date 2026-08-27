# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

BUUCTF_Agent 是一个自主 LLM CTF 解题 Agent。Agent 读取题目后进入自主循环，由 LLM 决定何时调用工具、何时停止，最终提取 FLAG。

## 常用命令

```bash
# 安装依赖（使用 .venv 虚拟环境）
pip install -r requirements.txt

# 运行（默认 solve 子命令，手动输入题目）
python main.py

# 指定题目运行
python main.py solve --question "题目内容" --no-resume --plain

# 配置检查
python main.py config check

# 列出可用工具
python main.py tools list

# 存档管理
python main.py checkpoint list
python main.py checkpoint clear

# Lint
ruff check .
ruff format .

# 类型检查（需使用 .venv 中的 Python）
python -m pyright
```

## 架构

```
main.py → cli/app.py (Typer CLI)
  → cli/commands/solve.py → cli/adapters/workflow_runner.py::run_workflow()
    → agent/skill.py::discover_skills()          # 发现 skills
    → utils/tools.py::ToolUtils.load_tools()     # 加载工具（BashShell、MCP）
    → ctf_tool/load_skill.py::LoadSkillTool       # 注册 load_skill 工具
    → prompt/system_prompt.yaml (Jinja2)          # 渲染系统提示词
    → agent/agent_core.py::run()                  # 核心自主循环
      → utils/llm_request.py::LLMRequest          # 调用 LLM（OpenAI 兼容接口）
      → utils/tools.py::ToolUtils.execute_tools() # 并行执行工具
      → agent/checkpoint.py::CheckpointManager    # 自动存档
    → ctf_platform/ FlagSubmitter                 # 提交 flag
```

**核心循环 (`agent/agent_core.py`)**：LLM 自主决定 `tool_choice="auto"`，有 tool_calls 则并行执行（ThreadPoolExecutor），无 tool_calls 则停止。结果通过 `FLAG_FOUND:` / `UNABLE_TO_SOLVE:` 标记提取。

## 关键模块

| 模块 | 职责 |
|------|------|
| `agent/agent_core.py` | 自主消息循环，上下文截断（3 char/token 估算，超限删最早 20%） |
| `agent/checkpoint.py` | 存档序列化到 `checkpoints/ckpt_{timestamp}.json` |
| `agent/skill.py` | Skill 发现：扫描 `./skills/`、`~/.buuctf_agent/skills/`、config 自定义路径 |
| `ctf_tool/base_tool.py` | 工具抽象基类，子类实现 `execute()` 和 `function_config` 属性 |
| `ctf_tool/bash_shell.py` | Bash 命令执行，Windows 自动检测 Git Bash |
| `ctf_tool/mcp_adapter.py` | MCP 服务器适配（stdio/HTTP），将远程工具桥接到 BaseTool 接口 |
| `ctf_platform/registry.py` | 注册表模式：`@register_inputer` / `@register_submitter` 装饰器 |
| `utils/tools.py` | 动态发现 `ctf_tool/` 下的工具类，加载 MCP 服务器配置 |
| `config.py` | 加载 `config.json`，支持 model alias（"analyzer"/"pre_processor" → "solve_agent"） |

## 配置

从 `config.json` 加载（从 `config_template.json` 复制）。关键字段：`llm`（模型/key/base_url）、`tool_config.bash_shell`、`mcp_server`、`skills.paths`、`platform`（inputer/submitter）。

## Skill 系统

每个 skill 是 `skills/<name>/SKILL.md`，含 YAML frontmatter（`name`、`description`）+ Markdown 正文。LLM 通过 `load_skill` 工具按名加载 skill 内容到上下文。

## 代码规范

- Python 3.12，缩进 4 空格，编码 UTF-8
- 类名 PascalCase，函数/变量 snake_case，私有方法 `_` 前缀
- 注释和文档字符串使用中文，文档字符串 Google 风格
- 日志用标准 `logging` 模块
- 导入顺序：标准库 → 第三方 → 本地（ruff `I` 规则自动排序）
- ruff 规则：`E`, `F`, `W`, `D`, `I`，忽略 `D100`, `D104`, `D415`

## Git 提交

格式：`<类型>: <中文描述>`。类型：`feat:` / `fix:` / `Update:` / `refactor:` / `fix:`。

## 边界

- 不要修改 `config.json`（含 API 密钥）
- 不要删除 `checkpoints/`、`logs/`、`attachments/` 中的运行时数据
- 不要在代码中硬编码 API 密钥
