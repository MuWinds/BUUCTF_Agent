from ctf_platform.base import Question, SubmitResult, QuestionInputer, FlagSubmitter
from ctf_platform.registry import (
    register_inputer, register_submitter,
    create_inputer, create_submitter,
)

# 导入内置实现，触发注册
from ctf_platform.file_inputer import FileQuestionInputer
from ctf_platform.manual_submitter import ManualFlagSubmitter

__all__ = [
    "Question", "SubmitResult",
    "QuestionInputer", "FlagSubmitter",
    "register_inputer", "register_submitter",
    "create_inputer", "create_submitter",
    "FileQuestionInputer", "ManualFlagSubmitter",
]
