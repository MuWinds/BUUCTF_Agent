# 开放式 Agent 工作流重构 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 BUUCTF_Agent 从固定步循环编排重构为完全自主的 LLM Agent，LLM 自行决定工具调用和停止时机。

**Architecture:** 删除 SolveAgent/Workflow/Analyzer/Memory，新建 `agent/agent_core.py` 实现最小消息循环（~80 行）。LLM 通过 OpenAI 原生 tool calling 自主探索，工具并行执行，对话上下文即记忆。系统提示合并为单一 Jinja2 模板。

**Tech Stack:** Python 3.8+, OpenAI SDK, Jinja2, concurrent.futures, Rich, prompt_toolkit

---

## File Structure

### 新增文件
| 文件 | 职责 |
|------|------|
| `agent/agent_core.py` | 核心 agent 循环：消息轮转 + 并行工具执行 + 上下文截断 |
| `prompt/system_prompt.yaml` | 单一系统提示模板（角色 + 工具指引 + 解题策略 + 输出约定） |
| `agent/skill.py` | Skill 发现与加载模块 |
| `ctf_tool/load_skill.py` | load_skill 工具实现 |
| `.buuctf_agent/skills/web-exploitation/SKILL.md` | 示例 skill：Web 漏洞利用 |

### 修改文件
| 文件 | 变更 |
|------|------|
| `utils/llm_request.py` | 新增 `chat_completion(messages, tools, tool_choice)` 方法 |
| `utils/tools.py` | `execute_tools()` 改为并行执行，删除 `output_summary()` |
| `utils/text.py` | 删除 `fix_json_with_llm()` |
| `agent/checkpoint.py` | 简化为序列化/反序列化 messages 列表 |
| `cli/adapters/workflow_runner.py` | 重写 `run_workflow()` 调用 `agent_core.run()` |
| `cli/commands/solve.py` | 去掉 auto/manual 模式选择 |
| `utils/user_interface.py` | 删除 `select_mode`, `manual_approval`, `manual_approval_step` |
| `cli/ui/interface.py` | 同步删除对应实现 |
| `config_template.json` | 增加 `max_tool_output`，删除 `max_history_steps`, `compression_threshold` |

### 删除文件
| 文件 | 替代 |
|------|------|
| `agent/solve_agent.py` | `agent/agent_core.py` |
| `agent/workflow.py` | 逻辑上移至 `workflow_runner.py` |
| `agent/analyzer.py` | 完全删除 |
| `agent/memory.py` | 完全删除 |
| `prompt.yaml` | `prompt/system_prompt.yaml` |

---

## Task 1: 创建系统提示模板

**Files:**
- Create: `prompt/system_prompt.yaml`

- [ ] **Step 1: 创建 `prompt/` 目录并写入系统提示模板**

```bash
mkdir -p prompt
```

写入 `prompt/system_prompt.yaml`：

```yaml
system_prompt: |
  你是一个专业的 CTF 解题专家，具备 Web、Crypto、Pwn、Reverse、Misc 全栈安全能力。

  ## 可用工具
  你可以使用以下工具来解题：
  {% for tool in tools %}
  - {{ tool.function.name }}: {{ tool.function.description }}
  {% endfor %}

  ## 可用技能
  你可以通过 load_skill 工具加载以下专业技能：
  {% for skill in skills %}
  - {{ skill.name }}: {{ skill.description }}
  {% endfor %}
  当你遇到需要专业知识的题目时，先加载相关技能再行动。

  ## 工具使用指引
  - 你可以一次返回多个工具调用，系统会并行执行它们
  - 优先使用 shell 命令进行信息收集和探测
  - 先侦察再深入：先了解目标环境，再进行针对性操作
  - 对于网络题目，先用 curl 探测目标，再分析响应

  ## 解题策略
  1. 仔细阅读题目描述，判断题目类型
  2. 如果有附件，先分析附件内容
  3. 系统性探索：信息收集 → 分析 → 利用 → 提取 flag
  4. 遇到死胡同时，反思并尝试不同的方法
  5. 不要迷信自动化工具的结果，基于题目本身的提示和线索行动
  6. 优先考虑最直接、最简单的攻击路径

  ## 输出约定
  - 当你找到 flag 时，在回复中明确输出：FLAG_FOUND: flag{xxx}
  - 当你确认无法解决时，输出：UNABLE_TO_SOLVE: <原因>
  - 其他时候，正常输出你的思考过程和分析

  ## 停止条件
  - 找到并输出 flag 后停止
  - 确认无法解决并说明原因后停止
  - 不要在没有进展时无限重复相同操作

  ## 当前题目
  {{ question }}
```

- [ ] **Step 2: 验证模板可被 Jinja2 加载**

运行以下 Python 代码验证：

```python
import yaml
from jinja2 import Environment, BaseLoader

with open("prompt/system_prompt.yaml", "r", encoding="utf-8") as f:
    data = yaml.safe_load(f)

env = Environment(loader=BaseLoader())
template = env.from_string(data["system_prompt"])
result = template.render(
    question="测试题目",
    tools=[{"function": {"name": "test_tool", "description": "测试工具"}}],
    skills=[{"name": "web-exploitation", "description": "Web 漏洞利用"}],
)
assert "测试题目" in result
assert "test_tool" in result
assert "web-exploitation" in result
assert "load_skill" in result
print("OK: 模板加载和渲染成功")
```

- [ ] **Step 3: 提交**

```bash
git add prompt/system_prompt.yaml
git commit -m "feat: 添加单一系统提示模板"
```

