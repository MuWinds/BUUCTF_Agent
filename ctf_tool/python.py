import paramiko
import subprocess
import sys
import tempfile
import os
import time
import logging
from typing import Tuple, Dict, Optional
from ctf_tool.base_tool import BaseTool
from config import Config

logger = logging.getLogger(__name__)


class PythonTool(BaseTool):
    def __init__(self, tool_config: Optional[Dict] = None):
        tool_config = tool_config or {}
        # 在初始化时询问是否要远程执行
        self.remote = self.ask_remote_execution(tool_config.get("default_mode"))
        if self.remote:
            ssh_config: dict = Config.get_tool_config("ssh_shell")
            self.hostname = ssh_config.get("host")
            self.port = ssh_config.get("port", 22)
            self.username = ssh_config.get("username")
            self.password = ssh_config.get("password")
            self.ssh_client = None
            self._connect()

    def ask_remote_execution(self, default_mode: Optional[str] = None) -> bool:
        """询问用户是否要远程执行，在非交互环境下遵循预设。"""
        env_mode = os.getenv("AGENT_PYTHON_MODE")
        if env_mode in {"local", "remote"}:
            return env_mode == "remote"

        if default_mode in {"local", "remote"}:
            return default_mode == "remote"

        if os.getenv("AGENT_NON_INTERACTIVE") == "1":
            return False

        print("\n--- Python 执行选项 ---")
        print("1. 本地执行")
        print("2. 远程执行")
        choice = input("请选择 Python 代码的执行方式 (1/2): ").strip()

        return choice == "2"

    def execute(self, tool_name: str, arguments: dict) -> Tuple[str, str]:
        """执行Python代码"""
        content = arguments.get("content", "")

        if self.remote:
            return self._execute_remotely(content)
        return self._execute_locally(content)

    def _execute_locally(self, content: str) -> Tuple[str, str]:
        try:
            with tempfile.NamedTemporaryFile(suffix=".py", delete=False) as tmp:
                tmp.write(content.encode("utf-8"))
                tmp_path = tmp.name

            result = subprocess.run(
                [sys.executable, tmp_path], capture_output=True, text=True, timeout=30
            )
            os.unlink(tmp_path)
            return result.stdout, result.stderr
        except Exception as e:
            return "", str(e)

    def _execute_remotely(self, content: str) -> Tuple[str, str]:
        temp_name = f"py_script_{int(time.time())}.py"

        # 修复：使用字典参数调用execute方法
        upload_cmd = f"cat > {temp_name} << 'EOF'\n{content}\nEOF"
        self._shell_execute({"content": upload_cmd})
        stdout, stderr = self._shell_execute({"content": f"python3 {temp_name}"})
        self._shell_execute({"content": f"rm -f {temp_name}"})

        return stdout, stderr

    def _is_connected(self):
        """检查连接是否有效"""
        if not self.ssh_client:
            return False
        try:
            transport = self.ssh_client.get_transport()
            return transport and transport.is_active()
        except Exception:
            return False

    def _connect(self):
        """建立SSH连接或重连"""
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
            logger.info(f"SSH连接成功: {self.username}@{self.hostname}:{self.port}")
        except Exception as e:
            logger.error(f"SSH连接失败: {str(e)}")
            raise ConnectionError(f"SSH连接失败: {str(e)}")

    def _shell_execute(self, arguments: dict):
        # 检查连接状态，自动重连
        if not self._is_connected():
            logger.warning("SSH会话断开，尝试重新连接...")
            self._connect()

        # 从参数中提取命令内容
        command = arguments.get("content", "")
        if not command:
            return "", "错误：未提供命令内容"

        try:
            _, stdout, stderr = self.ssh_client.exec_command(command)

            # 读取输出
            stdout_bytes = stdout.read()
            stderr_bytes = stderr.read()

            # 安全解码
            def safe_decode(data: bytes) -> str:
                try:
                    return data.decode("utf-8")
                except UnicodeDecodeError:
                    return data.decode("utf-8", errors="replace")

            return safe_decode(stdout_bytes), safe_decode(stderr_bytes)

        except Exception as e:
            logger.error(f"命令执行失败: {str(e)}")
            return "", f"命令执行错误: {str(e)}"

    @property
    def function_config(self) -> Dict:
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
                    "required": ["content"]
                },
            },
        }
