# SSH/Paramiko 与 Python 工具移除，统一为本地 Bash 执行 设计文档

## 1. 背景

当前项目里有两条执行链：

1. [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)
   通过 SSH 在远程 Linux 主机执行命令。
2. [`ctf_tool/python.py`](../ctf_tool/python.py)
   提供 `execute_python_code` 工具，本地模式下用 `subprocess` 跑临时代码文件，远程模式下仍然依赖 SSH。

这会带来几个实际问题：

1. 执行模型不统一。
   Shell 走远程，Python 既可能本地也可能远程，Agent 的工具语义不够稳定。
2. `paramiko` 依赖无法彻底清掉。
   即使把 `ssh_shell` 替换掉，只要保留 [`ctf_tool/python.py`](../ctf_tool/python.py) 的远程分支，项目里仍然要保留 `paramiko`。
3. 工具集重叠。
   很多原本通过 `execute_python_code` 做的事情，也可以通过本地 Bash 直接调用系统 Python 完成。
4. 维护成本偏高。
   两套执行工具都要维护配置、错误处理、依赖和文档。

基于你的新要求，本次设计目标不再是“把 `ssh_shell` 换成本地 Bash”，而是更进一步：

1. 完全移除 SSH/`paramiko` 依赖。
2. 删除独立的 Python 工具 [`ctf_tool/python.py`](../ctf_tool/python.py)。
3. 统一为一个本地 Bash 执行工具。

## 2. 改造目标

### 2.1 目标状态

改造完成后，项目的本地工具层只保留一个执行入口：

1. `execute_shell_command`

由新的 [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py) 提供，底层使用本地 Bash + `subprocess` 执行。

### 2.2 必须满足的目标

1. 删除 [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)。
2. 删除 [`ctf_tool/python.py`](../ctf_tool/python.py)。
3. 从代码和依赖中移除 `paramiko`。
4. `SolveAgent` 主流程尽量不改。
5. `function_config` 对外只暴露 `execute_shell_command`。
6. `config_template.json` 不再出现 `ssh_shell` 或 `python` 配置。

### 2.3 非目标

1. 本次不做复杂命令沙箱。
2. 本次不引入新的“专用 Python Runner”替代品。
3. 本次不做 WSL 路径映射兼容。

## 3. 现状分析

### 3.1 `ssh_shell.py` 当前职责