---

## Task 2: 创建 Skill 发现与加载模块

**Files:**
- Create: `agent/skill.py`

参考 OpenCode 的 skill 系统设计：每个 skill 是一个 `SKILL.md` 文件（YAML frontmatter + Markdown 内容），从多个目录发现，按需加载。

- [ ] **Step 1: 写入 `agent/skill.py`**

```python
"""
@brief Skill 发现与加载模块。

@details
参考 OpenCode 的 skill 系统设计。每个 skill 是一个 SKILL.md 文件，
包含 YAML frontmatter（name, description）和 Markdown 正文内容。
支持从项目本地目录和全局目录发现 skills。
"""

import logging
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

logger = logging.getLogger(__name__)

# skill 名称校验：小写字母+数字+连字符
SKILL_NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")


@dataclass
class SkillInfo:
    """单个 skill 的元数据和内容。"""
    name: str
    description: str
    content: str
    location: str  # SKILL.md 的绝对路径


@dataclass
class SkillRegistry:
    """Skill 注册表，负责发现和管理所有可用 skills。"""
    skills: Dict[str, SkillInfo] = field(default_factory=dict)
    dirs: List[str] = field(default_factory=list)

    def get(self, name: str) -> Optional[SkillInfo]:
        """按名称获取 skill。"""
        return self.skills.get(name)

    def all(self) -> List[SkillInfo]:
        """返回所有已注册的 skills，按名称排序。"""
        return sorted(self.skills.values(), key=lambda s: s.name)

    def names(self) -> List[str]:
        """返回所有 skill 名称列表。"""
        return sorted(self.skills.keys())


def discover_skills(
    config: Optional[Dict[str, Any]] = None,
    project_dir: str = ".",
) -> SkillRegistry:
    """
    @brief 从多个目录发现并加载所有 skills。
    @param config 全局配置字典，用于读取 skills.paths。
    @param project_dir 项目根目录。
    @return SkillRegistry 实例。
    """
    registry = SkillRegistry()
    search_dirs = _get_search_dirs(config, project_dir)

    for search_dir in search_dirs:
        _scan_dir(search_dir, registry)

    logger.info("共发现 %d 个 skills: %s", len(registry.skills), registry.names())
    return registry


def _get_search_dirs(
    config: Optional[Dict[str, Any]],
    project_dir: str,
) -> List[str]:
    """构建 skill 搜索目录列表。"""
    dirs = []

    # 1. 项目本地目录
    project_local = os.path.join(project_dir, ".buuctf_agent", "skills")
    if os.path.isdir(project_local):
        dirs.append(project_local)

    # 2. 全局目录
    home = Path.home()
    global_dir = home / ".buuctf_agent" / "skills"
    if global_dir.is_dir():
        dirs.append(str(global_dir))

    # 3. 配置自定义路径
    if config:
        skills_config = config.get("skills", {})
        custom_paths = skills_config.get("paths", [])
        for p in custom_paths:
            expanded = os.path.expanduser(p)
            if not os.path.isabs(expanded):
                expanded = os.path.join(project_dir, expanded)
            if os.path.isdir(expanded):
                dirs.append(expanded)

    return dirs


def _scan_dir(base_dir: str, registry: SkillRegistry) -> None:
    """扫描目录下的所有 */SKILL.md 文件。"""
    if base_dir in registry.dirs:
        return
    registry.dirs.append(base_dir)

    for entry in os.listdir(base_dir):
        skill_dir = os.path.join(base_dir, entry)
        if not os.path.isdir(skill_dir):
            continue
        skill_md = os.path.join(skill_dir, "SKILL.md")
        if not os.path.isfile(skill_md):
            continue
        _load_skill_file(skill_md, registry)


def _load_skill_file(path: str, registry: SkillRegistry) -> None:
    """解析单个 SKILL.md 文件并注册。"""
    try:
        with open(path, "r", encoding="utf-8") as f:
            raw = f.read()
    except OSError as error:
        logger.warning("读取 skill 文件失败 %s: %s", path, error)
        return

    # 分离 YAML frontmatter 和 Markdown 正文
    parts = raw.split("---", 2)
    if len(parts) < 3:
        logger.warning("skill 文件缺少 YAML frontmatter: %s", path)
        return

    try:
        frontmatter = yaml.safe_load(parts[1])
    except yaml.YAMLError as error:
        logger.warning("skill frontmatter 解析失败 %s: %s", path, error)
        return

    if not isinstance(frontmatter, dict):
        logger.warning("skill frontmatter 不是有效字典: %s", path)
        return

    name = frontmatter.get("name", "")
    description = frontmatter.get("description", "")
    content = parts[2].strip()

    if not name or not description:
        logger.warning("skill 缺少 name 或 description: %s", path)
        return

    if not SKILL_NAME_RE.match(name):
        logger.warning("skill 名称格式无效 '%s': %s", name, path)
        return

    if name in registry.skills:
        logger.warning("skill 名称重复 '%s'，忽略: %s", name, path)
        return

    registry.skills[name] = SkillInfo(
        name=name,
        description=description,
        content=content,
        location=os.path.abspath(path),
    )
    logger.info("已加载 skill: %s (%s)", name, path)
```

- [ ] **Step 2: 创建示例 skill 目录和文件**

```bash
mkdir -p .buuctf_agent/skills/web-exploitation
```

写入 `.buuctf_agent/skills/web-exploitation/SKILL.md`：

