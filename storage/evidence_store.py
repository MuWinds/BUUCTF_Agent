"""证据存储"""
import json
from datetime import datetime
from pathlib import Path
from typing import Optional, List

from agent.state import EvidenceItem


class EvidenceStore:
    """证据存储管理器"""
    
    def __init__(self, logs_dir: str):
        self.logs_dir = Path(logs_dir)
        self.evidence_file = self.logs_dir / "evidence.jsonl"
        self.tool_calls_file = self.logs_dir / "tool_calls.jsonl"
        self.summary_file = self.logs_dir / "summary.md"
        
        # 确保目录存在
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        
        # 初始化文件
        if not self.evidence_file.exists():
            self.evidence_file.touch()
        if not self.tool_calls_file.exists():
            self.tool_calls_file.touch()
        if not self.summary_file.exists():
            self.summary_file.write_text("# CTF Agent 运行日志\n\n", encoding="utf-8")
    
    def append_evidence(self, evidence: EvidenceItem) -> None:
        """追加证据记录"""
        with open(self.evidence_file, "a", encoding="utf-8") as f:
            f.write(evidence.to_jsonl() + "\n")
    
    def append_tool_call(self, tool_name: str, arguments: dict, 
                         raw_output: dict, duration_ms: float = 0,
                         error: Optional[str] = None) -> int:
        """追加工具调用记录，返回行号"""
        record = {
            "ts": datetime.now().isoformat(),
            "tool_name": tool_name,
            "arguments": arguments,
            "raw_output": raw_output,
            "duration_ms": duration_ms,
            "error": error
        }
        
        with open(self.tool_calls_file, "a", encoding="utf-8") as f:
            f.write(json.dumps(record, ensure_ascii=False) + "\n")
        
        # 计算行号
        with open(self.tool_calls_file, "r", encoding="utf-8") as f:
            return sum(1 for _ in f)
    
    def append_summary(self, step_id: int, goal: str, 
                       action: str, observation: str, 
                       conclusion: str) -> None:
        """追加人类可读摘要"""
        content = f"""
## Step {step_id}

**目标**: {goal}

**动作**: {action}

**观测**: {observation}

**结论**: {conclusion}

---
"""
        with open(self.summary_file, "a", encoding="utf-8") as f:
            f.write(content)
    
    def get_evidences(self) -> List[dict]:
        """读取所有证据"""
        evidences = []
        if self.evidence_file.exists():
            with open(self.evidence_file, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if line:
                        evidences.append(json.loads(line))
        return evidences
    
    def get_tool_calls(self) -> List[dict]:
        """读取所有工具调用"""
        calls = []
        if self.tool_calls_file.exists():
            with open(self.tool_calls_file, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if line:
                        calls.append(json.loads(line))
        return calls