[`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py) 当前负责：

1. 读取 `tool_config.ssh_shell`。
2. 建立 SSH 连接。
3. 自动把 `./attachments` 上传到远程环境。
4. 通过 `paramiko.exec_command()` 执行命令。
5. 返回 `stdout + stderr`。

这意味着它不仅是“执行器”，还承担了连接管理和文件同步副作用。

### 3.2 `python.py` 当前职责

[`ctf_tool/python.py`](../ctf_tool/python.py) 当前提供 `execute_python_code`：

1. 本地模式下用 `subprocess` 调用本机 Python。
2. 远程模式下读取 `tool_config.python.ssh`，仍然通过 SSH 执行。

也就是说，`python.py` 是当前项目里另一条残留的 SSH 使用路径。如果目标是“完全移除 SSH/paramiko”，那这个工具不能保留。

### 3.3 工具加载机制的影响

[`utils/tools.py`](../utils/tools.py) 当前会扫描 `ctf_tool/` 目录并自动加载继承 `BaseTool` 的类，加载逻辑并没有真正依赖配置做启停控制。

这意味着：

1. 只要 [`ctf_tool/python.py`](../ctf_tool/python.py) 文件还在，`execute_python_code` 就仍然会暴露给 Agent。
2. 只要 [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py) 文件还在，它就仍可能被实例化。

所以本次不能只“停用配置”，必须直接删除对应工具文件，或者至少保证它们不再被 `ToolUtils.load_tools()` 扫描到。考虑到你的要求是彻底移除，最干净的方案就是直接删除文件。

### 3.4 对 Agent 主流程的影响

[`agent/solve_agent.py`](../agent/solve_agent.py) 并不关心工具的底层实现，它只依赖两件事：

1. 工具的 `function_config`
2. 工具的 `execute()` 返回文本

因此，只要保留 `execute_shell_command` 这个函数名，`SolveAgent` 主流程基本不需要改。

### 3.5 对 Prompt 的影响

[`prompt.yaml`](../prompt.yaml) 当前没有写死 `execute_python_code` 或 `execute_shell_command` 的名称，主要依赖运行时传入的工具列表。

这意味着：

1. 删除 Python 工具后，不需要强制修改 Prompt 才能运行。
2. 但为了让模型更稳定地理解新工具职责，建议同步优化 `execute_shell_command` 的描述文案。

## 4. 核心决策

### 4.1 最终工具集

改造后只保留一个本地执行工具：

1. [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py)

改造后删除两个旧工具：

1. [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)
2. [`ctf_tool/python.py`](../ctf_tool/python.py)

### 4.2 对外函数名

保留：

1. `execute_shell_command`

删除：

1. `execute_python_code`

这样做的原因是：

1. `SolveAgent` 当前对工具函数名透明，保留 Shell 函数名即可最小化改动。
2. Python 工具删除后，避免给模型两个高度重叠的执行入口。
3. 工具选择空间更小，模型更不容易在 Shell 和 Python 之间摇摆。

### 4.3 对 Python 能力的处理原则

删除独立 Python 工具并不代表“项目不能执行 Python 代码”，而是改成统一通过 Shell 触发：

1. 简单脚本可使用 `python -c "..."`
2. 多行脚本可使用 heredoc，例如 `python - <<'PY' ... PY`
3. 如果本机没有 Python，可由命令失败信息反馈给 Agent

也就是说，Python 不再是一个“工具级能力”，而是本地 Bash 环境中的一个“可选命令”。

这是本次方案的重要取舍：

1. 好处是执行模型统一、依赖更少、维护更简单。
2. 代价是模型编写复杂 Python 片段时，提示和命令格式会比专用 Python 工具更脆弱。

基于你当前的要求，这个取舍是合理的。

## 5. 推荐设计

### 5.1 新工具接口

新的 [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py) 继续暴露：

```json
{
  "type": "function",
  "function": {
    "name": "execute_shell_command",
    "description": "在本地 Bash 环境执行 Shell 命令，可使用系统已安装的 curl、nmap、openssl、python 等命令",
    "parameters": {
      "type": "object",
      "properties": {
        "content": {
          "type": "string",
          "description": "要执行的 Bash 命令"
        }
      },
      "required": ["content"]
    }
  }
}
```

这里建议把描述写清楚：

1. 是“本地 Bash”
2. 依赖“系统已安装命令”
3. 如本机安装了 Python，也允许通过 Shell 间接执行 Python

### 5.2 底层执行方式

推荐使用：

```python
subprocess.run(
    [shell_path, "-lc", command] if login_shell else [shell_path, "-c", command],
    cwd=working_dir,
    env=merged_env,
    capture_output=True,
    text=True,
    timeout=timeout,
)
```

关键设计点：

1. 不使用 `shell=True`
   由 Bash 进程显式解释命令，语义更可控。
2. 由配置显式指定 `shell_path`
   不依赖 PowerShell/cmd 默认行为。
3. 保留 `cwd`
   确保命令能直接访问项目目录和 `attachments/`。
4. 保留超时
   避免命令长期阻塞 Agent。

### 5.3 推荐类结构

建议新建 [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py)：

```python
class BashShell(BaseTool):
    """本地 Bash 执行工具"""

    config_key = "bash_shell"

    def __init__(self) -> None:
        config = Config.get_tool_config(self.config_key)
        self.shell_path = config.get("shell_path", "bash")
        self.working_dir = config.get("working_dir", ".")
        self.timeout = config.get("timeout", 30)
        self.login_shell = config.get("login_shell", False)
        self.extra_env = config.get("env", {})
        self._validate_shell()

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        command = self._normalize_command(arguments)
        return self._run_command(command)
```

### 5.4 输出格式建议

不建议继续简单返回 `stdout + stderr`，因为删除 Python 工具后，Shell 会承担更多复杂执行任务，返回码更重要。

推荐统一返回：

```text
[exit_code] 0
[stdout]
...

[stderr]
...
```

这样 Agent 至少可以：

1. 判断命令是否真正执行成功
2. 区分“无输出但成功”和“失败但错误在 stderr”
3. 在 Bash 调用 Python 失败时，拿到更完整的错误上下文

### 5.5 附件处理

既然完全去掉远程执行，就不再需要附件上传逻辑。

处理原则改为：

1. Bash 的工作目录默认设为项目根目录。
2. 命令直接访问 `./attachments`。
3. [`agent/workflow.py`](../agent/workflow.py) 现有的附件文件名补充逻辑保留。

### 5.6 Bash 路径兼容策略

第一阶段建议只支持“可直接执行的 Bash”：

1. Linux / macOS 的 `bash`
2. Windows 上 Git Bash 的 `bash.exe`

本阶段不处理：

1. WSL 路径自动转换
2. MSYS/MinGW 特殊路径映射
3. 多 Shell 自动探测

原因很简单：当前目标是“统一执行模型并删除依赖”，不是做一个跨平台 Shell 抽象层。

## 6. 配置设计

### 6.1 新配置结构

[`config_template.json`](../config_template.json) 建议从：

```json
"tool_config": {
    "ssh_shell": {
        "host": "127.0.0.1",
        "port": 22,
        "username": "",
        "password": ""
    },
    "python": {}
}
```

调整为：

```json
"tool_config": {
    "bash_shell": {
        "shell_path": "bash",
        "working_dir": ".",
        "timeout": 30,
        "login_shell": false,
        "env": {}
    }
}
```

### 6.2 迁移策略

这次不建议保留旧配置兼容分支。

也就是说：

1. 不再读取 `tool_config.ssh_shell`
2. 不再读取 `tool_config.python`
3. 新代码只读取 `tool_config.bash_shell`

原因：

1. 你的目标是“完全移除 SSH/paramiko 依赖”，兼容旧键只会拖延清理。
2. 项目当前仍处于快速演进阶段，没必要为旧配置背长期包袱。
3. 工具加载本来就靠扫描文件，删除旧工具后继续兼容旧配置意义不大。

注意：

1. 按 AGENTS 约束，不应自动修改用户现有的 [`config.json`](../config.json)。
2. 这意味着代码改造完成后，需要用户手动把自己的 `config.json` 从旧键迁移到 `bash_shell`。

## 7. 代码改动点

### 7.1 必改文件

1. 新增 [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py)
2. 删除 [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)
3. 删除 [`ctf_tool/python.py`](../ctf_tool/python.py)
4. 更新 [`config_template.json`](../config_template.json)
5. 更新 [`README.md`](../README.md)
6. 更新 [`requirements.txt`](../requirements.txt)
7. 更新 [`main.py`](../main.py)

### 7.2 各文件修改说明

[`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py)：

