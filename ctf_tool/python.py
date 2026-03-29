"""@brief 提供本地/远程 Python 代码执行工具。"""

import logging
import os
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, Optional, Tuple

import paramiko

from config import Config
from ctf_tool.base_tool import BaseTool

logger = logging.getLogger(__name__)


class PythonTool(BaseTool):
    """@brief Python 代码执行工具。"""

    def __init__(self, tool_config: Optional[Dict[str, Any]] = None):
        """@brief 初始化 Python 工具。

        @param tool_config 工具配置（当前实现兼容保留参数）。
        """
        tool_config = tool_config or {}
        _ = tool_config

        try:
            python_config = Config.get_tool_config("python")
        except (KeyError, ValueError):
            python_config = {}

        self.remote = python_config.get("remote", False)
        self.ssh_client: Optional[paramiko.SSHClient] = None

        if self.remote:
            ssh_config: Dict[str, Any] = Config.get_tool_config("python").get("ssh", {})
            self.hostname = ssh_config.get("host", "")
            self.port = ssh_config.get("port", 22)
            self.username = ssh_config.get("username")
            self.password = ssh_config.get("password")
            self._connect()

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        """@brief 执行 Python 代码。

        @param tool_name 工具名（当前实现不直接使用）。
        @param arguments 工具参数，需包含 content。
        @return str 执行输出。
        """
        content_value = arguments.get("content", "")
        content = content_value if isinstance(content_value, str) else str(content_value)
        if self.remote:
            return self._execute_remotely(content)

        return self._execute_locally(content)

    def _execute_locally(self, content: str) -> str:
        """@brief 在本地执行 Python 代码。

        @param content 代码内容。
        @return str 标准输出与标准错误拼接结果。
        """
        try:
            with tempfile.NamedTemporaryFile(suffix=".py", delete=False) as temp_file:
                temp_file.write(content.encode("utf-8"))
                temp_path = temp_file.name

            result = subprocess.run(
                [sys.executable, temp_path],
                capture_output=True,
                text=True,
                timeout=30,
            )
            os.unlink(temp_path)
            return result.stdout + result.stderr
        except Exception as error:
            return str(error)

    def _execute_remotely(self, content: str) -> str:
        """@brief 在远程主机执行 Python 代码。

        @param content 代码内容。
        @return str 标准输出与标准错误拼接结果。
        """
        temp_name = f"py_script_{int(time.time())}.py"
        upload_cmd = f"cat > {temp_name} << 'EOF'\n{content}\nEOF"

        self._shell_execute({"content": upload_cmd})
        stdout, stderr = self._shell_execute({"content": f"python3 {temp_name}"})
        self._shell_execute({"content": f"rm -f {temp_name}"})

        return stdout + stderr

    def _is_connected(self) -> bool:
        """@brief 检查 SSH 连接是否有效。

        @return bool 连接是否有效。
        """
        if not self.ssh_client:
            return False

        try:
            transport = self.ssh_client.get_transport()
            return bool(transport and transport.is_active())
        except Exception:
            return False

    def _connect(self) -> None:
        """@brief 建立或重建 SSH 连接。

        @raises ConnectionError 连接失败时抛出。
        """
        try:
            if self.ssh_client:
                self.ssh_client.close()

            client = paramiko.SSHClient()
            client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
            client.connect(
                hostname=self.hostname,
                port=self.port,
                username=self.username,
                password=self.password,
                timeout=10,
            )
            self.ssh_client = client
            logger.info("SSH连接成功: %s@%s:%s", self.username, self.hostname, self.port)
        except Exception as error:
            logger.error("SSH连接失败: %s", str(error))
            raise ConnectionError(f"SSH连接失败: {str(error)}") from error

    def _shell_execute(self, arguments: Dict[str, Any]) -> Tuple[str, str]:
        """@brief 通过 SSH 执行 Shell 命令。

        @param arguments 命令参数，需包含 content。
        @return Tuple[str, str] 标准输出与标准错误。
        """
        if not self._is_connected():
            logger.warning("SSH会话断开，尝试重新连接...")
            self._connect()

        command = arguments.get("content", "")
        if not command:
            return "", "错误：未提供命令内容"

        try:
            assert self.ssh_client is not None, "SSH客户端未初始化"
            _, stdout, stderr = self.ssh_client.exec_command(command)

            stdout_bytes = stdout.read()
            stderr_bytes = stderr.read()

            def safe_decode(data: bytes) -> str:
                """@brief 安全解码字节流。

                @param data 待解码字节数据。
                @return str 解码后的字符串。
                """
                try:
                    return data.decode("utf-8")
                except UnicodeDecodeError:
                    return data.decode("utf-8", errors="replace")

            return safe_decode(stdout_bytes), safe_decode(stderr_bytes)
        except Exception as error:
            logger.error("命令执行失败: %s", str(error))
            return "", f"命令执行错误: {str(error)}"

    @property
    def function_config(self) -> Dict[str, Any]:
        """@brief 返回工具函数配置。

        @return Dict[str, Any] 函数配置字典。
        """
        return {
            "type": "function",
            "function": {
                "name": "execute_python_code",
                "description": "执行Python代码片段",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "要执行的Python代码",
                        }
                    },
                    "required": ["content"],
                },
            },
        }
