# ctf_tools/python.py
from ctf_tool.base_tool import BaseTool
from ctf_tool.ssh_shell import SSHShell
from typing import Tuple, Dict
import subprocess
import sys
import tempfile
import os
import time

class PythonTool(BaseTool):
    def execute(self, arguments: dict) -> Tuple[str, str]:
        """执行Python代码"""
        # 从参数中提取必要信息
        content = arguments.get("content", "")
        remote = arguments.get("remote", False)
        
        if remote and self.ssh_shell:
            return self._execute_remotely(content)
        return self._execute_locally(content)
    
    def _execute_locally(self, content: str) -> Tuple[str, str]:
        try:
            with tempfile.NamedTemporaryFile(suffix='.py', delete=False) as tmp:
                tmp.write(content.encode('utf-8'))
                tmp_path = tmp.name
            
            result = subprocess.run(
                [sys.executable, tmp_path],
                capture_output=True,
                text=True,
                timeout=30
            )
            os.unlink(tmp_path)
            return result.stdout, result.stderr
        except Exception as e:
            return "", str(e)
    
    def _execute_remotely(self, content: str) -> Tuple[str, str]:
        temp_name = f"/tmp/py_script_{int(time.time())}.py"
        upload_cmd = f"cat > {temp_name} << 'EOF'\n{content}\nEOF"
        self.ssh_shell.execute_command(upload_cmd)
        stdout, stderr = self.ssh_shell.execute_command(f"python3 {temp_name}")
        self.ssh_shell.execute_command(f"rm -f {temp_name}")
        return stdout, stderr
    
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
                        "purpose": {
                            "type": "string",
                            "description": "执行此步骤的目的"
                        },
                        "content": {
                            "type": "string",
                            "description": "要执行的Python代码"
                        },
                        "remote": {
                            "type": "boolean",
                            "description": "是否在远程服务器执行，默认为False",
                            "default": False
                        }
                    },
                    "required": ["purpose", "content"]
                }
            }
        }