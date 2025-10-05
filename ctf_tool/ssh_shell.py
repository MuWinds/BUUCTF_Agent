from typing import Dict
from config import Config
from ctf_tool.base_tool import BaseTool
import paramiko
import logging

logger = logging.getLogger(__name__)


class SSHShell(BaseTool):
    def __init__(self):
        tool_config:dict = Config.get_tool_config("ssh_shell")
        ssh_config:dict = tool_config.get("ssh_shell", {})
        self.hostname = ssh_config.get("host")
        self.port = ssh_config.get("port", 22)
        self.username = ssh_config.get("username")
        self.password = ssh_config.get("password")
        self.ssh_client = None
        self._connect()  # 初始化时立即连接

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

    def _is_connected(self):
        """检查连接是否有效"""
        if not self.ssh_client:
            return False
        try:
            transport = self.ssh_client.get_transport()
            return transport and transport.is_active()
        except Exception:
            return False

    def execute(self, arguments: dict):
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
            def safe_decode(data:bytes) -> str:
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
                "name": "execute_shell_command",
                "description": "在远程服务器上执行Shell命令，服务器内提供了curl,sqlmap,nmap,openssl等常用工具",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "purpose": {
                            "type": "string",
                            "description": "执行此步骤的目的",
                        },
                        "content": {
                            "type": "string",
                            "description": "要执行的Shell命令",
                        },
                    },
                    "required": ["content", "purpose"],
                },
            },
        }
