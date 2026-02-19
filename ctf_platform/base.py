from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Optional, List


@dataclass
class Question:
    """题目数据结构"""
    title: str                          # 题目标题
    content: str                        # 题目正文
    category: Optional[str] = None      # 题目分类（Web/Pwn/Crypto/Misc等）
    attachments: Optional[List[str]] = None  # 附件文件路径列表
    url: Optional[str] = None           # 靶机地址（如有）
    metadata: Optional[dict] = None     # 额外元数据（平台题目ID等）


@dataclass
class SubmitResult:
    """提交结果数据结构"""
    success: bool           # 是否正确
    message: str = ""       # 平台返回的消息或说明


class QuestionInputer(ABC):
    """题目输入器抽象基类"""

    @abstractmethod
    def fetch_question(self) -> Question:
        """获取一道题目

        Returns:
            Question: 题目数据
        """
        pass

    def list_questions(self) -> List[Question]:
        """列出可用题目（可选实现，用于平台批量拉取场景）

        Returns:
            List[Question]: 题目列表
        """
        raise NotImplementedError("该输入器不支持列出题目")


class FlagSubmitter(ABC):
    """Flag提交器抽象基类"""

    @abstractmethod
    def submit(self, flag: str, question: Question) -> SubmitResult:
        """提交flag

        Args:
            flag: 待提交的flag字符串
            question: 对应的题目（用于定位平台上的题目）

        Returns:
            SubmitResult: 提交结果
        """
        pass
