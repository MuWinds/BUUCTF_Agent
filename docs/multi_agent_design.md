# BUUCTF Agent 多 Agent 架构设计文档

## 1. 背景与动机

### 1.1 当前架构

当前系统是一个**单 Agent 循环**架构：

```
main.py → Workflow → SolveAgent（think → execute → analyze → repeat）→ Flag
```

核心数据流：`题目文本` → `SolveAgent.next_instruction()` → `LLM 规划下一步` → `execute_tools()` → `Analyzer.analyze_step_output()` → `Memory.add_step()` → `循环或终止`

当前系统只有**静态工具集**：`execute_shell_command`。Agent 只能用 Bash 命令完成所有操作，无法根据题目特点创造专用工具。

### 1.2 当前架构的局限

| 问题 | 影响 |
|------|------|
| **单模型瓶颈** | 同一个 LLM 同时负责规划、分析、反思，上下文窗口压力大 |
| **无任务分解** | Agent 只能逐步推进，无法将复杂题目拆解为并行子任务 |
| **无角色分工** | 缺少"侦察→分析→利用"的专业化分工，策略选择粗糙 |
| **被动反思** | 只在用户手动反馈时才触发反思，无法自主纠错 |
| **静态工具集** | 只能使用预置的 `execute_shell_command`，无法根据题目需求创建专用工具 |
| **Shell 带宽低** | 复杂逻辑（如定制加密/解密、协议解析）通过 Bash 传递效率极低 |
| **无并行探索** | 遇到分叉路径时无法同时尝试多种方法 |
| **单点故障** | LLM 卡住时整个解题流程停滞 |

### 1.3 改造目标

1. 引入多 Agent 协作，实现任务分解和专业分工
2. **核心目标：Agent 能根据题目需求自主创建、验证、注册和使用新工具**
3. 保持与现有工具系统、记忆系统、检查点系统的兼容
4. 支持配置化的 Agent 数量、模型选择和协作策略
5. 逐步演进：先串行后并行，先简单后复杂

---

## 2. 总体架构

### 2.1 架构概览

```
                          ┌──────────────┐
                          │   用户输入    │
                          │ (CLI / API)  │
                          └──────┬───────┘
                                 │
                          ┌──────▼───────┐
                          │  Workflow    │  ← 入口编排（保持现有接口）
                          └──────┬───────┘
                                 │
           ┌─────────────────────▼─────────────────────┐
           │              Orchestrator                 │
           │              (编排Agent)                  │
           │  ┌───────────────────────────────────┐   │
           │  │  题目分析 → 策略规划 → 任务分解    │   │
           │  │  结果综合 → 策略调整 → 工具缺口识别│   │
           │  └───────────────────────────────────┘   │
           └───┬────────┬────────┬────────┬───────────┘
               │        │        │        │
    ┌──────────┼────────┼────────┼────────┼──────────┐
    │          │        │        │        │          │
    ▼          ▼        ▼        ▼        ▼          ▼
┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐ ┌──────────┐
│Recon │ │Exploit│ │Analy │ │Reflec│ │ToolSmith│ │Coordinator│
│Agent │ │Agent  │ │-sis  │ │-tion │ │ Agent   │ │  (Legacy) │
│      │ │       │ │Agent │ │Agent │ │         │ │          │
├──────┤ ├───────┤ ├──────┤ ├──────┤ ├─────────┤ ├──────────┤
│信息  │ │漏洞   │ │输出  │ │失败  │ │工具创造 │ │向后兼容  │
│收集  │ │利用   │ │分析  │ │复盘  │ │代码生成 │ │单Agent   │
│文件  │ │Payload│ │Flag  │ │策略  │ │工具验证 │ │模式      │
│分析  │ │构造   │ │检测  │ │建议  │ │工具迭代 │ │          │
└──┬───┘ └──┬────┘ └──┬───┘ └──┬───┘ └────┬────┘ └────┬─────┘
   │        │         │        │          │          │
   └────────┼─────────┼────────┼──────────┼──────────┘
            │         │        │          │
        ┌───▼─────────▼────────▼──────────▼───┐
        │          共享基础设施                │
        │  ┌──────────────────────────────┐   │
        │  │  SharedMemory (共享记忆)     │   │
        │  │  MessageBus (消息总线)       │   │
        │  │  DynamicToolRegistry (动态工具)│  │
        │  │  ToolSandbox (工具沙箱)       │   │
        │  │  CheckpointManager (检查点)   │   │
        │  └──────────────────────────────┘   │
        └─────────────────────────────────────┘
```

### 2.2 核心创新：动态工具创建能力

区别于传统多Agent系统，本架构的**关键差异化能力**是 Agent 能自主创造工具。

**传统模式：** Agent → 从固定工具集中选择 → 执行 → 分析

**本架构模式：** Agent → 分析题目需求 → 发现工具缺口 → ToolSmith Agent 生成专用脚本 → 验证 → 注册为临时工具 → 执行 Agent 调用新工具 → 迭代优化

```
题目需求分析
     │
     ▼
┌─────────────┐     ┌──────────────┐     ┌──────────────┐
│ Orchestrator│────►│  ToolSmith   │────►│  ToolSandbox │
│ 识别工具缺口 │     │  生成工具代码 │     │  安全验证    │
└─────────────┘     └──────┬───────┘     └──────┬───────┘
                           │                    │
                           │ 验证失败，迭代修改  │
                           │◄───────────────────┘
                           │
                           │ 验证通过
                           ▼
                    ┌──────────────┐     ┌──────────────┐
                    │DynamicToolReg│────►│ Exploit/Recon│
                    │ 注册为新工具  │     │ 调用新工具    │
                    └──────────────┘     └──────┬───────┘
                                                │
                                                ▼
                                         ┌──────────────┐
                                         │  Analysis    │
                                         │  评估工具效果 │
                                         └──────────────┘
```

