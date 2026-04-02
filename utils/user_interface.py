from abc import ABC, abstractmethod
from typing import Any, Dict, List, Optional, Tuple, TypeAlias, Union

ToolCall: TypeAlias = Dict[str, Any]
ApprovedStep: TypeAlias = Tuple[str, List[ToolCall]]
FeedbackStep: TypeAlias = Tuple[str, List[ToolCall], str]
ManualApprovalStepData: TypeAlias = Union[ApprovedStep, FeedbackStep, None]


class UserInterface(ABC):
    """
    @brief 用户交互抽象接口。

    @details
    定义求解流程中与用户交互所需的标准方法。
    """

    @abstractmethod
    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        @brief 让用户确认候选 flag 是否正确。
        @param flag_candidate 候选 flag。
        @return 用户确认结果；True 表示正确，False 表示不正确。
        """
        pass

    @abstractmethod
    def select_mode(self) -> bool:
        """
        @brief 让用户选择运行模式。
        @return 是否选择自动模式；True 为自动模式，False 为手动模式。
        """
        pass

    @abstractmethod
    def input_question_ready(self, prompt: str) -> None:
        """
        @brief 等待用户确认题目输入已准备完毕。
        @param prompt 输入提示信息。
        @return 无返回值。
        """
        pass

    @abstractmethod
    def display_message(self, message: str) -> None:
        """
        @brief 向用户显示消息。
        @param message 要显示的消息内容。
        @return 无返回值。
        """
        pass

    @abstractmethod
    def manual_approval(self, think: str, tool_calls: Any) -> tuple[bool, tuple[str, Any]]:
        """
        @brief 在手动模式下获取用户对当前步骤的批准。
        @param think 当前思考过程文本。
        @param tool_calls 计划执行的工具调用信息。
        @return 二元组：(是否批准, (思考过程, 工具调用信息))。
        """
        pass

    @abstractmethod
    def manual_approval_step(
        self,
        think: str,
        tool_calls: List[ToolCall],
    ) -> tuple[bool, ManualApprovalStepData]:
        """
        @brief 在手动模式下执行完整批准流程。
        @param think 当前思考过程文本。
        @param tool_calls 计划执行的工具调用列表。
        @return 二元组：(是否批准, 相关上下文)。
        """
        pass

    @abstractmethod
    def confirm_resume(self) -> bool:
        """
        @brief 询问用户是否恢复存档。
        @return 用户选择结果；True 表示恢复，False 表示重新开始。
        """
        pass
