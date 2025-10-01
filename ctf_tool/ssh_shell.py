from typing import Dict
from config import Config
from ctf_tool.base_tool import BaseTool
import paramiko

class SSHShell(BaseTool):
    def __init__(self):
        tool_config = Config.get_tool_config("ssh_shell")
        ssh_config = tool_config.get("ssh_shell", {})
        print(ssh_config)
        hostname = ssh_config.get("host")
        port = ssh_config.get("port", 22)
        username = ssh_config.get("username")
        password = ssh_config.get("password")
        self.ssh_client = self.create_ssh_client(hostname, port, username, password)

    def create_ssh_client(self, hostname, port, username, password):
        client = paramiko.SSHClient()
        client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
        client.connect(hostname, port, username, password)
        return client

    def execute(self, arguments):
        # 从参数中提取命令内容
        command = arguments.get("content", "")
        
        _, stdout, stderr = self.ssh_client.exec_command(command)
        
        # 直接读取字节数据，不立即解码
        stdout_bytes = stdout.read()
        stderr_bytes = stderr.read()
        
        # 尝试UTF-8解码，失败时使用错误处理
        def safe_decode(data):
            try:
                return data.decode('utf-8')
            except UnicodeDecodeError:
                # 用占位符替换无效字节
                return data.decode('utf-8', errors='replace')
        
        return safe_decode(stdout_bytes), safe_decode(stderr_bytes)
    
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
                        "content": {
                            "type": "string",
                            "description": "要执行的Shell命令"
                        }
                    },
                    "required": ["content"]
                }
            }
        }