---

## 3. Agent 角色定义

### 3.1 Orchestrator（编排Agent）—— 大脑

| 维度 | 说明 |
|------|------|
| **职责** | 题目分类、策略规划、任务分解与分派、进度跟踪、**识别工具缺口并触发工具创建**、策略切换决策 |
| **输入** | 题目描述、全局记忆摘要、各Agent状态报告、**当前可用工具列表** |
| **输出** | 任务分配指令、策略调整、下一步决策、**工具创建请求** |
| **模型建议** | 强推理模型（如 gpt-4o / claude-sonnet-4-6） |
| **触发时机** | 每个解题回合开始时、子任务完成后、遇到瓶颈时、发现工具缺口时 |

核心 Prompt 片段：

```
你是一个CTF解题的总指挥。你需要分析题目，制定策略，并将任务分配给专业Agent。
此外，如果你发现当前可用工具无法满足解题需求，你可以请求ToolSmith Agent创建新工具。

当前题目：{question}
当前进度：{progress_summary}
当前可用工具：{available_tools}
可用Agent：recon, exploit, analysis, toolsmith, reflection

请输出JSON格式的策略计划：
{
    "problem_type": "web/crypto/pwn/reverse/misc",
    "strategy": "整体解题思路",
    "tool_gap_analysis": {
        "has_gap": true/false,
        "gap_description": "当前工具无法满足的需求是什么",
        "required_tool": {
            "name": "建议的工具名称",
            "purpose": "工具需要解决什么问题",
            "input_spec": "期望的输入参数",
            "output_spec": "期望的输出格式",
            "language": "python/bash",
            "rationale": "为什么现有Shell命令无法满足"
        }
    },
    "tasks": [
        {
            "id": "task_1",
            "assignee": "recon | exploit | analysis | toolsmith",
            "instruction": "具体任务描述",
            "expected_output": "预期产出",
            "priority": 1,
            "required_tools": ["tool_name"]
        }
    ],
    "next_action": "需要立即执行的下一步"
}
```

### 3.2 Recon Agent（侦察Agent）—— 眼睛

| 维度 | 说明 |
|------|------|
| **职责** | 信息收集：文件类型识别、端口扫描、目录枚举、网页爬取、附件解压与内容分析 |
| **输入** | 题目描述、附件路径、目标地址、历史发现 |
| **输出** | 侦察报告：发现的服务、文件内容摘要、关键线索 |
| **模型建议** | 快速模型（如 gpt-4o-mini / claude-haiku-4-5） |
| **特点** | 可并行执行多个探测任务，容忍部分失败；**也可请求ToolSmith创建专用扫描脚本** |

### 3.3 Exploit Agent（利用Agent）—— 手

| 维度 | 说明 |
|------|------|
| **职责** | 编写和执行漏洞利用代码、构造payload、与目标服务交互、绕过安全机制 |
| **输入** | 侦察报告、漏洞线索、目标信息 |
| **输出** | 利用结果、获得的权限/数据、遇到的问题 |
| **模型建议** | 强推理模型（如 gpt-4o / claude-sonnet-4-6） |
| **特点** | 基于侦察发现行动，每次操作有明确目标；**对复杂利用场景可请求专用工具** |

### 3.4 Analysis Agent（分析Agent）—— 判断

| 维度 | 说明 |
|------|------|
| **职责** | 分析命令/工具输出、检测flag、验证结果有效性、**评估动态创建工具的效果** |
| **输入** | 工具执行输出、当前上下文、历史类似案例 |
| **输出** | 分析报告：关键发现、flag候选、置信度评估、**工具效果评分** |
| **模型建议** | 轻量模型（如 gpt-4o-mini / claude-haiku-4-5） |
| **特点** | 每次工具执行后触发，高度结构化输出 |

### 3.5 Reflection Agent（反思Agent）—— 自我纠错

| 维度 | 说明 |
|------|------|
| **职责** | 回顾失败尝试、分析失败模式、提出替代策略、**评估创建的工具是否解决了真正的问题** |
| **输入** | 失败历史、尝试过的路径、**已创建工具的使用记录和效果** |
| **输出** | 反思报告：失败原因、替代方案、策略建议、**工具改进建议** |
| **模型建议** | 强推理模型（如 gpt-4o / claude-sonnet-4-6） |
| **触发** | 连续失败 / 工具创建后使用效果差 / 主动请求 / 解题超时 |

### 3.6 ToolSmith Agent（工具锻造Agent）—— 创造力核心

这是本架构**最关键的差异化能力**：Agent 能自主分析需求并生成新工具。

| 维度 | 说明 |
|------|------|
| **职责** | 分析工具需求 → 生成工具代码（Python/Bash） → 自我验证 → 提交注册 → 根据反馈迭代改进 |
| **输入** | 工具需求规格（来自Orchestrator或其他Agent）、当前可用的执行环境信息 |
| **输出** | 可执行的工具代码文件、工具描述和接口定义、验证报告 |
| **模型建议** | 强代码生成模型（如 gpt-4o / claude-sonnet-4-6） |
| **特点** | 这是唯一可以"扩展系统能力"的Agent |

核心 Prompt 片段：

