"""CTF Agent 状态数据结构定义"""
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Optional
from enum import Enum
import json


class JudgeResult(Enum):
    """判断结果枚举"""
    MET = "met"
    NOT_MET = "not_met"
    STUCK = "stuck"


@dataclass
class EvidenceItem:
    """证据项数据结构"""
    ts: str  # ISO 时间
    step_id: int  # 步骤ID
    goal: str  # 本轮小目标
    action: dict  # 工具调用信息
    observation: dict  # 观测结果
    conclusion: str  # 本轮结论
    judge_result: Optional[str] = None  # 判断结果

    def to_dict(self) -> dict:
        return {
            "ts": self.ts,
            "step_id": self.step_id,
            "goal": self.goal,
            "action": self.action,
            "observation": self.observation,
            "conclusion": self.conclusion,
            "judge_result": self.judge_result
        }

    def to_jsonl(self) -> str:
        return json.dumps(self.to_dict(), ensure_ascii=False)


@dataclass
class PlanItem:
    """计划项数据结构"""
    id: int
    description: str
    priority: int = 0  # 优先级，数字越小优先级越高
    status: str = "pending"  # pending, in_progress, completed, failed
    success_criteria: str = ""

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "description": self.description,
            "priority": self.priority,
            "status": self.status,
            "success_criteria": self.success_criteria
        }


@dataclass
class State:
    """Agent 状态"""
    # 题目信息
    target_url: str = ""
    prompt: str = ""  # 题面原文
    category: str = ""  # 题目类别
    
    # 证据库
    evidences: list = field(default_factory=list)
    
    # 假设集
    hypotheses: list = field(default_factory=list)
    
    # 计划栈
    plans: list = field(default_factory=list)
    
    # 解题产物
    artifacts: dict = field(default_factory=dict)
    
    # 当前步骤
    current_step: int = 0
    max_steps: int = 20
    
    # 会话目录
    session_dir: str = ""
    
    # 工具调用历史
    tool_call_history: list = field(default_factory=list)

    def add_evidence(self, evidence: EvidenceItem) -> None:
        """添加证据"""
        self.evidences.append(evidence)
        self.current_step = evidence.step_id

    def add_plan(self, plan: PlanItem) -> None:
        """添加计划"""
        self.plans.append(plan)

    def get_current_plan(self) -> Optional[PlanItem]:
        """获取当前执行中的计划"""
        for plan in self.plans:
            if plan.status == "in_progress":
                return plan
        return None

    def update_plan_status(self, plan_id: int, status: str) -> None:
        """更新计划状态"""
        for plan in self.plans:
            if plan.id == plan_id:
                plan.status = status
                break

    def add_artifact(self, name: str, path: str, hash_value: str = "") -> None:
        """添加产物"""
        self.artifacts[name] = {
            "path": path,
            "hash": hash_value,
            "ts": datetime.now().isoformat()
        }

    def is_finished(self) -> bool:
        """是否已完成"""
        return self.current_step >= self.max_steps

    def to_dict(self) -> dict:
        return {
            "target_url": self.target_url,
            "prompt": self.prompt,
            "category": self.category,
            "evidences": [e.to_dict() for e in self.evidences],
            "hypotheses": self.hypotheses,
            "plans": [p.to_dict() for p in self.plans],
            "artifacts": self.artifacts,
            "current_step": self.current_step,
            "max_steps": self.max_steps,
            "session_dir": self.session_dir
        }