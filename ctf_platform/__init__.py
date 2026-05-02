"""导出 CTF 平台抽象接口与内置实现。"""

from ctf_platform.base import FlagSubmitter, Platform, Question, QuestionInputer, SolverFn, SubmitResult
from ctf_platform.file_inputer import FileQuestionInputer
from ctf_platform.manual_submitter import ManualFlagSubmitter
from ctf_platform.registry import (
    create_inputer,
    create_platform,
    create_submitter,
    get_all_platform_cli,
    get_platform_cli,
    register_inputer,
    register_platform,
    register_platform_cli,
    register_submitter,
)

# 自动发现并导入所有平台模块，触发 @register_* 装饰器注册
from ctf_platform.registry import _auto_discover  # noqa: F401
_auto_discover()

__all__ = [
    "Question",
    "SubmitResult",
    "SolverFn",
    "QuestionInputer",
    "FlagSubmitter",
    "Platform",
    "register_inputer",
    "register_submitter",
    "register_platform",
    "register_platform_cli",
    "create_inputer",
    "create_submitter",
    "create_platform",
    "get_platform_cli",
    "get_all_platform_cli",
    "FileQuestionInputer",
    "ManualFlagSubmitter",
]