1. 新增本地 Bash 工具实现
2. 提供 `execute_shell_command`
3. 负责 Bash 路径校验、超时控制和输出格式化

[`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)：

1. 直接删除
2. 不保留兼容壳文件

[`ctf_tool/python.py`](../ctf_tool/python.py)：

1. 直接删除
2. 不再暴露 `execute_python_code`

[`config_template.json`](../config_template.json)：

1. 删除 `ssh_shell`
2. 删除 `python`
3. 只保留 `bash_shell`

[`README.md`](../README.md)：

1. 删除“支持 Python 工具和 SSH 到 Linux 机器”之类表述
2. 改为“通过本地 Bash 环境执行命令”
3. 说明如需 Python，可通过本地 Shell 调用系统 Python

[`requirements.txt`](../requirements.txt)：

1. 必删 `paramiko`
2. 审核是否还需要保留其相关依赖

[`main.py`](../main.py)：

1. 删除 `logging.getLogger("paramiko").setLevel(logging.WARNING)`

### 7.3 关于 `requirements.txt` 的进一步说明

`paramiko` 删除后，以下库可能也可清理，但应先确认没有其他依赖在使用：

1. `bcrypt`
2. `cryptography`
3. `PyNaCl`
4. `cffi`
5. `pycparser`

这里建议分两步做：

1. 先移除明确的直接依赖 `paramiko`
2. 再根据实际安装验证结果，决定是否继续删这些关联库

这样更稳妥。

## 8. `utils/tools.py` 的建议修正

[`utils/tools.py`](../utils/tools.py) 当前这段逻辑：

```python
if name in config.get("tool_config", {}):
    tool_instance = obj()
else:
    tool_instance = obj()
```

实际上没有起到任何配置控制作用。

在本次“只保留一个本地工具”的改造下，这个问题的紧迫性会下降，因为旧工具已经被删除了，不再存在重复暴露的问题。

但仍建议顺手修正，原因是：

1. 未来再加工具时还会踩同样的问题。
2. 更清晰的工具启停逻辑有利于维护。

推荐方案：

1. 给工具类增加 `config_key`
2. `load_tools()` 按 `config_key` 判断是否实例化
3. 对缺失配置给出更清晰日志

不过从本次目标来看，这属于“建议一起做”，不是“为了移除 SSH/paramiko 必须做”。

## 9. 风险与取舍

### 9.1 风险：删除 Python 工具后，复杂脚本生成更脆弱

以前模型可以直接把多行 Python 代码作为 `execute_python_code` 的参数传进去；删除后只能通过 Shell 包装。

影响：

1. heredoc 或引号转义更容易出错
2. 多行代码的可读性会下降
3. 某些复杂脚本执行的成功率可能略有下降

应对：

1. 在 `execute_shell_command` 描述里明确“可调用系统 Python”
2. 返回完整 `stderr` 和 `exit_code`
3. 后续如果确实发现痛点，再考虑增加新的本地代码执行器，但那将是一个全新设计，不再和 SSH/paramiko 绑定

### 9.2 风险：本地环境没有 Python

删除 Python 工具后，Shell 调 Python 完全依赖用户本地环境。

应对：

1. README 中明确说明 Python 不是独立工具，而是 Bash 环境中的可选命令
2. 如果用户机器没有 `python` 或 `python3`，Agent 会从错误输出中得到反馈

### 9.3 风险：本地命令直接作用于宿主机

统一成本地执行后，所有命令都直接落到用户机器。

应对：

1. 继续依赖现有自动/手动模式
2. 文档明确提示用户谨慎使用自动模式
3. 如后续需要，再增加命令黑名单或目录白名单

### 9.4 风险：Windows Bash 路径问题

项目当前运行环境可能是 Windows + PowerShell + venv，实际执行器却是 Bash。

应对：

1. 由配置显式指定 `shell_path`
2. 第一阶段推荐 Git Bash
3. 不在本次方案里做自动探测和路径适配

## 10. 实施顺序建议

建议按以下顺序实施：

1. 新增 [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py)
2. 删除 [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)
3. 删除 [`ctf_tool/python.py`](../ctf_tool/python.py)
4. 更新 [`config_template.json`](../config_template.json)
5. 更新 [`README.md`](../README.md)
6. 更新 [`requirements.txt`](../requirements.txt)
7. 更新 [`main.py`](../main.py)
8. 可选：修正 [`utils/tools.py`](../utils/tools.py) 的加载逻辑

## 11. 验收建议

由于项目当前没有标准测试框架，本次建议至少做以下 smoke test：

1. 启动程序，确认工具列表中只有 `execute_shell_command`
2. 确认 `execute_python_code` 不再暴露
3. 执行 `echo hello`
4. 执行 `pwd`
5. 执行 `ls ./attachments`
6. 执行一个不存在的命令，确认错误信息能返回
7. 执行一个超时命令，确认超时能被中断
8. 如果本地安装了 Python，通过 Bash 执行 `python -c "print(1)"` 或 `python3 -c "print(1)"`

额外的清理验收：

1. `rg -n -F "paramiko" .` 不应再命中项目源码和依赖文件
2. `rg -n -F "execute_python_code" .` 不应再命中运行代码
3. `requirements.txt` 中不应再出现 `paramiko`

## 12. 最终结论

如果目标是“完全移除 SSH/paramiko 依赖”，那么只替换 [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py) 还不够，必须同时删除 [`ctf_tool/python.py`](../ctf_tool/python.py)，因为它仍然保留了 SSH 分支和独立执行模型。

因此，推荐的最终方案是：

1. 新增 [`ctf_tool/bash_shell.py`](../ctf_tool/bash_shell.py)
2. 删除 [`ctf_tool/ssh_shell.py`](../ctf_tool/ssh_shell.py)
3. 删除 [`ctf_tool/python.py`](../ctf_tool/python.py)
4. 删除 `paramiko` 及相关残留配置
5. 对外只保留 `execute_shell_command`
6. 如需运行 Python，统一通过本地 Bash 调用系统 Python

这样做的结果是：

1. 执行模型更统一
2. 依赖更干净
3. 工具选择更简单
4. 维护成本更低

代价则是失去一个专用 Python 工具，但在你当前“彻底本地化、彻底去 SSH”这个目标下，这是合理且一致的取舍。
