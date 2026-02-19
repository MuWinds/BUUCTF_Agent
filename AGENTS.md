# AGENTS.md

## Commands

项目提供两种运行模式：
conda环境为buuctf_agent
```bash
conda activate buuctf_agent
# 命令行模式 — 读取 question.txt 中的题目，交互式解题
python main.py

# Web 界面模式 — 启动 Flask 服务，默认端口 5000
python webui/app.py
```

## Testing

项目目前没有使用标准测试框架（pytest 等），仅有手动测试脚本：

- `test_rag.py` — RAG 知识库系统的功能验证脚本

运行方式：

```bash
python test_rag.py
```

如需添加测试，建议使用 pytest，放置在项目根目录或 `tests/` 目录下。

## Project Structure

```
BUUCTF_Agent/
├── agent/                    # Agent 核心模块
│   ├── analyzer.py          # 步骤输出分析器（Flag 检测、终止判断）
│   ├── checkpoint.py        # 存档管理（自动保存/恢复解题进度）
│   ├── memory.py            # 记忆系统（历史压缩、关键事实提取）
│   ├── solve_agent.py       # 解题 Agent 主逻辑（思考、工具调用、反思）
│   └── workflow.py          # 工作流控制（题目摘要、协调执行、Flag 确认）
├── ctf_tool/                # CTF 工具模块
│   ├── base_tool.py         # 工具抽象基类
│   ├── mcp_adapter.py       # MCP 服务器适配器（异步）
│   ├── python.py            # Python 代码执行工具
│   └── ssh_shell.py         # SSH Shell 工具（paramiko）
├── rag/                     # RAG 知识库系统
│   ├── knowledge_base.py    # 知识库管理（chromadb 向量存储）
│   ├── memory_base.py       # 记忆基类
│   └── rag_service.py       # RAG 服务基类
├── utils/                   # 工具函数
│   ├── llm_request.py       # LLM 请求封装（litellm）
│   ├── text.py              # 文本处理
│   ├── tools.py             # 工具加载和管理
│   └── user_interface.py    # 用户交互接口（抽象类）
├── webui/                   # Web 界面
│   ├── app.py               # Flask 应用入口
│   ├── static/              # 静态资源
│   └── templates/           # HTML 模板
├── design_md/               # 设计文档
├── attachments/             # 题目附件目录
├── logs/                    # 日志输出（DEBUG 级别）
├── rag_db/                  # RAG 向量数据库存储
├── checkpoints/             # 解题存档目录
├── main.py                  # 命令行入口
├── config.json              # 运行配置（从 config_template.json 复制）
├── config_template.json     # 配置模板
├── config.py                # 配置加载器
├── prompt.yaml              # Prompt 模板（摘要/思考/分析/反思）
├── requirements.txt         # Python 依赖
├── Dockerfile               # Docker 配置
└── README.md                # 项目说明
```

核心数据流：`main.py` → `Workflow` → `SolveAgent`（思考+工具调用循环） → `Analyzer`（输出分析） → `Memory`（记忆管理）

## Code Style

- 语言：Python 3.8+（推荐 3.10+）
- 缩进：4 个空格
- 编码：UTF-8
- 类名：PascalCase（`SolveAgent`, `CheckpointManager`）
- 函数/变量：snake_case（`load_config()`, `max_history_steps`）
- 私有方法：前缀下划线（`_connect()`, `_execute()`）
- 注释语言：中文为主
- 文档字符串：三引号，描述功能、参数、返回值
- 导入顺序：标准库 → 第三方库 → 本地模块
- 日志：使用 Python 标准 `logging` 模块，文件 DEBUG / 控制台 INFO

示例：

```python
class SolveAgent:
    """解题 Agent，负责思考和工具调用"""

    def __init__(self, config: dict, tools: list):
        self._config = config
        self._tools = tools
        self._memory = Memory(config)

    def solve(self, problem: str) -> str:
        """执行解题流程"""
        # 生成下一步思考
        thought = self._think_next(problem)
        return thought

    def _think_next(self, context: str) -> str:
        """内部方法：生成下一步推理"""
        pass
```

## Git Workflow

分支：基于 `main` 分支开发。

提交消息格式：`<类型>: <中文描述>`

类型前缀：
- `feat:` / `Feat:` — 新功能
- `fix:` — 修复 bug
- `Update:` — 更新/改进已有功能

示例：
```
fix: 健壮读取配置并统一执行输出为单字符串
Feat: 添加存档管理功能，支持从存档恢复解题状态
Update: 修改Python执行的配置文件
```

每个功能点独立提交，消息使用中文简洁描述改动内容。

## Boundaries

允许的操作：
- 修改 `agent/`、`ctf_tool/`、`rag/`、`utils/`、`webui/` 下的源代码
- 修改 `prompt.yaml` 中的 Prompt 模板
- 修改 `config_template.json` 配置模板
- 添加新的工具类（继承 `BaseTool`）
- 添加新的设计文档到 `design_md/`

禁止的操作：
- 不要修改用户的 `config.json`（包含 API 密钥等敏感信息）
- 不要删除 `checkpoints/`、`logs/`、`rag_db/` 中的运行时数据
- 不要删除 `attachments/` 中的题目附件
- 不要在代码中硬编码 API 密钥、密码等敏感信息
- 不要修改 `Dockerfile` 中的基础镜像和安全相关配置，除非明确要求
