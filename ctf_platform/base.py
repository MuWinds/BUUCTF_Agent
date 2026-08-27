"""定义题目输入与提交抽象接口。"""

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


@dataclass
class Question:
    """CTF 题目数据结构。

    Attributes:
        title: 题目标题。
        content: 题目正文。
        category: 题目分类（Web/Pwn/Crypto/Misc 等）。
        attachments: 附件文件路径列表。
        url: 靶机地址（如有）。
        metadata: 额外元数据（如平台题目 ID）。
    """

    title: str
    content: str
    category: Optional[str] = None
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