```markdown
---
name: web-exploitation
description: Web 安全漏洞利用技术，涵盖 SQL 注入、XSS、文件包含、命令注入等常见 Web 攻击手法
---

# Web 漏洞利用指南

## 信息收集
- 使用 curl -I 探测 HTTP 头信息
- 检查 robots.txt、sitemap.xml
- 目录扫描：使用 dirb/gobuster/fuzz
- 检查常见备份文件：.bak、.swp、.git

## SQL 注入
- 判断注入点：单引号、双引号、数字型
- 联合查询注入：ORDER BY 确定列数 → UNION SELECT
- 报错注入：extractvalue()、updatexml()、floor()
- 布尔盲注：AND 1=1 / AND 1=2
- 时间盲注：sleep()、benchmark()
- 常用工具：sqlmap

## 文件包含
- 本地文件包含：../../etc/passwd
- 远程文件包含：http://evil.com/shell.txt
- PHP 伪协议：php://filter/convert.base64-encode/resource=
- 日志包含：/var/log/apache2/access.log

## 命令注入
- 命令分隔符：;、|、||、&&、`、$()
- 绕过空格：$IFS、{,}、%09
- 绕过黑名单：base64 编码、变量拼接、通配符

## 反序列化
- PHP：unserialize()、phar://
- Java：readObject()、ysoserial
- Python：pickle.loads()
```

- [ ] **Step 3: 验证 skill 发现**

```bash
python -c "
from agent.skill import discover_skills
registry = discover_skills(project_dir='.')
print(f'发现 {len(registry.skills)} 个 skills')
for s in registry.all():
    print(f'  - {s.name}: {s.description[:50]}')
assert 'web-exploitation' in registry.skills
print('OK')
"
```

- [ ] **Step 4: 提交**

```bash
git add agent/skill.py .buuctf_agent/skills/web-exploitation/SKILL.md
git commit -m "feat: 新增 Skill 发现与加载模块，附带 web-exploitation 示例 skill"
```

---

## Task 3: 创建 load_skill 工具

**Files:**
- Create: `ctf_tool/load_skill.py`

将 skill 加载能力封装为一个工具，LLM 可以通过 tool calling 按需加载 skill 内容到上下文。

- [ ] **Step 1: 写入 `ctf_tool/load_skill.py`**

```python
"""
@brief 提供 load_skill 工具，让 LLM 按需加载专业技能。
"""

from typing import Any, Dict

from agent.skill import SkillRegistry
from ctf_tool.base_tool import BaseTool


class LoadSkillTool(BaseTool):
    """@brief 按需加载 CTF 专业技能到对话上下文。"""

    def __init__(self, skill_registry: SkillRegistry) -> None:
        self._registry = skill_registry

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        """@brief 加载指定 skill 并返回其内容。
        @param tool_name 工具名（未使用）。
        @param arguments 参数字典，需包含 name。
        @return skill 内容文本，或错误信息。
        """
        del tool_name
        name = arguments.get("name", "")
        if not name:
            return "错误：未提供 skill 名称"

        skill = self._registry.get(name)
        if not skill:
            available = ", ".join(self._registry.names())
            return f"错误：未找到 skill '{name}'。可用 skills: {available}"

        return (
            f"<skill_content name=\"{skill.name}\">\n"
            f"{skill.content}\n"
            f"</skill_content>"
        )

    @property
    def function_config(self) -> Dict[str, Any]:
        """@brief 返回工具函数配置。"""
        # 构建可用 skill 列表用于描述
        skill_list = "\n".join(
            f"- {s.name}: {s.description}"
            for s in self._registry.all()
        )
        description = (
            "加载 CTF 专业技能到上下文中。当你遇到需要特定领域知识的题目时，"
            "先调用此工具加载相关技能。\n可用技能：\n" + skill_list
        )

        return {
            "type": "function",
            "function": {
                "name": "load_skill",
                "description": description,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "要加载的 skill 名称",
                        }
                    },
                    "required": ["name"],
                },
            },
        }
```

- [ ] **Step 2: 验证工具可加载**

```bash
python -c "
from agent.skill import discover_skills
from ctf_tool.load_skill import LoadSkillTool

registry = discover_skills(project_dir='.')
tool = LoadSkillTool(registry)
config = tool.function_config
assert config['function']['name'] == 'load_skill'
assert 'web-exploitation' in config['function']['description']

result = tool.execute('load_skill', {'name': 'web-exploitation'})
assert 'SQL 注入' in result
print('OK: load_skill 工具正常')
"
```

- [ ] **Step 3: 提交**

```bash
git add ctf_tool/load_skill.py
git commit -m "feat: 新增 load_skill 工具，LLM 可按需加载专业技能"
```

---

## Task 4: 为 LLMRequest 添加 chat_completion 方法

**Files:**
- Modify: `utils/llm_request.py:62-84`

当前 `text_completion()` 总是将 prompt 包装为单条 user message。agent 循环需要传入完整的消息列表。

- [ ] **Step 1: 在 LLMRequest 类中添加 `chat_completion` 方法**

在 `utils/llm_request.py` 的 `text_completion` 方法之后（第 84 行后）添加：

```python
def chat_completion(
    self,
    messages: List[Dict[str, Any]],
    tools: Optional[List[Dict[str, Any]]] = None,
    tool_choice: str = "auto",
) -> Any:
    """
    @brief 使用完整消息列表发起对话补全请求。
    @param messages OpenAI 格式的消息列表。
    @param tools 工具定义列表。
    @param tool_choice 工具选择策略。
    @return OpenAI 原始响应对象。
    """
    kwargs: Dict[str, Any] = {}
    if tools:
        kwargs["tools"] = tools
        kwargs["tool_choice"] = tool_choice

    response = self.client.chat.completions.create(
        model=self.llm_config["model"],
        messages=messages,
        **kwargs,
    )
    logger.debug("LLM Response Message: %s", response.choices[0].message.content)
    return response
