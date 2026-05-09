from abc import ABC, abstractmethod


class UserInterface(ABC):
    """
    @brief 用户交互抽象接口。
    """

    @abstractmethod
    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        @brief 让用户确认候选 flag 是否正确。
        @param flag_candidate 候选 flag。
        @return 用户确认结果。
        """
        pass

    @abstractmethod
    def input_question_ready(self, prompt: str) -> None:
        """
        @brief 等待用户确认题目输入已准备完毕。
        @param prompt 输入提示信息。
        """
        pass

    @abstractmethod
    def display_message(self, message: str) -> None:
        """
        @brief 向用户显示消息。
        @param message 要显示的消息内容。
        """
        pass

    @abstractmethod
    def confirm_resume(self) -> bool:
        """
        @brief 询问用户是否恢复存档。
        @return 用户选择结果。
        """
        pass
