from typing import Dict
from config import Config
from ctf_tool.base_tool import BaseTool
import paramiko
import os
import logging

logger = logging.getLogger(__name__)


class SSHShell(BaseTool):
    def __init__(self):
        ssh_config: dict = Config.get_tool_config("ssh_shell")
        self.hostname = ssh_config.get("host")
        self.port = ssh_config.get("port", 22)
        self.username = ssh_config.get("username")
        self.password = ssh_config.get("password")
        self.ssh_client = None
        # 如果附件目录不为空的话，上传附件
        if len(os.listdir("./attachments")) > 0:
            logger.info("检测到题目有附件，正在上传……")
            self.upload_folder("./attachments", ".")
            logger.info("附件上传完成")
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

    def execute(self, tool_name: str, arguments: dict):
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

    def upload_folder(self, local_path, remote_path):
        if not self._is_connected():
            logger.warning("SSH会话断开，尝试重新连接...")
            self._connect()

        try:
            sftp = self.ssh_client.open_sftp()

            # 确保远程路径存在
            try:
                sftp.stat(remote_path)
            except IOError:
                sftp.mkdir(remote_path)

            # 递归上传文件夹
            for root, _, files in os.walk(local_path):
                # 计算相对路径并转换为Unix风格
                relative_path = os.path.relpath(root, local_path).replace("\\", "/")
                remote_dir = (
                    remote_path + "/" + relative_path
                    if relative_path != "."
                    else remote_path
                )

                # 确保远程目录存在
                try:
                    sftp.stat(remote_dir)
                except IOError:
                    sftp.mkdir(remote_dir)

                # 上传文件
                for file in files:
                    local_file = os.path.join(root, file)
                    remote_file = remote_dir + "/" + file  # 使用Unix风格路径
                    sftp.put(local_file, remote_file)
                    logger.debug(f"上传文件: {local_file} -> {remote_file}")

                sftp.close()
                return f"文件夹上传成功: {local_path} -> {remote_path}"

        except Exception as e:
            logger.error(f"文件夹上传失败: {str(e)}")
            raise IOError(f"文件夹上传失败: {str(e)}")

    @property
    def function_config(self) -> Dict:
        return {
            "type": "function",
            "function": {
                "name": "execute_shell_command",
                "description": "在Linux服务器上执行Shell命令",
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