```

需要在文件顶部的 import 中确认 `Optional` 已导入（当前已有 `from typing import Any, Dict, List, Union`，需加上 `Optional`）：

```python
from typing import Any, Dict, List, Optional, Union
```

- [ ] **Step 2: 提交**

```bash
git add utils/llm_request.py
git commit -m "feat: LLMRequest 新增 chat_completion 方法支持完整消息列表"
```

---

## Task 5: 修改 ToolUtils 支持并行执行

**Files:**
- Modify: `utils/tools.py:148-214` (替换 `execute_tools`)
- Modify: `utils/tools.py:216-305` (删除 `output_summary`)

- [ ] **Step 1: 替换 `execute_tools` 为并行版本**

将 `utils/tools.py` 中的 `execute_tools` 静态方法（第 148-214 行）替换为：

```python
@staticmethod
def execute_tools(
    tools: Dict[str, BaseTool],
    tool_calls: list,
    display_message: Optional[Callable[[str], None]] = None,
) -> list:
    """
    @brief 并行执行一组工具调用，返回 OpenAI 格式的 tool result 消息列表。

    @param tools 工具实例映射，key 为工具名。
    @param tool_calls OpenAI 原生 tool_call 对象列表。
    @param display_message 可选的消息显示回调。
    @return OpenAI 格式的 tool result 消息列表。
    """
    from concurrent.futures import ThreadPoolExecutor, as_completed

    def _run_one(tc):
        func_name = tc.function.name
        try:
            args = json.loads(tc.function.arguments)
        except json.JSONDecodeError:
            import json_repair
            args = json_repair.loads(tc.function.arguments)

        if display_message:
            display_message(f"  执行工具: {func_name}")

        if func_name in tools:
            try:
                result = tools[func_name].execute(func_name, args)
                if not result:
                    result = "注意！无输出内容！"
            except Exception as error:
                result = f"工具执行出错: {str(error)}"
        else:
            result = f"错误: 未找到工具 '{func_name}'"

        logger.info("工具 %s 输出:\n%s", func_name, result)
        return tc.id, result

    results = []
    with ThreadPoolExecutor(max_workers=len(tool_calls)) as executor:
        futures = {executor.submit(_run_one, tc): tc for tc in tool_calls}
        for future in as_completed(futures):
            tc_id, output = future.result(timeout=120)
            results.append({
                "tool_call_id": tc_id,
                "role": "tool",
                "content": str(output),
            })

    # 按原始 tool_calls 顺序排列结果
    tc_id_order = {tc.id: i for i, tc in enumerate(tool_calls)}
    results.sort(key=lambda r: tc_id_order.get(r["tool_call_id"], 0))
    return results
```

- [ ] **Step 2: 删除 `output_summary` 静态方法**

删除 `utils/tools.py` 中第 216-305 行的 `output_summary` 方法。

- [ ] **Step 3: 清理不再需要的 import**

`utils/tools.py` 顶部的 import 中，删除不再使用的：

```python
# 删除这两行
import json_repair
from utils.text import fix_json_with_llm
```

保留 `json`（仍被 `_run_one` 使用）。`json_repair` 在 `_run_one` 内部局部导入。

同时删除不再使用的 `yaml` 和 `jinja2` 相关 import（第 10-11 行），以及 `LLMRequest` import（第 15 行）——`output_summary` 删除后这些都不再需要。

- [ ] **Step 4: 提交**

```bash
git add utils/tools.py
git commit -m "refactor: ToolUtils 改为并行执行，删除 output_summary"
```

---

## Task 6: 简化 CheckpointManager

**Files:**
- Modify: `agent/checkpoint.py`

当前 CheckpointManager 基于 MD5 哈希文件名，存储 `problem/step_count/auto_mode/memory`。新方案存储 messages 列表。

- [ ] **Step 1: 重写 CheckpointManager**

用以下内容替换 `agent/checkpoint.py` 的完整内容：

```python
"""
@brief 解题进度存档管理模块。
"""

import json
import logging
import os
from datetime import datetime
from typing import Any, Dict, List, Optional

logger = logging.getLogger(__name__)