```
你是一个CTF工具开发专家。你的任务是创建专用工具来解决特定问题。

当前系统环境：
- 可用解释器：{available_interpreters}
- 可用外部命令：{available_commands}
- 工作目录：{working_dir}
- 附件目录：{attachments_dir}

工具需求：
- 名称：{tool_name}
- 用途：{purpose}
- 期望输入：{input_spec}
- 期望输出：{output_spec}

请生成以下内容：
1. 工具代码（Python 或 Bash）
2. 工具的使用说明
3. 工具的函数配置（用于注册到LLM工具列表）

输出JSON：
{
    "tool_code": "完整的工具代码",
    "language": "python/bash",
    "tool_config": {
        "type": "function",
        "function": {
            "name": "工具函数名",
            "description": "工具功能描述",
            "parameters": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }
    },
    "self_test": {
        "test_command": "验证命令",
        "expected_output_pattern": "预期输出模式"
    }
}
```

---

## 4. 动态工具系统设计

### 4.1 DynamicToolRegistry（动态工具注册中心）

```python
from typing import Any, Dict, List, Optional, Callable
from dataclasses import dataclass, field
from datetime import datetime
import uuid

@dataclass
class DynamicTool:
    """运行时动态创建的工具"""

    tool_id: str                          # 唯一标识
    name: str                             # 工具函数名
    description: str                      # 功能描述
    code: str                             # 工具源代码
    language: str                         # python 或 bash
    file_path: str                        # 工具脚本文件路径
    function_config: Dict[str, Any]       # LLM工具配置
    status: str = "active"                # active | deprecated | failed
    created_by: str = ""                  # 创建者Agent
    created_at: str = ""                  # 创建时间
    usage_count: int = 0                  # 使用次数
    success_count: int = 0                # 成功次数
    feedback_history: List[Dict] = field(default_factory=list)  # 使用反馈

class DynamicToolRegistry:
    """运行时工具注册、发现和管理"""

    def __init__(self, tools_dir: str = "./generated_tools"):
        self.tools_dir = tools_dir
        self._tools: Dict[str, DynamicTool] = {}  # tool_id → DynamicTool
        self._name_index: Dict[str, str] = {}      # tool_name → tool_id
        self._ensure_tools_dir()

    def register(
        self,
        name: str,
        description: str,
        code: str,
        language: str,
        function_config: Dict[str, Any],
        created_by: str = "",
    ) -> DynamicTool:
        """
        注册新工具：
        1. 分配唯一ID
        2. 写入工具脚本文件（generated_tools/{tool_id}.py/.sh）
        3. 加入注册表
        4. 更新LLM工具列表
        """
        pass

    def get_llm_function_configs(self) -> List[Dict[str, Any]]:
        """获取所有活跃工具的LLM函数配置，注入到Agent的工具列表中"""
        pass

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        """执行动态工具"""
        pass

    def record_feedback(
        self,
        tool_id: str,
        success: bool,
        feedback: str,
    ) -> None:
        """记录工具使用反馈"""
        pass

    def deprecate(self, tool_id: str, reason: str) -> None:
        """废弃效果不佳的工具"""
        pass

    def iterate_tool(
        self,
        tool_id: str,
        new_code: str,
        new_function_config: Dict[str, Any],
    ) -> DynamicTool:
        """基于反馈迭代工具，保留历史版本"""
        pass

    def to_dict(self) -> Dict:
        """序列化所有动态工具（用于检查点保存）"""
        pass

    def restore_from_dict(self, data: Dict) -> None:
        """从检查点恢复"""
        pass
```

### 4.2 ToolSandbox（工具沙箱）

创建的工具在执行前需要经过安全验证，防止恶意代码或无限循环导致系统问题。

```python
class ToolSandbox:
    """工具代码安全验证沙箱"""

    # 危险模式黑名单
    BANNED_PATTERNS = [
        r"os\.system\(.*rm\s+-rf\s+/",       # 危险删除
        r"subprocess\.run\(.*shell=True.*rm", # 通过shell删除
        r"__import__\(.*os.*\).*system",      # 动态导入执行
        r"while\s+True:",                     # 无限循环（需额外检查）
        r"fork\s*\(\s*\)",                   # 进程fork炸弹
        r"eval\s*\(.*__",                     # 危险eval
        r"exec\s*\(.*__",                     # 危险exec
    ]

    # 资源限制
    TIMEOUT_SECONDS = 60
    MAX_OUTPUT_BYTES = 1024 * 1024  # 1MB

    def validate(self, code: str, language: str) -> ValidationResult:
        """
        验证工具代码安全性：
        1. 静态分析：正则匹配危险模式
        2. 语法检查：python -m py_compile 或 bash -n
        3. 沙箱执行：子进程隔离，超时控制
        4. 输出校验：输出大小限制
        """
        pass

    def dry_run(self, code: str, language: str, test_input: Dict) -> str:
        """试运行工具代码，检查是否能正常执行"""
        pass
```

### 4.3 工具创建完整流程

```
Step 1: 需求识别
  Orchestrator/Recon/Exploit 分析当前任务
   → 判断现有工具是否满足需求
   → 如不满足，生成工具需求规格

Step 2: 工具生成 (ToolSmith Agent)
   → 接收工具需求规格
   → 分析执行环境（Python版本、可用库等）
   → 生成工具代码（优先Python，备选Bash）
   → 生成LLM工具配置（函数名、参数、描述）

Step 3: 安全验证 (ToolSandbox)
   → 静态代码分析
   → 语法检查
   → 试运行测试
   → 通过 → 进入注册；失败 → 返回ToolSmith迭代

Step 4: 工具注册 (DynamicToolRegistry)
   → 写入 generated_tools/{tool_id}.py
   → 加入注册表
   → 更新全局LLM工具列表

Step 5: 工具使用 (Exploit/Recon Agent)
   → Agent看到新工具出现在可用工具列表中
   → 调用新工具执行任务
   → Analysis Agent评估工具效果

Step 6: 反馈迭代
   → Analysis Agent记录工具效果
   → 如效果差 → Reflection Agent分析原因
   → ToolSmith Agent根据反馈改进工具
   → 新版本替换旧版本（保留历史）
```

