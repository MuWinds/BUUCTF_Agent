"""会话目录与产物管理"""
import hashlib
import shutil
from datetime import datetime
from pathlib import Path
from typing import Optional


class SessionManager:
    """会话目录管理器"""
    
    def __init__(self, base_dir: str = "runs"):
        self.base_dir = Path(base_dir)
        self.session_dir: Optional[Path] = None
        self.input_dir: Optional[Path] = None
        self.work_dir: Optional[Path] = None
        self.artifacts_dir: Optional[Path] = None
        self.logs_dir: Optional[Path] = None
    
    def create_session(self, timestamp: Optional[str] = None) -> str:
        """创建新会话目录"""
        if timestamp is None:
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        
        self.session_dir = self.base_dir / timestamp
        self.input_dir = self.session_dir / "input"
        self.work_dir = self.session_dir / "work"
        self.artifacts_dir = self.session_dir / "artifacts"
        self.logs_dir = self.session_dir / "logs"
        
        # 创建所有目录
        for d in [self.session_dir, self.input_dir, self.work_dir, 
                  self.artifacts_dir, self.logs_dir]:
            d.mkdir(parents=True, exist_ok=True)
        
        return str(self.session_dir)
    
    def save_prompt(self, prompt: str) -> str:
        """保存题面"""
        if self.input_dir is None:
            raise RuntimeError("Session not created")
        
        prompt_path = self.input_dir / "prompt.md"
        prompt_path.write_text(prompt, encoding="utf-8")
        return str(prompt_path)
    
    def save_work_file(self, filename: str, content: bytes) -> str:
        """保存工作文件"""
        if self.work_dir is None:
            raise RuntimeError("Session not created")
        
        file_path = self.work_dir / filename
        file_path.parent.mkdir(parents=True, exist_ok=True)
        
        if isinstance(content, str):
            content = content.encode("utf-8")
        
        file_path.write_bytes(content)
        return str(file_path)
    
    def save_artifact(self, filename: str, content: bytes) -> dict:
        """保存产物文件，返回路径和哈希"""
        if self.artifacts_dir is None:
            raise RuntimeError("Session not created")
        
        file_path = self.artifacts_dir / filename
        file_path.parent.mkdir(parents=True, exist_ok=True)
        
        if isinstance(content, str):
            content = content.encode("utf-8")
        
        file_path.write_bytes(content)
        hash_value = hashlib.sha256(content).hexdigest()
        
        return {
            "path": str(file_path),
            "hash": hash_value
        }
    
    def get_log_path(self, filename: str) -> str:
        """获取日志文件路径"""
        if self.logs_dir is None:
            raise RuntimeError("Session not created")
        return str(self.logs_dir / filename)
    
    def calculate_hash(self, content: bytes) -> str:
        """计算文件哈希"""
        return hashlib.sha256(content).hexdigest()
    
    def cleanup(self) -> None:
        """清理会话目录"""
        if self.session_dir and self.session_dir.exists():
            shutil.rmtree(self.session_dir)