class CheckpointManager:
    """
    @brief 管理解题流程中的存档文件。
    """

    def __init__(self, checkpoint_dir: str = "./checkpoints") -> None:
        self.checkpoint_dir = checkpoint_dir
        os.makedirs(self.checkpoint_dir, exist_ok=True)

    def save(self, messages: List[Dict[str, Any]], problem: str = "") -> None:
        """
        @brief 保存当前对话消息列表到存档文件。
        @param messages OpenAI 格式的消息列表。
        @param problem 题目文本（用于元数据）。
        """
        # 将消息列表中的不可序列化对象转换为 dict
        serializable_msgs = []
        for msg in messages:
            if isinstance(msg, dict):
                serializable_msgs.append(msg)
            elif hasattr(msg, "model_dump"):
                serializable_msgs.append(msg.model_dump())
            else:
                serializable_msgs.append(str(msg))

        data = {
            "problem": problem,
            "messages": serializable_msgs,
            "saved_at": datetime.now().isoformat(),
        }
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        path = os.path.join(self.checkpoint_dir, f"ckpt_{timestamp}.json")
        with open(path, "w", encoding="utf-8") as file:
            json.dump(data, file, ensure_ascii=False, indent=2)
        logger.info("存档已保存: %s", path)

    def load_latest(self) -> Optional[Dict[str, Any]]:
        """
        @brief 加载最新的存档。
        @return 存档字典；若不存在则返回 None。
        """
        files = self.list_checkpoints()
        if not files:
            return None

        # 按文件名排序取最新
        files.sort(reverse=True)
        path = os.path.join(self.checkpoint_dir, files[0])
        try:
            with open(path, "r", encoding="utf-8") as file:
                return json.load(file)
        except (json.JSONDecodeError, IOError) as error:
            logger.error("读取存档失败: %s", error)
            return None

    def delete_latest(self) -> None:
        """
        @brief 删除最新的存档文件。
        """
        files = self.list_checkpoints()
        if not files:
            return
        files.sort(reverse=True)
        path = os.path.join(self.checkpoint_dir, files[0])
        if os.path.isfile(path):
            os.remove(path)
            logger.info("存档已删除: %s", path)

    def list_checkpoints(self) -> List[str]:
        """
        @brief 列出存档目录下所有存档文件名。
        @return 存档文件名列表。
        """
        if not os.path.exists(self.checkpoint_dir):
            return []
        return [
            f for f in os.listdir(self.checkpoint_dir)
            if f.startswith("ckpt_") and f.endswith(".json")
        ]
```

- [ ] **Step 2: 提交**

```bash
git add agent/checkpoint.py
git commit -m "refactor: 简化 CheckpointManager，存储 messages 列表"
```

---

## Task 7: 创建 agent_core.py

**Files:**
- Create: `agent/agent_core.py`

这是重构的核心——一个最小的 agent 消息循环。

- [ ] **Step 1: 写入 `agent/agent_core.py`**

```python
"""
@brief 自主 Agent 核心循环。
"""

import logging
import re
from typing import Any, Callable, Dict, List, Optional

logger = logging.getLogger(__name__)

# 单个工具输出的最大字符数
DEFAULT_MAX_TOOL_OUTPUT = 8192


def run(
    messages: List[Dict[str, Any]],
    tools: list,
    tool_defs: list,
    llm: Any,
    on_message: Optional[Callable] = None,
    checkpoint_mgr: Any = None,
    problem: str = "",
    max_tool_output: int = DEFAULT_MAX_TOOL_OUTPUT,
) -> str:
    """
    @brief 自主 agent 循环：LLM 自主决定工具调用和停止时机。
    @param messages 初始消息列表（含 system + user）。
    @param tools 工具实例字典。
    @param tool_defs OpenAI 格式的工具定义列表。
    @param llm LLMRequest 实例。
    @param on_message 消息回调（用于 UI 显示）。
    @param checkpoint_mgr 可选的存档管理器。
    @param problem 题目文本（用于存档元数据）。
    @param max_tool_output 单个工具输出的最大字符数。
    @return 解题结果字符串。
    """
    from utils.tools import ToolUtils

    while True:
        # 调用 LLM
        try:
            response = llm.chat_completion(
                messages=messages,
                tools=tool_defs if tool_defs else None,
                tool_choice="auto",
            )
        except Exception as error:
            logger.error("LLM 调用失败: %s", error)
            return f"LLM 调用失败: {error}"

        msg = response.choices[0].message

        # 将 assistant 消息加入历史
        messages.append(_msg_to_dict(msg))

        # 回调：显示 assistant 消息
        if on_message and msg.content:
            on_message(msg.content)

        # LLM 未调用工具 → 停止
        if not msg.tool_calls:
            break

        # 回调：显示工具调用信息
        if on_message:
            for tc in msg.tool_calls:
                on_message(f"  调用工具: {tc.function.name}")

        # 并行执行工具
        tool_results = ToolUtils.execute_tools(
            tools=tools,
            tool_calls=msg.tool_calls,
            display_message=on_message,
        )

        # 截断过长的工具输出
        for result in tool_results:
            if len(result["content"]) > max_tool_output:
                result["content"] = (
                    result["content"][:max_tool_output]
                    + f"\n... [输出被截断，原始长度: {len(result['content'])} 字符]"
                )

        # 工具结果加入历史
        messages.extend(tool_results)

        # 回调：显示工具结果
        if on_message:
            for result in tool_results:
                preview = result["content"][:200]
                if len(result["content"]) > 200:
                    preview += "..."
                on_message(f"  工具结果: {preview}")

        # 保存存档
        if checkpoint_mgr:
            checkpoint_mgr.save(messages, problem=problem)

        # 上下文截断
        messages = _trim_context(messages, max_tokens=100000)

    # 从最后一条 assistant 消息提取结果
    return _extract_result(messages)


def _msg_to_dict(msg: Any) -> Dict[str, Any]:
    """将 OpenAI message 对象转为可序列化的 dict。"""
    if hasattr(msg, "model_dump"):
        return msg.model_dump()
    # 兼容旧版 SDK
    result = {"role": msg.role or "assistant"}
    if msg.content:
        result["content"] = msg.content
    if msg.tool_calls:
        result["tool_calls"] = [
            {
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                },
            }
            for tc in msg.tool_calls
        ]
    return result