### 4.4 工具迭代与版本管理

```
generated_tools/
├── tool_a1b2c3d4_v1.py     # 第一版
├── tool_a1b2c3d4_v2.py     # 第二版（改进）
├── tool_a1b2c3d4.meta.json # 元数据（版本历史、使用统计）
├── tool_e5f6g7h8_v1.sh     # Bash工具
└── ...
```

每个工具保留完整的版本历史，Reflection Agent 可以对比版本间的效果差异。

### 4.5 工具创建示例

**题目场景：** 一道 Crypto 题目，加密算法是自定义的 Feistel 网络变体

```
1. Recon Agent 分析附件
   → 发现 encrypt.py 使用了自定义的 Feistel 加密
   → 识别出需要编写对应的解密脚本

2. Orchestrator 识别工具缺口
   → 当前只有 execute_shell_command
   → 用Bash heredoc写复杂Python代码容易出错
   → 决定创建专用解密工具

3. ToolSmith Agent 生成工具
   → 分析 encrypt.py 的加密逻辑
   → 生成 decrypt_tool.py，实现反向解密
   → 生成函数配置：
     {
       "name": "decrypt_custom_feistel",
       "description": "对自定义Feistel加密的密文进行解密",
       "parameters": {
         "ciphertext": {"type": "string", "description": "密文(hex)"},
         "key": {"type": "string", "description": "密钥"},
         "rounds": {"type": "integer", "description": "加密轮数", "default": 16}
       }
     }

4. ToolSandbox 验证
   → 静态分析通过
   → 用已知明文-密文对测试
   → 验证通过

5. Exploit Agent 使用新工具
   → 调用 decrypt_custom_feistel(ciphertext="...", key="...", rounds=16)
   → 成功解密获得flag
```

---

## 5. 通信与协调机制

### 5.1 消息总线（MessageBus）

所有 Agent 间通信通过统一的消息总线：

```
消息类型扩展（新增工具相关）：
{
    "id": "msg_uuid",
    "type": "task_assignment | task_result | status_query |
             strategy_update | alert |
             tool_request | tool_created | tool_feedback | tool_deprecate",
    "sender": "orchestrator",
    "receiver": "toolsmith | broadcast",
    "timestamp": "2026-04-27T12:00:00Z",
    "payload": { ... },
    "reply_to": "msg_uuid"
}
```

新增消息类型：

| 消息类型 | 发送者 | 接收者 | 用途 |
|----------|--------|--------|------|
| `tool_request` | Orchestrator/任意Agent | ToolSmith | 请求创建新工具 |
| `tool_created` | ToolSmith | Broadcast | 通知新工具可用 |
| `tool_feedback` | Analysis Agent | ToolSmith/Registry | 工具使用效果反馈 |
| `tool_deprecate` | Orchestrator | Registry/Broadcast | 废弃效果差的工具 |
| `tool_iterate` | Orchestrator/Reflection | ToolSmith | 请求改进现有工具 |

### 5.2 Agent 生命周期

```
                    ┌──────────┐
                    │  IDLE    │
                    └────┬─────┘
                         │ 收到任务
                    ┌────▼─────┐
              ┌─────│ THINKING │◄──────┐
              │     └────┬─────┘       │
              │          │              │
              │     ┌────┴────────┐    │
              │     │ 需要工具？   │    │
              │     └────┬────────┘    │
              │     是   │       否    │
              │  ┌───────▼──┐    │     │
              │  │ 请求     │    │     │
              │  │ToolSmith │    │     │
              │  │创建工具  │    │     │
              │  └────┬─────┘    │     │
              │       │ 工具就绪 │     │
              │       └────┬─────┘     │
              │            │           │
              │     ┌──────▼──────┐    │
              │     │  EXECUTING  │    │
              │     └──────┬──────┘    │
              │            │ 执行完成   │
              │     ┌──────▼──────┐    │
              │     │  REPORTING  ├────┘
              │     └──────┬──────┘
              │            │ 任务完成
              │     ┌──────▼──────┐
              └─────│   IDLE     │
                    └─────────────┘
```

### 5.3 协作时序图（完整解题示例）

```
题目: "一个自定义加密算法的Crypto挑战"

Orchestrator    Recon       ToolSmith    Exploit     Analysis    Reflection
    │             │            │           │           │           │
    │──分析题目──►│            │           │           │           │
    │             │            │           │           │           │
    │◄─侦察报告──│            │           │           │           │
    │  (发现encrypt.py)       │           │           │           │
    │             │            │           │           │           │
    │ 识别工具缺口 │           │           │           │           │
    │──────创建请求───────────►│           │           │           │
    │             │            │           │           │           │
    │             │   生成解密工具          │           │           │
    │             │   验证通过             │           │           │
    │◄─────工具就绪───────────│           │           │           │
    │             │            │           │           │           │
    │────────────────────分配利用任务──────►│           │           │
    │             │            │           │           │           │
    │             │            │   调用解密工具        │           │
    │             │            │           │           │           │
    │             │            │           │──分析结果─►│           │
    │             │            │           │           │           │
    │             │            │           │◄─Flag!────│           │
    │             │            │           │           │           │
    │◄───────────Flag─────────────────────│           │           │
    │             │            │           │           │           │
```

