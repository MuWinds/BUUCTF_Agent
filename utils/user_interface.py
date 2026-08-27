"""用户交互抽象接口定义。"""

from abc import ABC, abstractmethod


class UserInterface(ABC):
    """用户交互抽象接口。"""

    @abstractmethod
    def confirm_flag(self, flag_candidate: str) -> bool:
        """让用户确认候选 flag 是否正确。

        Args:
            flag_candidate: 候选 flag。

        Returns:
            用户确认结果。
        """
        pass

    @abstractmethod
    def input_question(self) -> str:
        """多行题目输入，空行结束。

        Returns:
            用户输入的完整题目文本。
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
    def confirm_resume(self) -> bool:
        """询问用户是否恢复存档。

        Returns:
            用户选择结果。
        """
        pass