def _trim_context(
    messages: List[Dict[str, Any]],
    max_tokens: int = 100000,
) -> List[Dict[str, Any]]:
    """
    @brief 当消息列表过长时，截断最早的消息。
    @details 保留系统提示（第一条）+ 最近的消息。使用粗略的字符数估算 token。
    """
    # 粗略估算：1 token ≈ 2 个中文字符 或 4 个英文字符，取平均 3
    total_chars = sum(len(str(m.get("content", ""))) for m in messages)
    estimated_tokens = total_chars // 3

    if estimated_tokens <= max_tokens:
        return messages

    # 保留系统提示 + 截断早期消息
    system_msg = messages[0] if messages and messages[0].get("role") == "system" else None
    remaining = messages[1:] if system_msg else messages[:]

    # 删除最早的 20% 消息
    cut_count = max(1, len(remaining) // 5)
    remaining = remaining[cut_count:]
    logger.info("上下文截断：移除了 %d 条消息", cut_count)

    if system_msg:
        return [system_msg] + remaining
    return remaining


def _extract_result(messages: List[Dict[str, Any]]) -> str:
    """
    @brief 从消息历史中提取最终结果。
    @details 扫描最后一条 assistant 消息，查找 FLAG_FOUND 或 UNABLE_TO_SOLVE 标记。
    """
    # 从后往前找最后一条 assistant 消息
    for msg in reversed(messages):
        if msg.get("role") != "assistant":
            continue
        content = msg.get("content", "")
        if not content:
            continue

        # 检查 flag
        flag_match = re.search(r"FLAG_FOUND:\s*(.+)", content)
        if flag_match:
            return flag_match.group(1).strip()

        # 检查无法解决
        unsolvable_match = re.search(r"UNABLE_TO_SOLVE:\s*(.+)", content)
        if unsolvable_match:
            return f"未找到flag：{unsolvable_match.group(1).strip()}"

        # 有内容但没有标记，返回整个内容
        return content

    return "未找到flag：无输出"
```

- [ ] **Step 2: 验证模块可导入**

```bash
python -c "from agent.agent_core import run; print('OK')"
```

- [ ] **Step 3: 提交**

```bash
git add agent/agent_core.py
git commit -m "feat: 新增 agent_core 自主 Agent 循环"
```

---

## Task 8: 重写 workflow_runner.py

**Files:**
- Modify: `cli/adapters/workflow_runner.py:171-199` (重写 `run_workflow`)

- [ ] **Step 1: 重写 `run_workflow` 函数**

将 `cli/adapters/workflow_runner.py` 中的 `run_workflow` 函数（第 171-199 行）替换为：

```python
def run_workflow(
    config: Dict[str, Any],
    user_interface: UserInterface,
    problem: str,
    question: Question,
    resume_data: Optional[Dict[str, Any]],
) -> str:
    """执行 Agent 自主解题流程。"""
    import yaml
    from jinja2 import Environment, BaseLoader

    from agent.agent_core import run
    from agent.checkpoint import CheckpointManager
    from agent.skill import discover_skills
    from ctf_platform.registry import create_submitter
    from ctf_tool.load_skill import LoadSkillTool
    from utils.llm_request import LLMRequest
    from utils.tools import ToolUtils

    # 加载系统提示模板
    with open("./prompt/system_prompt.yaml", "r", encoding="utf-8") as f:
        prompt_data = yaml.safe_load(f)

    # 发现并加载 skills
    skill_registry = discover_skills(config=config, project_dir=".")

    # 加载工具（含 load_skill）
    tool_utils = ToolUtils()
    tools, tool_defs = tool_utils.load_tools()

    # 注册 load_skill 工具
    if skill_registry.skills:
        load_skill_tool = LoadSkillTool(skill_registry)
        tools["load_skill"] = load_skill_tool
        tool_defs.append(load_skill_tool.function_config)
        user_interface.display_message(
            f"已加载 {len(skill_registry.skills)} 个 skills: "
            + ", ".join(skill_registry.names())
        )

    if not tool_defs:
        user_interface.display_message("当前没有可用工具，无法解题")
        return "未找到flag：无可用工具"

    # 渲染系统提示（注入 skills 列表）
    env = Environment(loader=BaseLoader())
    template = env.from_string(prompt_data["system_prompt"])
    skills_info = [{"name": s.name, "description": s.description} for s in skill_registry.all()]
    system_prompt = template.render(
        question=problem,
        tools=tool_defs,
        skills=skills_info,
    )

    # 构建初始消息
    if resume_data and "messages" in resume_data:
        messages = resume_data["messages"]
        user_interface.display_message("已恢复存档，继续解题")
    else:
        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": problem},
        ]

    # 初始化 LLM
    llm = LLMRequest("solve_agent")

    # 初始化存档管理器
    checkpoint_dir = config.get("checkpoint_dir", "./checkpoints")
    checkpoint_mgr = CheckpointManager(checkpoint_dir=checkpoint_dir)

    # 设置 flag 确认回调
    platform_config = config.get("platform", {})
    submitter = create_submitter(
        platform_config.get("submitter", {"type": "manual"}),
        user_interface=user_interface,
    )

    max_tool_output = config.get("max_tool_output", 8192)

    # 运行 agent 循环
    result = run(
        messages=messages,
        tools=tools,
        tool_defs=tool_defs,
        llm=llm,
        on_message=user_interface.display_message,
        checkpoint_mgr=checkpoint_mgr,
        problem=problem,
        max_tool_output=max_tool_output,
    )

    # 尝试提交 flag
    if "flag{" in result.lower() or "FLAG_FOUND" in result:
        flag_candidate = _extract_flag_from_result(result)
        if flag_candidate:
            submit_result = submitter.submit(flag_candidate, question)
            if submit_result.success:
                checkpoint_mgr.delete_latest()
                return flag_candidate

    return result


