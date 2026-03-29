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


class CommandLineInterface(UserInterface):
    """
    @brief 命令行用户交互实现。
    """

    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        @brief 让用户确认候选 flag 是否正确。
        @param flag_candidate 候选 flag。
        @return 用户确认结果；True 表示正确，False 表示不正确。
        """
        print(f"\n发现flag：\n{flag_candidate}")
        print("请确认这个flag是否正确？")

        while True:
            response = input("输入 'y' 确认正确，输入 'n' 表示不正确: ").strip().lower()
            if response == "y":
                return True
            if response == "n":
                return False
            print("无效输入，请输入 'y' 或 'n'")

    def select_mode(self) -> bool:
        """
        @brief 让用户选择运行模式。
        @return 是否选择自动模式；True 为自动模式，False 为手动模式。
        """
        print("\n请选择运行模式:")
        print("1. 自动模式（Agent自动生成和执行所有命令）")
        print("2. 手动模式（每一步需要用户批准）")

        while True:
            choice = input("请输入选项编号: ").strip()
            if choice == "1":
                return True
            if choice == "2":
                return False
            print("无效选项，请重新选择")

    def input_question_ready(self, prompt: str) -> None:
        """
        @brief 等待用户确认题目输入已准备完毕。
        @param prompt 输入提示信息。
        @return 无返回值。
        """
        input(prompt)

    def display_message(self, message: str) -> None:
        """
        @brief 向用户显示消息。
        @param message 要显示的消息内容。
        @return 无返回值。
        """
        print(message)

    def manual_approval(self, think: str, tool_calls: Any) -> tuple[bool, tuple[str, Any]]:
        """
        @brief 在手动模式下获取用户对当前步骤的批准。
        @param think 当前思考过程文本。
        @param tool_calls 计划执行的工具调用信息。
        @return 二元组：(是否批准, (思考过程, 工具调用信息))。
        """
        print(f"\n思考过程: {think}")
        print(f"工具调用: {tool_calls}")

        while True:
            response = input("是否批准执行？(y/n): ").strip().lower()
            if response == "y":
                return True, (think, tool_calls)
            if response == "n":
                return False, (think, tool_calls)
            print("无效输入，请输入 'y' 或 'n'")

    def manual_approval_step(
        self,
        think: str,
        tool_calls: List[ToolCall],
    ) -> tuple[bool, ManualApprovalStepData]:
        """
        @brief 在手动模式下执行完整批准流程。
        @param think 当前思考过程文本。
        @param tool_calls 计划执行的工具调用列表。
        @return
            二元组：(是否批准, 相关上下文)。
            当选择“反馈并重新思考”时，第二项包含额外 feedback 字段；
            当选择“终止解题”时，第二项为 None。
        """
        while True:
            print(f"\n计划调用 {len(tool_calls)} 个工具:")
            for index, tool_call in enumerate(tool_calls, start=1):
                print(f"{index}. {tool_call.get('tool_name')}: {tool_call.get('arguments')}")
            print()

            print("1. 批准并执行所有工具")
            print("2. 提供反馈并重新思考")
            print("3. 终止解题")
            choice = input("请输入选项编号: ").strip()

            if choice == "1":
                return True, (think, tool_calls)
            if choice == "2":
                feedback = input("请提供改进建议: ").strip()
                return False, (think, tool_calls, feedback)
            if choice == "3":
                return False, None
            print("无效选项，请重新选择")

    def confirm_resume(self) -> bool:
        """
        @brief 询问用户是否恢复存档。
        @return 用户选择结果；True 表示恢复，False 表示重新开始。
        """
        print("\n检测到未完成的解题存档，是否恢复？")
        while True:
            response = input("输入 'y' 恢复存档，输入 'n' 重新开始: ").strip().lower()
            if response == "y":
                return True
            if response == "n":
                return False
            print("无效输入，请输入 'y' 或 'n'")
