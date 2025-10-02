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
    def __init__(self):
        # 在初始化时询问是否要远程执行
        self.remote = self.ask_remote_execution()
        if self.remote:
            self.ssh_shell = SSHShell()
    
    def ask_remote_execution(self) -> bool:
        """询问用户是否要远程执行"""
        print("\n--- Python 执行选项 ---")
        print("1. 本地执行")
        print("2. 远程执行")
        choice = input("请选择 Python 代码的执行方式 (1/2): ").strip()
        
        return choice == "2"
    
    def execute(self, arguments: dict) -> Tuple[str, str]:
        """执行Python代码"""
        content = arguments.get("content", "")
        
        if self.remote:
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
        if self.ssh_shell is None:
            return "", "错误：未配置SSH，无法远程执行"
        
        temp_name = f"/tmp/py_script_{int(time.time())}.py"
        
        # 修复：使用字典参数调用execute方法
        upload_cmd = f"cat > {temp_name} << 'EOF'\n{content}\nEOF"
        self.ssh_shell.execute({"content": upload_cmd, "purpose": "上传Python脚本"})
        
        # 修复：使用字典参数调用execute方法
        stdout, stderr = self.ssh_shell.execute({
            "content": f"python3 {temp_name}", 
            "purpose": "执行Python脚本"
        })
        
        # 修复：使用字典参数调用execute方法
        self.ssh_shell.execute({
            "content": f"rm -f {temp_name}", 
            "purpose": "清理临时文件"
        })
        
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
                        }
                    },
                    "required": ["purpose", "content"]
                }
            }
        }