def _extract_flag_from_result(result: str) -> Optional[str]:
    """从结果字符串中提取 flag。"""
    import re
    match = re.search(r"flag\{[^}]+\}", result, re.IGNORECASE)
    if match:
        return match.group(0)
    match = re.search(r"FLAG_FOUND:\s*(.+)", result)
    if match:
        return match.group(1).strip()
    return None
```

- [ ] **Step 2: 确认不再需要的 import 已清理**

`workflow_runner.py` 顶部不再需要 `from agent.workflow import Workflow`（它在 `run_workflow` 内部延迟导入，现在改为导入 `agent_core` 和 `agent.skill`）。确认没有残留的旧 import。

- [ ] **Step 3: 提交**

```bash
git add cli/adapters/workflow_runner.py
git commit -m "refactor: 重写 run_workflow 使用 agent_core 自主循环"
```

---

## Task 9: 更新 solve.py CLI 命令

**Files:**
- Modify: `cli/commands/solve.py`

去掉 auto/manual 模式选择（完全自主模式无需选择），简化启动面板。

- [ ] **Step 1: 修改 `solve_command` 函数**

将 `cli/commands/solve.py` 中的 `solve_command` 函数替换为：

```python
def solve_command(
    question_file: Optional[str] = typer.Option(
        None,
        "--question-file",
        help="从文件读取题目文本",
    ),
    question: Optional[str] = typer.Option(
        None,
        "--question",
        help="直接传入题目文本",
    ),
    resume: bool = typer.Option(
        True,
        "--resume/--no-resume",
        help="是否尝试恢复存档",
    ),
    attachments_dir: Optional[str] = typer.Option(
        None,
        "--attachments-dir",
        help="覆盖附件目录",
    ),
    show_think: bool = typer.Option(
        True,
        "--show-think/--hide-think",
        help="是否显示思考过程",
    ),
    plain: bool = typer.Option(
        False,
        "--plain",
        help="关闭彩色输出，回退为基础命令行交互",
    ),
) -> None:
    """启动自主解题流程。"""
    setup_logging()
    config = Config.load_config()

    ui = RichPromptToolkitInterface(
        plain=plain,
        show_think=show_think,
        forced_auto_mode=True,
        forced_resume=resume if not resume else None,
    )

    checkpoint_dir_value = config.get("checkpoint_dir", "./checkpoints")
    checkpoint_dir = checkpoint_dir_value if isinstance(checkpoint_dir_value, str) else "./checkpoints"
    checkpoint_mgr = CheckpointManager(checkpoint_dir=checkpoint_dir)

    resume_data = load_checkpoint_for_solve(
        checkpoint_mgr=checkpoint_mgr,
        allow_resume=resume,
        ui=ui,
    )

    problem, question_data, source = resolve_question(
        config=config,
        question_text=question,
        question_file=question_file,
        attachment_dir_override=attachments_dir,
        user_interface=ui,
    )

    if isinstance(ui, RichPromptToolkitInterface):
        ckpt_status = "将尝试恢复" if resume else "不恢复"
        ui.display_startup(
            mode_text="自主模式",
            question_source=source,
            attachments_dir=attachments_dir or "./attachments",
            checkpoint_status=ckpt_status,
        )

    try:
        result = run_workflow(
            config=config,
            user_interface=ui,
            problem=problem,
            question=question_data,
            resume_data=resume_data,
        )
    except ModuleNotFoundError as error:
        raise typer.BadParameter(
            f"缺少运行依赖: {error.name}，请先执行 `pip install -r requirements.txt`"
        ) from error

    console = Console(no_color=plain, force_terminal=not plain)
    console.print(
        Panel(
            str(result),
            title="最终结果",
            border_style="green",
        )
    )
```

- [ ] **Step 2: 清理不再需要的 import**

删除 `cli/commands/solve.py` 顶部不再使用的 import（如果有的话）。确认 `auto` 和 `manual` 参数已移除。

- [ ] **Step 3: 提交**

```bash
git add cli/commands/solve.py
git commit -m "refactor: solve 命令去掉 auto/manual 模式选择"
```

---

## Task 10: 清理 UserInterface 和 RichPromptToolkitInterface

**Files:**
- Modify: `utils/user_interface.py`
- Modify: `cli/ui/interface.py`

删除不再需要的 `select_mode`, `manual_approval`, `manual_approval_step` 方法。

- [ ] **Step 1: 精简 `utils/user_interface.py`**

用以下内容替换 `utils/user_interface.py`：

```python
from abc import ABC, abstractmethod
from typing import Any