---

## 6. 解题流程重构

### 6.1 新解题流程

```
Phase 1: 题目分析 (Orchestrator)
  └─ 分析题目类型、难度
  └─ 提取关键信息
  └─ 扫描当前可用工具
  └─ 生成初始策略和工具需求

Phase 2: 侦察阶段 (Recon Agent)
  └─ 分析附件文件
  └─ 探测目标服务
  └─ 收集环境信息
  └─ 输出侦察报告
  └─ [条件] 如需专用探测工具 → 请求ToolSmith

Phase 3: 工具准备 (ToolSmith Agent，条件触发)
  └─ 接收工具需求
  └─ 生成专用工具代码
  └─ 沙箱验证
  └─ 注册到动态工具注册表
  └─ 通知所有Agent新工具可用

Phase 4: 策略制定 (Orchestrator)
  └─ 基于侦察报告和可用工具制定攻击计划
  └─ 将计划分解为可执行任务
  └─ 按优先级排序任务

Phase 5: 执行循环 (Exploit + Analysis)
  └─ Exploit Agent 执行攻击任务（可使用动态创建的工具）
  └─ Analysis Agent 分析执行结果
  └─ 若发现 flag → 验证并提交
  └─ 若失败 → 记录原因

Phase 6: 反思调整 (Reflection Agent，条件触发)
  └─ 连续失败时触发
  └─ 分析工具是否有效
  └─ 生成替代策略
  └─ [条件] 请求ToolSmith改进工具
  └─ Orchestrator 更新计划

Phase 7: 继续或终止
  └─ 返回 Phase 5 或 Phase 3 或 Phase 2
  └─ 达到最大轮次或 flag 找到 → 结束
```

### 6.2 与现有代码的对应关系

| 现有模块 | 改造方式 |
|----------|----------|
| `agent/workflow.py` | 保留为入口，内部改用 Orchestrator |
| `agent/solve_agent.py` | 保留为 Legacy 向后兼容模式 |
| `agent/analyzer.py` | 升级为 Analysis Agent |
| `agent/memory.py` | 扩展为 SharedMemory，支持多Agent视图 |
| `agent/checkpoint.py` | 扩展保存范围：多Agent状态 + 动态工具 |
| `utils/tools.py` (ToolUtils) | 扩展：整合 DynamicToolRegistry |
| `utils/llm_request.py` | 扩展为支持多个模型独立配置 |
| `ctf_tool/bash_shell.py` | 保留，作为动态工具的执行基座 |
| `config_template.json` | 新增 agents、orchestration、tool_autonomy 配置段 |
| `prompt.yaml` | 拆分为 `prompts/` 目录，按角色独立配置 |
| `generated_tools/` | **[新增]** 动态工具存储目录 |

---

## 7. 代码结构设计

### 7.1 目录结构

```
BUUCTF_Agent/
├── agent/                              # Agent 模块
│   ├── __init__.py
│   ├── base_agent.py                  # [新增] Agent 抽象基类
│   ├── orchestrator.py                # [新增] 编排Agent
│   ├── recon_agent.py                 # [新增] 侦察Agent
│   ├── exploit_agent.py               # [新增] 利用Agent
│   ├── analysis_agent.py              # [新增] 分析Agent
│   ├── reflection_agent.py            # [新增] 反思Agent
│   ├── toolsmith_agent.py             # [新增] ★ 工具锻造Agent
│   ├── agent_registry.py              # [新增] Agent 注册与管理
│   ├── message_bus.py                 # [新增] 消息总线
│   ├── task.py                        # [新增] 任务定义与管理
│   ├── tool_sandbox.py                # [新增] ★ 工具安全沙箱
│   ├── dynamic_tool_registry.py       # [新增] ★ 动态工具注册中心
│   ├── workflow.py                    # [修改] 适配多Agent
│   ├── solve_agent.py                 # [保留] 向后兼容Legacy模式
│   ├── analyzer.py                    # [保留] 基础分析器
│   ├── memory.py                      # [修改] 扩展为 SharedMemory
│   └── checkpoint.py                  # [修改] 扩展状态保存范围
├── ctf_tool/                          # 工具模块
│   ├── __init__.py
│   ├── base_tool.py                   # [保留] 工具基类
│   ├── bash_shell.py                  # [保留] Shell执行工具
│   ├── mcp_adapter.py                 # [保留] MCP适配器
│   └── dynamic_tool_wrapper.py        # [新增] ★ 动态工具执行包装器
├── generated_tools/                   # [新增] ★ 动态工具输出目录
│   └── .gitkeep
├── cli/                               # 不变
├── utils/                             # 工具函数
│   ├── llm_request.py                 # [修改] 多模型支持
│   ├── tools.py                       # [修改] 整合动态工具注册
│   ├── text.py
│   └── user_interface.py
├── prompts/                           # [新增] 按角色拆分
│   ├── orchestrator.yaml
│   ├── recon.yaml
│   ├── exploit.yaml
│   ├── analysis.yaml
│   ├── reflection.yaml
│   └── toolsmith.yaml                 # ★ ToolSmith专用Prompt
├── config_template.json               # [修改] 扩展配置
└── docs/
    └── multi_agent_design.md
```

### 7.2 核心类设计

#### BaseAgent

