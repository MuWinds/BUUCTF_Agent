"""提供本地 Bash 命令执行工具。"""

import logging
import os
import shutil
import subprocess
from typing import Any, Dict, Optional

from config import Config
from ctf_tool.base_tool import BaseTool

logger = logging.getLogger(__name__)


class BashShell(BaseTool):
    """在本地 Bash 环境执行 Shell 命令。"""

    def __init__(self) -> None:
        """初始化 Bash 工具配置。"""
        try:
            shell_config: Dict[str, Any] = Config.get_tool_config("bash_shell")
        except (KeyError, ValueError):
            shell_config = {}
            logger.warning("未读取到 bash_shell 配置，使用默认值")

        shell_path_value = shell_config.get("shell_path", "bash")
        self.shell_path = (
            shell_path_value if isinstance(shell_path_value, str) else "bash"
        )

        working_dir_value = shell_config.get("working_dir", ".")
        self.working_dir = (
            working_dir_value if isinstance(working_dir_value, str) else "."
        )

        timeout_value = shell_config.get("timeout", 30)
        self.timeout = timeout_value if isinstance(timeout_value, int) else 30

        login_shell_value = shell_config.get("login_shell", False)
        self.login_shell = (
            login_shell_value if isinstance(login_shell_value, bool) else False
        )

        env_value = shell_config.get("env", {})
        self.extra_env = env_value if isinstance(env_value, dict) else {}

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        """执行本地 Bash 命令。

        Args:
            tool_name: 工具名（当前实现不直接使用）。
            arguments: 参数字典，需包含 content。

        Returns:
            执行结果文本。
        """
        del tool_name

        command_value = arguments.get("content", "")
        command = command_value if isinstance(command_value, str) else str(command_value)
        if not command.strip():
            return "错误：未提供命令内容"

        shell_exec = self._resolve_shell_executable()
        if shell_exec is None:
            return (
                "错误：未找到可执行的 Bash。请在 config.json 的 tool_config.bash_shell.shell_path 中配置 Bash 路径。"
            )

        cwd = os.path.abspath(self.working_dir)
        env = os.environ.copy()
        for key, value in self.extra_env.items():
            env[str(key)] = str(value)

        bash_args = [shell_exec, "-lc", command] if self.login_shell else [
            shell_exec,
            "-c",
            command,
        ]

        try:
            result = subprocess.run(
                bash_args,
                cwd=cwd,
                env=env,
                capture_output=True,
                text=True,
                timeout=self.timeout,
            )
            stdout_text = result.stdout or ""
            stderr_text = result.stderr or ""
            return (
                f"[exit_code] {result.returncode}\n"
                f"[stdout]\n{stdout_text}\n"
                f"[stderr]\n{stderr_text}"
            )
        except subprocess.TimeoutExpired as error:
            stdout_text = error.stdout if isinstance(error.stdout, str) else ""
            stderr_text = error.stderr if isinstance(error.stderr, str) else ""
            return (
                f"命令执行超时（{self.timeout}秒）\n"
                f"[stdout]\n{stdout_text}\n"
                f"[stderr]\n{stderr_text}"
            )
        except Exception as error:
            logger.error("本地 Bash 命令执行失败: %s", error)
            return f"命令执行错误: {str(error)}"

    def _resolve_shell_executable(self) -> Optional[str]:
        """解析并校验 Bash 可执行路径。

        Returns:
            可执行的 Bash 路径，未找到时返回 None。
        """
        if os.path.isabs(self.shell_path):
            return self.shell_path if os.path.isfile(self.shell_path) else None

        # Windows 下常见误配：/bin/bash 会触发 WSL 路径解析，优先改用 Git Bash。
        if os.name == "nt" and self.shell_path.strip() == "/bin/bash":
            git_bash = self._find_git_bash()
            if git_bash:
                logger.info("检测到 Windows 环境，将 /bin/bash 自动映射到 Git Bash: %s", git_bash)
                return git_bash
            return None

        # 优先使用 PATH 中可解析到的 bash。
        located_path = shutil.which(self.shell_path)
        return located_path

    @staticmethod
    def _find_git_bash() -> Optional[str]:
        """在 Windows 常见目录中查找 Git Bash。

        Returns:
            Git Bash 可执行文件路径，未找到时返回 None。
        """
        candidates = [
            r"C:\Program Files\Git\bin\bash.exe",
            r"C:\Program Files\Git\usr\bin\bash.exe",
            r"C:\Program Files (x86)\Git\bin\bash.exe",
            r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
        ]
        for candidate in candidates:
            if os.path.isfile(candidate):
                return candidate

        return None

    @property
    def function_config(self) -> Dict[str, Any]:
        """返回工具函数配置。

        Returns:
            函数调用配置字典。
        """
        return {
            "type": "function",
            "function": {
                "name": "execute_shell_command",
                "description": (
                    "在本地Bash环境执行Shell命令"
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "要执行的Bash命令",
                        }
                    },
                    "required": ["content"],
                },
            },
        }
