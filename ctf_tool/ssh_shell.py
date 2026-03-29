"""@brief 提供基于 SSH 的远程 Shell 执行工具。"""

import logging
import os
from typing import Any, Dict, Optional, Tuple

import paramiko

from config import Config
from ctf_tool.base_tool import BaseTool

logger = logging.getLogger(__name__)


class SSHShell(BaseTool):
    """@brief 通过 SSH 在远程主机执行 Shell 命令。"""

    def __init__(self):
        """@brief 初始化 SSH 配置并尝试上传附件。"""
        ssh_config: Dict[str, Any] = Config.get_tool_config("ssh_shell")
        host_value = ssh_config.get("host")
        self.hostname = host_value if isinstance(host_value, str) else ""
        self.port = ssh_config.get("port", 22)
        self.username = ssh_config.get("username")
        self.password = ssh_config.get("password")
        self.ssh_client: Optional[paramiko.SSHClient] = None

        attachment_dir = "./attachments"
        if os.path.isdir(attachment_dir):
            attachment_files = os.listdir(attachment_dir)
            if attachment_files:
                logger.info("检测到题目有附件，正在上传……")
                self.upload_folder(attachment_dir, ".")
                logger.info("附件上传完成")

        self._connect()

    def _connect(self) -> None:
        """@brief 建立 SSH 连接或重连。

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

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> str:
        """@brief 执行远程 Shell 命令。

        @param tool_name 工具名（当前实现不直接使用）。
        @param arguments 命令参数，需包含 content。
        @return 执行结果字符串。
        """
        if not self._is_connected():
            logger.warning("SSH会话断开，尝试重新连接...")
            self._connect()

        command_value = arguments.get("content", "")
        command = command_value if isinstance(command_value, str) else str(command_value)
        if not command:
            return "错误：未提供命令内容"

        try:
            assert self.ssh_client is not None, "SSH客户端未初始化"
            _, stdout, stderr = self.ssh_client.exec_command(command)

            stdout_bytes = stdout.read()
            stderr_bytes = stderr.read()

            def safe_decode(data: bytes) -> str:
                """@brief 安全解码字节数据。

                @param data 待解码字节数据。
                @return str 解码后的字符串。
                """
                try:
                    return data.decode("utf-8")
                except UnicodeDecodeError:
                    return data.decode("utf-8", errors="replace")

            return safe_decode(stdout_bytes) + safe_decode(stderr_bytes)
        except Exception as error:
            logger.error("命令执行失败: %s", str(error))
            return f"命令执行错误: {str(error)}"

    def upload_folder(self, local_path: str, remote_path: str) -> str:
        """@brief 上传本地文件夹到远程目录。

        @param local_path 本地目录路径。
        @param remote_path 远程目录路径。
        @return str 上传结果说明。
        @raises IOError 上传失败时抛出。
        """
        if not self._is_connected():
            logger.warning("SSH会话断开，尝试重新连接...")
            self._connect()

        try:
            assert self.ssh_client is not None, "SSH客户端未初始化"
            sftp = self.ssh_client.open_sftp()

            try:
                sftp.stat(remote_path)
            except IOError:
                sftp.mkdir(remote_path)

            for root, _, files in os.walk(local_path):
                relative_path = os.path.relpath(root, local_path).replace("\\", "/")
                remote_dir = (
                    f"{remote_path}/{relative_path}"
                    if relative_path != "."
                    else remote_path
                )

                try:
                    sftp.stat(remote_dir)
                except IOError:
                    sftp.mkdir(remote_dir)

                for file_name in files:
                    local_file = os.path.join(root, file_name)
                    remote_file = f"{remote_dir}/{file_name}"
                    sftp.put(local_file, remote_file)
                    logger.debug("上传文件: %s -> %s", local_file, remote_file)

            sftp.close()
            return f"文件夹上传成功: {local_path} -> {remote_path}"
        except Exception as error:
            logger.error("文件夹上传失败: %s", str(error))
            raise IOError(f"文件夹上传失败: {str(error)}") from error

    @property
    def function_config(self) -> Dict[str, Any]:
        """@brief 返回工具函数配置。

        @return Dict[str, Any] 函数配置字典。
        """
        return {
            "type": "function",
            "function": {
                "name": "execute_shell_command",
                "description": (
                    "在Linux服务器上执行Shell命令，可以用curl,sqlmap,nmap,openssl等常用工具"
                ),
                "parameters": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "要执行的Shell命令",
                        }
                    },
                    "required": ["content"],
                },
            },
        }