```python
from abc import ABC, abstractmethod
from typing import Any, Dict, List, Optional
from agent.message_bus import MessageBus, Message
from agent.memory import SharedMemory
from agent.dynamic_tool_registry import DynamicToolRegistry

class BaseAgent(ABC):
    """所有 Agent 的抽象基类"""

    def __init__(
        self,
        name: str,
        config: Dict[str, Any],
        message_bus: MessageBus,
        shared_memory: SharedMemory,
        tool_registry: DynamicToolRegistry,  # 使用动态注册表
    ):
        self.name = name
        self.config = config
        self.message_bus = message_bus
        self.shared_memory = shared_memory
        self.tool_registry = tool_registry
        self.status = "idle"
        self.current_task: Optional[Task] = None

    @abstractmethod
    def handle_message(self, message: Message) -> Optional[Message]:
        """处理接收到的消息"""
        pass

    @abstractmethod
    def execute_task(self, task: "Task") -> "TaskResult":
        """执行分配的任务"""
        pass

    def send_message(self, receiver: str, msg_type: str, payload: Any) -> None:
        """发送消息到总线"""
        pass

    def _think(self, prompt: str) -> str:
        """调用LLM进行推理"""
        pass

    def _use_tool(self, tool_name: str, arguments: Dict) -> str:
        """调用工具（包括静态工具和动态工具）"""
        pass

    def _request_tool(self, requirement: Dict) -> str:
        """请求ToolSmith创建新工具，返回tool_id"""
        pass

    def _get_available_tools(self) -> List[Dict[str, Any]]:
        """获取当前可用工具列表（静态 + 动态）"""
        pass
```

#### ToolSmithAgent

```python
class ToolSmithAgent(BaseAgent):
    """工具锻造Agent —— 系统的扩展能力核心"""

    def __init__(self, sandbox: ToolSandbox, ...):
        super().__init__(name="toolsmith", ...)
        self.sandbox = sandbox

    def handle_message(self, message: Message) -> Optional[Message]:
        """处理工具创建/迭代请求"""
        if message.type == "tool_request":
            return self._handle_tool_request(message)
        elif message.type == "tool_iterate":
            return self._handle_tool_iteration(message)

    def _handle_tool_request(self, message: Message) -> Message:
        """
        完整的工具创建流程：
        1. 分析需求
        2. 生成代码
        3. 自我验证
        4. 沙箱验证
        5. 注册
        6. 通知
        """
        pass

    def generate_tool_code(self, requirement: Dict) -> Tuple[str, str, Dict]:
        """
        使用LLM生成工具代码
        返回: (code, language, function_config)
        """
        pass

    def self_validate(self, code: str, language: str) -> bool:
        """LLM自我审查生成的代码"""
        pass

    def iterate(self, tool_id: str, feedback: str) -> DynamicTool:
        """基于反馈改进工具"""
        pass
```

#### DynamicToolRegistry

```python
@dataclass
class DynamicTool:
    tool_id: str
    name: str
    description: str
    code: str
    language: str          # "python" | "bash"
    file_path: str
    function_config: Dict[str, Any]
    version: int = 1
    status: str = "active"  # active | deprecated | failed
    created_by: str = ""
    usage_count: int = 0
    success_count: int = 0

class DynamicToolRegistry:
    """运行时工具注册中心"""

    def __init__(self, tools_dir: str = "./generated_tools"):
        self.tools_dir = tools_dir
        self._tools: Dict[str, DynamicTool] = {}     # tool_id → tool
        self._name_to_id: Dict[str, str] = {}         # name → tool_id
        self._ensure_dir()

    def register(self, name, description, code, language,
                 function_config, created_by="") -> DynamicTool:
        """注册新工具到系统"""
        tool_id = self._generate_id()
        file_path = self._write_tool_file(tool_id, code, language)
        tool = DynamicTool(
            tool_id=tool_id, name=name, description=description,
            code=code, language=language, file_path=file_path,
            function_config=function_config, created_by=created_by
        )
        self._tools[tool_id] = tool
        self._name_to_id[name] = tool_id
        return tool

    def get_llm_function_configs(self) -> List[Dict[str, Any]]:
        """返回所有活跃工具的LLM函数配置"""
        return [
            t.function_config
            for t in self._tools.values()
            if t.status == "active"
        ]

    def execute(self, tool_name: str, arguments: Dict) -> str:
        """执行动态工具（通过对应语言的解释器）"""
        pass

    def record_usage(self, tool_id: str, success: bool) -> None:
        """更新工具使用统计"""
        pass

    def deprecate(self, tool_id: str, reason: str) -> None:
        """废弃工具"""
        pass

    def to_dict(self) -> Dict:
        """序列化用于检查点"""
        pass
```

#### ToolSandbox

```python
class ToolSandbox:
    """工具安全验证沙箱"""

    BANNED_PATTERNS = [
        r"os\.system\(.*rm\s+-rf\s+/",
        r"__import__\(.*os.*\).*system",
        r"fork\s*\(\s*\)",
        r"while\s+True\s*:",
        r"eval\s*\(.*__",
        r"exec\s*\(.*__",
    ]

    TIMEOUT_SECONDS = 60
    MAX_OUTPUT_BYTES = 1024 * 1024

    def validate(self, code: str, language: str) -> ValidationResult:
        """三步验证：静态分析 → 语法检查 → 试运行"""
        pass

    def _static_analysis(self, code: str) -> List[str]:
        """正则匹配危险模式"""
        pass

    def _syntax_check(self, code: str, language: str) -> bool:
        """语法检查"""
        pass

    def _dry_run(self, code: str, language: str) -> str:
        """隔离执行试运行"""
        pass
```

---

