"""@brief 定义题目输入与提交抽象接口。"""

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


@dataclass
class Question:
    """@brief CTF 题目数据结构。

    @param title 题目标题。
    @param content 题目正文。
    @param category 题目分类（Web/Pwn/Crypto/Misc 等）。
    @param attachments 附件文件路径列表。
    @param url 靶机地址（如有）。
    @param metadata 额外元数据（如平台题目 ID）。
    """

    title: str
    content: str
    category: Optional[str] = None
    attachments: Optional[List[str]] = None
    url: Optional[str] = None
    metadata: Optional[Dict[str, Any]] = None


@dataclass
class SubmitResult:
    """@brief Flag 提交结果数据结构。

    @param success 是否提交成功。
    @param message 平台返回消息或说明。
    """

    success: bool
    message: str = ""


class QuestionInputer(ABC):
    """@brief 题目输入器抽象基类。"""

    @abstractmethod
    def fetch_question(self) -> Question:
        """@brief 获取一道题目。

        @return Question 题目数据。
        """
        raise NotImplementedError

    def list_questions(self) -> List[Question]:
        """@brief 列出可用题目（可选实现）。

        @return List[Question] 题目列表。
        @raises NotImplementedError 当输入器不支持列题时抛出。
        """
        raise NotImplementedError("该输入器不支持列出题目")


class FlagSubmitter(ABC):
    """@brief Flag 提交器抽象基类。"""

    @abstractmethod
    def submit(self, flag: str, question: Question) -> SubmitResult:
        """@brief 提交 Flag。

        @param flag 待提交的 Flag 字符串。
        @param question 对应题目，用于定位平台上的题目。
        @return SubmitResult 提交结果。
        """
        raise NotImplementedError
