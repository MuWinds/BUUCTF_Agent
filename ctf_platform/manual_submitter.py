"""提供手动确认方式的 Flag 提交器实现。"""

from ctf_platform.base import FlagSubmitter, Question, SubmitResult
from ctf_platform.registry import register_submitter
from utils.user_interface import UserInterface


@register_submitter("manual")
class ManualFlagSubmitter(FlagSubmitter):
    """通过用户手动确认来验证 Flag。"""

    def __init__(self, user_interface: UserInterface):
        """初始化手动提交器。

        Args:
            user_interface: 用户交互接口实例。
        """
        self.user_interface = user_interface

    def submit(self, flag: str, question: Question) -> SubmitResult:
        """通过用户确认结果提交 Flag。

        Args:
            flag: 待确认的 Flag。
            question: 对应题目对象（当前实现不直接使用）。

        Returns:
            提交结果。
        """
        confirmed = self.user_interface.confirm_flag(flag)
        return SubmitResult(
            success=confirmed,
            message="用户确认正确" if confirmed else "用户确认不正确",
        )