## 8. 配置设计

### 8.1 完整配置模板

```json
{
    "llm": {
        "model": "gpt-4o-mini",
        "api_key": "",
        "api_base": "https://api.openai.com/v1"
    },
    "agents": {
        "orchestrator": {
            "model": "gpt-4o",
            "max_tokens": 4096,
            "temperature": 0.3
        },
        "recon": {
            "model": "gpt-4o-mini",
            "max_tokens": 2048,
            "temperature": 0.5,
            "max_retries": 3
        },
        "exploit": {
            "model": "gpt-4o",
            "max_tokens": 4096,
            "temperature": 0.2
        },
        "analysis": {
            "model": "gpt-4o-mini",
            "max_tokens": 2048,
            "temperature": 0.1
        },
        "reflection": {
            "model": "gpt-4o",
            "max_tokens": 4096,
            "temperature": 0.3,
            "trigger_condition": "consecutive_failures >= 2"
        },
        "toolsmith": {
            "model": "gpt-4o",
            "max_tokens": 8192,
            "temperature": 0.2,
            "allowed_languages": ["python", "bash"],
            "max_tools_per_session": 10
        }
    },
    "orchestration": {
        "max_rounds": 30,
        "task_timeout_seconds": 300,
        "reflection_enabled": true,
        "parallel_execution": false,
        "default_mode": "orchestrated"
    },
    "tool_autonomy": {
        "enabled": true,
        "auto_create_threshold": "medium",
        "max_tools_per_session": 10,
        "require_user_approval": false,
        "persist_tools": false,
        "sandbox": {
            "timeout_seconds": 60,
            "max_output_bytes": 1048576,
            "banned_patterns": [
                "rm -rf /",
                "fork()",
                "while True:",
                "eval(__",
                "exec(__"
            ]
        }
    },
    "tool_config": {
        "bash_shell": {
            "shell_path": "bash",
            "working_dir": ".",
            "timeout": 30,
            "login_shell": false,
            "env": {}
        }
    },
    "platform": { ... }
}
```

### 8.2 自主性级别说明

`tool_autonomy.auto_create_threshold` 控制 Agent 创建工具的自主程度：

| 级别 | 说明 |
|------|------|
| `"low"` | 仅在 Agent 明确找不到可用工具时才创建，创建前需用户确认 |
| `"medium"` (推荐) | Agent 判断需要专用工具时自动创建，但数量有上限 |
| `"high"` | Agent 可自由创建工具，包括主动优化现有工具 |

---

## 9. 动态工具执行模型

### 9.1 执行流程

```
Agent调用工具
     │
     ▼
┌──────────────┐
│ 查找工具名    │
└──┬───────┬───┘
   │       │
   ▼       ▼
静态工具  动态工具
(BashShell) (DynamicTool)
   │       │
   │       ├── Python工具 → subprocess [python, tool_file, args_json]
   │       └── Bash工具   → subprocess [bash, tool_file, args...]
   │
   ▼
返回结果
```

### 9.2 动态工具包装器

```python
# ctf_tool/dynamic_tool_wrapper.py

class DynamicToolWrapper(BaseTool):
    """
    将 DynamicToolRegistry 中的工具包装为 BaseTool 接口，
    使动态工具能与静态工具通过统一的 execute() 接口调用
    """

    def __init__(self, registry: DynamicToolRegistry):
        self.registry = registry

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        tool = self.registry.get_by_name(tool_name)
        if tool is None:
            return f"错误：未找到动态工具 '{tool_name}'"

        if tool.language == "python":
            return self._execute_python(tool, arguments)
        elif tool.language == "bash":
            return self._execute_bash(tool, arguments)

    def _execute_python(self, tool: DynamicTool, args: Dict) -> str:
        """
        执行Python工具：
        python tool_file.py '{"arg1": "val1", ...}'
        工具脚本接收JSON参数，输出结果到stdout
        """
        pass

    def _execute_bash(self, tool: DynamicTool, args: Dict) -> str:
        """
        执行Bash工具：
        bash tool_file.sh arg1 arg2 ...
        """
        pass

    @property
    def function_config(self) -> Dict[str, Any]:
        """动态工具不使用此属性，配置由 Registry 提供"""
        return {}
```

### 9.3 动态工具代码规范

ToolSmith Agent 生成的工具需要遵循统一接口规范：

```python
# generated_tools/tool_xxxx_v1.py
"""
Dynamic tool: decrypt_custom_feistel
Created by: ToolSmith Agent
Purpose: 解密自定义Feistel加密的密文
"""

import sys
import json

def main():
    """入口函数：从stdin读取JSON参数，输出结果到stdout"""
    try:
        input_data = json.loads(sys.stdin.read())
        ciphertext = input_data["ciphertext"]
        key = input_data["key"]
        rounds = input_data.get("rounds", 16)

        result = decrypt(ciphertext, key, rounds)

        output = {
            "success": True,
            "plaintext": result
        }
    except Exception as e:
        output = {
            "success": False,
            "error": str(e)
        }

    print(json.dumps(output))


def decrypt(ciphertext: str, key: str, rounds: int) -> str:
    """解密逻辑"""
    # ... 实现代码 ...
    pass


if __name__ == "__main__":
    main()
```

---

## 10. 风险与应对