class UserInterface(ABC):
    """
    @brief 用户交互抽象接口。
    """

    @abstractmethod
    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        @brief 让用户确认候选 flag 是否正确。
        @param flag_candidate 候选 flag。
        @return 用户确认结果。
        """
        pass

    @abstractmethod
    def input_question_ready(self, prompt: str) -> None:
        """
        @brief 等待用户确认题目输入已准备完毕。
        @param prompt 输入提示信息。
        """
        pass

    @abstractmethod
    def display_message(self, message: str) -> None:
        """
        @brief 向用户显示消息。
        @param message 要显示的消息内容。
        """
        pass

    @abstractmethod
    def confirm_resume(self) -> bool:
        """
        @brief 询问用户是否恢复存档。
        @return 用户选择结果。
        """
        pass
```

- [ ] **Step 2: 精简 `cli/ui/interface.py`**

从 `cli/ui/interface.py` 中删除以下方法：
- `select_mode`（第 128-144 行）
- `manual_approval`（第 176-185 行）
- `manual_approval_step`（第 193-224 行）
- `_prompt_choice` 中对 `manual_approval_step` 的引用（如果有）

同时删除不再使用的 import：
- `from utils.user_interface import ManualApprovalStepData, ToolCall, UserInterface` 简化为 `from utils.user_interface import UserInterface`
- `Table` import（如果只在已删除的方法中使用）

保留的方法：`confirm_flag`, `input_question_ready`, `display_message`, `confirm_resume`, 以及所有 `render_*` 辅助方法和 `display_startup`。

- [ ] **Step 3: 提交**

```bash
git add utils/user_interface.py cli/ui/interface.py
git commit -m "refactor: 精简 UserInterface，删除手动审批相关方法"
```

---

## Task 11: 清理 utils/text.py 和 config_template.json

**Files:**
- Modify: `utils/text.py`
- Modify: `config_template.json`

- [ ] **Step 1: 从 `utils/text.py` 删除 `fix_json_with_llm` 函数**

删除第 10-36 行的 `fix_json_with_llm` 函数。保留 `optimize_text` 函数。

同时删除不再需要的 import（`json_repair` 相关）。

精简后的 `utils/text.py`：

```python
import re


def optimize_text(text: str) -> str:
    """
    @brief 缩减 Prompt 中的重复空白字符。
    @param text 待优化文本。
    @return 优化后的文本。
    """
    text = re.sub(r"(\s)\1+", r"\1", text)
    return text.strip()
```

- [ ] **Step 2: 更新 `config_template.json`**

删除不再需要的配置项 `max_history_steps` 和 `compression_threshold`，增加 `max_tool_output`：

```json
{
    "llm": {
        "model": "gpt-4o-mini",
        "api_key": "",
        "api_base": "https://api.openai.com/v1"
    },
    "max_tool_output": 8192,
    "checkpoint_dir": "./checkpoints",
    "tool_config": {
        "bash_shell": {
            "shell_path": "bash",
            "working_dir": ".",
            "timeout": 30,
            "login_shell": false,
            "env": {}
        }
    },
    "mcp_server": {},
    "skills": {
        "paths": []
    },
    "platform": {
        "inputer": {
            "type": "file",
            "file_path": "./question.txt",
            "attachment_dir": "./attachments"
        },
        "submitter": {
            "type": "manual"
        }
    }
}
```

- [ ] **Step 3: 提交**

```bash
git add utils/text.py config_template.json
git commit -m "refactor: 清理 text.py 和 config_template.json"
```

---

## Task 12: 删除旧文件

**Files:**
- Delete: `agent/solve_agent.py`
- Delete: `agent/workflow.py`
- Delete: `agent/analyzer.py`
- Delete: `agent/memory.py`
- Delete: `prompt.yaml`

- [ ] **Step 1: 确认无残留引用**

运行以下命令确认没有其他文件还在引用已删除的模块：

```bash
grep -r "from agent.solve_agent" --include="*.py" .
grep -r "from agent.workflow" --include="*.py" .
grep -r "from agent.analyzer" --include="*.py" .
grep -r "from agent.memory" --include="*.py" .
grep -r "import prompt.yaml" --include="*.py" .
grep -r "from prompt import" --include="*.py" .
```

如果有残留引用，先修复再删除。

- [ ] **Step 2: 删除文件**

```bash
rm agent/solve_agent.py agent/workflow.py agent/analyzer.py agent/memory.py prompt.yaml
```

- [ ] **Step 3: 提交**

```bash
git add -A
git commit -m "refactor: 删除旧的 SolveAgent/Workflow/Analyzer/Memory 和 prompt.yaml"
```

---

## Task 13: 端到端验证

- [ ] **Step 1: 确认所有模块可导入**

```bash
python -c "
from agent.agent_core import run
from agent.checkpoint import CheckpointManager
from agent.skill import discover_skills, SkillRegistry
from ctf_tool.load_skill import LoadSkillTool
from utils.tools import ToolUtils
from utils.llm_request import LLMRequest
from utils.text import optimize_text
from cli.adapters.workflow_runner import run_workflow
from cli.commands.solve import solve_command
print('OK: 所有模块导入成功')
"
```

- [ ] **Step 2: 运行 `config check` 命令验证配置**

```bash
python main.py config check
```

- [ ] **Step 3: 运行 `tools list` 命令验证工具加载**

```bash
python main.py tools list
```

- [ ] **Step 4: 验证 skill 发现和 load_skill 工具**

```bash
python -c "
from agent.skill import discover_skills
from ctf_tool.load_skill import LoadSkillTool

registry = discover_skills(project_dir='.')
print(f'发现 {len(registry.skills)} 个 skills:')
for s in registry.all():
    print(f'  - {s.name}: {s.description[:60]}')

tool = LoadSkillTool(registry)
result = tool.execute('load_skill', {'name': 'web-exploitation'})
assert 'SQL' in result
print('OK: skill 系统正常')
"
```

- [ ] **Step 5: 使用简单题目做端到端测试**

创建一个简单的测试题目文件，运行 solve 命令验证完整流程：

```bash
echo "What is 1+1?" > question.txt
python main.py solve --question "What is 1+1?" --no-resume --plain
```

预期：LLM 直接回答 2 并停止（不调用工具），或调用 shell 工具执行计算后停止。

- [ ] **Step 6: 最终提交**

```bash
git add -A
git commit -m "refactor: 完成开放式 Agent 工作流重构"
```
