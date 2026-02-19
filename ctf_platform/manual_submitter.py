from ctf_platform.base import FlagSubmitter, Question, SubmitResult
from ctf_platform.registry import register_submitter
from utils.user_interface import UserInterface


@register_submitter("manual")
class ManualFlagSubmitter(FlagSubmitter):
    """通过用户手动确认来验证flag（兼容现有行为）"""

    def __init__(self, user_interface: UserInterface):
        self.user_interface = user_interface

    def submit(self, flag: str, question: Question) -> SubmitResult:
        confirmed = self.user_interface.confirm_flag(flag)
        return SubmitResult(
            success=confirmed,
            message="用户确认正确" if confirmed else "用户确认不正确"
        )