| 风险 | 影响 | 应对策略 |
|------|------|----------|
| **LLM 调用成本翻倍** | 多Agent + 工具生成增加API调用 | 侦察/分析用轻量模型；`max_rounds` 和 `max_tools_per_session` 限流；支持降级为单Agent模式 |
| **生成的工具不安全** | 恶意或危险代码执行 | ToolSandbox 三层验证；危险模式黑名单；沙箱子进程隔离；用户确认模式可选 |
| **Agent间信息丢失** | 传递中关键线索遗漏 | SharedMemory 统一存储关键发现；结构化消息格式；关键信息双写 |
| **工具爆炸** | 创建过多无用工具 | 工具使用统计 + 自动废弃低效工具；`max_tools_per_session` 限制；Reflection Agent 定期审计 |
| **协调Agent误判** | 错误的策略/任务分配 | Analysis Agent 验证策略有效性；Reflection Agent 兜底；用户可随时介入 |
| **生成的代码质量差** | 工具执行失败率高 | ToolSmith 自我验证 + Sandbox试运行；失败自动触发迭代；Execution Agent 有重试机制 |
| **并发竞态** | 并行时工具注册冲突 | 工具注册加锁；Phase 1-3 先串行再逐步放开 |
| **上下文膨胀** | 多Agent + 工具代码占用大量token | 工具代码按需注入上下文（不全部展示）；Memory 压缩机制 |

---

## 11. 实施路线

### Phase 1：基础设施搭建

- [ ] 新增 `BaseAgent` 抽象基类
- [ ] 新增 `MessageBus` 消息总线
- [ ] 新增 `DynamicToolRegistry` 动态工具注册中心
- [ ] 新增 `ToolSandbox` 安全验证沙箱
- [ ] 新增 `SharedMemory` 扩展记忆
- [ ] 新增 `Task` 和 `TaskQueue`
- [ ] 新增 `AgentRegistry`
- [ ] 扩展 `LLMRequest` 支持多模型独立配置
- [ ] 新增 `DynamicToolWrapper` 工具包装器
- [ ] 扩展 `config_template.json`
- [ ] 拆分 `prompt.yaml` 为 `prompts/` 目录

**验收标准**：
- DynamicToolRegistry 可注册/执行/废弃动态工具
- ToolSandbox 可拦截危险代码
- MessageBus 可正常收发

### Phase 2：ToolSmith Agent 独立验证

- [ ] 实现 `ToolSmithAgent`（不含多Agent协调）
- [ ] 实现代码生成 + 自我验证 + 沙箱验证完整流程
- [ ] 与现有 SolveAgent 集成：让单Agent能调用 ToolSmith 创建工具
- [ ] 验证端到端：给定一个需要专用工具的场景，ToolSmith 能生成并注册可用工具

**验收标准**：
- ToolSmith 能根据自然语言描述生成正确的 Python/Bash 工具
- 生成的工具通过沙箱验证后能被 SolveAgent 调用
- 工具效果可被 Analysis Agent 评估

### Phase 3：Orchestrator + 多Agent 协作

- [ ] 实现 `Orchestrator`
  - 题目分析、策略规划、任务分解
  - 工具缺口识别，自动请求 ToolSmith
- [ ] 实现 `ReconAgent`、`ExploitAgent`
- [ ] 将 `Analyzer` 升级为 `AnalysisAgent`
- [ ] 修改 `Workflow.solve()` 使用 Orchestrator
- [ ] 保持向后兼容：Legacy 单Agent模式可用

**验收标准**：
- Orchestrator 能自动识别工具缺口并触发 ToolSmith
- 多Agent协作完成完整解题流程
- 动态创建的工具在解题中实际发挥作用

### Phase 4：反思与迭代优化

- [ ] 实现 `ReflectionAgent`
- [ ] 工具使用效果追踪与自动迭代
- [ ] 工具版本管理与对比
- [ ] 支持多路径并行探索
- [ ] 性能优化和成本控制

**验收标准**：
- Reflection Agent 能在失败后提出有效改进建议
- 工具能基于使用反馈自动迭代
- 并行执行无竞态问题

---

## 12. 兼容性说明

### 12.1 向后兼容

- 保留 `agent/solve_agent.py` 作为 "Legacy Mode"
- 配置中 `orchestration.default_mode: "single"` 可回退到单Agent模式
- CLI 参数 `--mode single` 强制使用传统模式
- 现有检查点文件格式不变，新增字段通过可选参数兼容

### 12.2 配置迁移

`config_template.json` 新增 `agents`、`orchestration` 和 `tool_autonomy` 配置段。所有新增字段都有合理默认值，未配置时自动降级为单Agent静态工具模式。

---

## 13. 总结

本方案将当前的单Agent逐步循环模式重构为**具备自主工具创造能力的多Agent协作系统**：

**五个专业Agent：**
1. **Orchestrator** — 任务分解、策略调度、识别工具缺口
2. **Recon Agent** — 信息收集、侦察探测
3. **Exploit Agent** — 漏洞利用、Payload构造
4. **Analysis Agent** — 输出分析、Flag检测、工具效果评估
5. **Reflection Agent** — 失败复盘、策略建议、工具改进建议

**一个核心创新Agent：**
6. **ToolSmith Agent** — ★ 根据题目需求自主生成专用工具

**核心基础设施：**
- **DynamicToolRegistry** — 运行时工具注册、发现、版本管理
- **ToolSandbox** — 三层安全验证（静态分析 + 语法检查 + 试运行）
- **MessageBus** — Agent间统一通信
- **SharedMemory** — 全局与私有双层记忆

**核心能力提升：**
- 从"使用固定工具集" → "根据题目自主创造工具"
- 从"被动执行" → "主动扩展自身能力边界"
- 从"单一路径串行" → "可并行多路径探索"
- 从"人工反馈纠错" → "自动反思迭代优化"
