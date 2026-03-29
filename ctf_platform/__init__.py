"""@brief 导出 CTF 平台抽象接口与内置实现。"""

from ctf_platform.base import FlagSubmitter, Question, QuestionInputer, SubmitResult
from ctf_platform.file_inputer import FileQuestionInputer
from ctf_platform.manual_submitter import ManualFlagSubmitter
from ctf_platform.registry import (
    create_inputer,
    create_submitter,
    register_inputer,
    register_submitter,
)

__all__ = [
    "Question",
    "SubmitResult",
    "QuestionInputer",
    "FlagSubmitter",
    "register_inputer",
    "register_submitter",
    "create_inputer",
    "create_submitter",
    "FileQuestionInputer",
    "ManualFlagSubmitter",
]
