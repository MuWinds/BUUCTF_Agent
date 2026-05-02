from abc import ABC, abstractmethod
from typing import Any, Dict, List, Optional, Tuple, TypeAlias, Union

ToolCall: TypeAlias = Dict[str, Any]
ApprovedStep: TypeAlias = Tuple[str, List[ToolCall]]
FeedbackStep: TypeAlias = Tuple[str, List[ToolCall], str]
ManualApprovalStepData: TypeAlias = Union[ApprovedStep, FeedbackStep, None]


class UserInterface(ABC):
    """用户交互抽象接口。

    定义求解流程中与用户交互所需的标准方法。
    """

    @abstractmethod
    def confirm_flag(self, flag_candidate: str) -> bool:
        """让用户确认候选 flag 是否正确。

        Args:
            flag_candidate: 候选 flag。

        Returns:
            用户确认结果；True 表示正确，False 表示不正确。
        """
        pass

    @abstractmethod
    def select_mode(self) -> bool:
        """让用户选择运行模式。

        Returns:
            是否选择自动模式；True 为自动模式，False 为手动模式。
        """
        pass

    @abstractmethod
    def input_question(self, prompt: str) -> str:
        """获取用户输入的题目文本（支持多行）。

        Args:
            prompt: 输入提示信息。

        Returns:
            用户输入的题目文本。
        """
        pass

    @abstractmethod
    def display_message(self, message: str) -> None:
        """向用户显示消息。

        Args:
            message: 要显示的消息内容。
        """
        pass

    @abstractmethod
    def manual_approval(self, think: str, tool_calls: Any) -> tuple[bool, tuple[str, Any]]:
        """在手动模式下获取用户对当前步骤的批准。

        Args:
            think: 当前思考过程文本。
            tool_calls: 计划执行的工具调用信息。

        Returns:
            二元组：(是否批准, (思考过程, 工具调用信息))。
        """
        pass

    @abstractmethod
    def manual_approval_step(
        self,
        think: str,
        tool_calls: List[ToolCall],
    ) -> tuple[bool, ManualApprovalStepData]:
        """在手动模式下执行完整批准流程。

        Args:
            think: 当前思考过程文本。
            tool_calls: 计划执行的工具调用列表。

        Returns:
            二元组：(是否批准, 相关上下文)。
        """
        pass

    @abstractmethod
    def confirm_resume(self) -> bool:
        """询问用户是否恢复存档。

        Returns:
            用户选择结果；True 表示恢复，False 表示重新开始。
        """
        pass
