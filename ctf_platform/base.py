"""定义题目输入、提交与平台抽象接口。"""

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any, Callable, Dict, List, Optional

# 解题函数签名：接收 Question 对象，返回解题结果
SolverFn = Callable[["Question"], str]


@dataclass
class Question:
    """CTF 题目数据结构。

    Attributes:
        title: 题目标题。
        content: 题目正文。
        attachments: 附件文件路径列表。
        url: 靶机地址（如有）。
        metadata: 额外元数据（如平台题目 ID）。
    """

    title: str
    content: str
    attachments: Optional[List[str]] = None
    url: Optional[str] = None
    metadata: Optional[Dict[str, Any]] = None


@dataclass
class SubmitResult:
    """Flag 提交结果数据结构。

    Attributes:
        success: 是否提交成功。
        message: 平台返回消息或说明。
    """

    success: bool
    message: str = ""


class QuestionInputer(ABC):
    """题目输入器抽象基类。"""

    @abstractmethod
    def fetch_question(self) -> Question:
        """获取一道题目。

        Returns:
            题目数据。
        """
        raise NotImplementedError

    def list_questions(self) -> List[Question]:
        """列出可用题目（可选实现）。

        Returns:
            题目列表。

        Raises:
            NotImplementedError: 当输入器不支持列题时抛出。
        """
        raise NotImplementedError("该输入器不支持列出题目")


class FlagSubmitter(ABC):
    """Flag 提交器抽象基类。"""

    @abstractmethod
    def submit(self, flag: str, question: Question) -> SubmitResult:
        """提交 Flag。

        Args:
            flag: 待提交的 Flag 字符串。
            question: 对应题目，用于定位平台上的题目。

        Returns:
            提交结果。
        """
        raise NotImplementedError


class Platform(ABC):
    """平台编排抽象基类。

    负责协调题目获取、解题调用、结果收集的完整流程。
    各平台实现此接口以支持不同的批量解题模式。
    """

    def __init__(
        self,
        inputer: "QuestionInputer",
        submitter: "FlagSubmitter",
        user_interface: Any,
    ):
        """初始化平台编排器。

        Args:
            inputer: 题目输入器。
            submitter: Flag 提交器。
            user_interface: 用户交互接口。
        """
        self.inputer = inputer
        self.submitter = submitter
        self.ui = user_interface

    @abstractmethod
    def run(self, solver: SolverFn) -> dict:
        """执行平台解题流程。

        Args:
            solver: 解题函数，接收 (problem, question) 返回结果。

        Returns:
            测试报告字典。
        """
        raise NotImplementedError

    def on_question_done(self, question: Question, result: str) -> None:
        """单题解题结束后的回调，子类可覆盖以实现自定义行为。

        Args:
            question: 刚结束的题目。
            result: 解题结果字符串。
        """
