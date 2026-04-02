"""命令参数到 Workflow 调用的适配。"""

from __future__ import annotations

import json
import logging
import os
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from agent.checkpoint import CheckpointManager
from ctf_platform import Question, create_inputer, create_submitter
from utils.user_interface import UserInterface


class HTTPRequestToDebugFilter(logging.Filter):
    """将 HTTP 请求日志降级为 DEBUG。"""

    def filter(self, record: logging.LogRecord) -> bool:
        if record.name in {"httpx", "httpcore"}:
            message = record.getMessage()
            if (
                isinstance(message, str)
                and message.startswith("HTTP Request:")
                and record.levelno >= logging.INFO
            ):
                record.levelno = logging.DEBUG
                record.levelname = "DEBUG"
        return True


def setup_logging() -> None:
    """初始化日志输出。"""
    log_dir = "logs"
    os.makedirs(log_dir, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_file = os.path.join(log_dir, f"log_{timestamp}.log")

    file_handler = logging.FileHandler(log_file, encoding="utf-8")
    file_handler.setLevel(logging.DEBUG)
    file_handler.setFormatter(
        logging.Formatter("%(asctime)s - %(name)s - %(levelname)s - %(message)s")
    )

    console_handler = logging.StreamHandler()
    console_handler.setLevel(logging.INFO)
    console_handler.setFormatter(
        logging.Formatter("%(asctime)s - %(levelname)s - %(message)s")
    )

    http_filter = HTTPRequestToDebugFilter()
    file_handler.addFilter(http_filter)
    console_handler.addFilter(http_filter)

    root_logger = logging.getLogger()
    root_logger.setLevel(logging.DEBUG)
    for handler in root_logger.handlers[:]:
        root_logger.removeHandler(handler)
    root_logger.addHandler(file_handler)
    root_logger.addHandler(console_handler)

    logging.getLogger("httpx").setLevel(logging.DEBUG)
    logging.getLogger("httpcore").setLevel(logging.DEBUG)


def build_question_from_text(
    content: str,
    attachment_dir: str = "./attachments",
) -> Question:
    """基于文本构造题目对象。"""
    attachments: List[str] = []
    if os.path.isdir(attachment_dir):
        for file_name in os.listdir(attachment_dir):
            path = os.path.join(attachment_dir, file_name)
            if os.path.isfile(path):
                attachments.append(path)

    return Question(
        title="",
        content=content,
        attachments=attachments,
    )


def load_question_from_file(path: str, attachment_dir: str = "./attachments") -> Question:
    """从文件读取题目内容。"""
    with open(path, "r", encoding="utf-8") as file:
        content = file.read()
    return build_question_from_text(content, attachment_dir=attachment_dir)


def resolve_question(
    config: Dict[str, Any],
    question_text: Optional[str],
    question_file: Optional[str],
    attachment_dir_override: Optional[str],
    user_interface: UserInterface,
) -> Tuple[str, Question, str]:
    """解析题目来源并返回 (problem, question_obj, source_text)。"""
    platform_config = config.get("platform", {})
    inputer_config = platform_config.get("inputer", {"type": "file"})
    attachment_dir = inputer_config.get("attachment_dir", "./attachments")
    if attachment_dir_override:
        attachment_dir = attachment_dir_override

    if question_text:
        question = build_question_from_text(question_text, attachment_dir=attachment_dir)
        return question.content, question, "命令参数 --question"

    if question_file:
        question = load_question_from_file(question_file, attachment_dir=attachment_dir)
        return question.content, question, str(Path(question_file))

    # 未显式传参时沿用原逻辑：提示用户后按 inputer 配置读取
    inputer = create_inputer(inputer_config)
    user_interface.display_message("如题目中含有附件，可放到项目根目录的attachments文件夹下")
    user_interface.input_question_ready("将题目文本放在Agent根目录下的question.txt回车以结束")
    question = inputer.fetch_question()
    return question.content, question, inputer_config.get("file_path", "./question.txt")


def load_checkpoint_for_solve(
    checkpoint_mgr: CheckpointManager,
    allow_resume: bool,
    ui: UserInterface,
) -> Optional[Dict[str, Any]]:
    """根据策略读取存档。"""
    if not allow_resume:
        return None

    checkpoint_data = checkpoint_mgr.load_any()
    if not checkpoint_data:
        return None

    if ui.confirm_resume():
        return checkpoint_data
    return None


def clear_all_checkpoints(checkpoint_mgr: CheckpointManager) -> int:
    """清空所有存档并返回删除数量。"""
    files = checkpoint_mgr.list_checkpoints()
    deleted = 0
    for file_name in files:
        file_path = os.path.join(checkpoint_mgr.checkpoint_dir, file_name)
        if os.path.isfile(file_path):
            os.remove(file_path)
            deleted += 1
    return deleted


def load_checkpoint_file(
    checkpoint_mgr: CheckpointManager,
    file_name: str,
) -> Optional[Dict[str, Any]]:
    """读取指定存档文件内容。"""
    if not file_name.endswith(".json"):
        return None
    file_path = os.path.join(checkpoint_mgr.checkpoint_dir, file_name)
    if not os.path.isfile(file_path):
        return None
    try:
        with open(file_path, "r", encoding="utf-8") as file:
            return json.load(file)
    except (json.JSONDecodeError, OSError):
        return None


def run_workflow(
    config: Dict[str, Any],
    user_interface: UserInterface,
    problem: str,
    question: Question,
    resume_data: Optional[Dict[str, Any]],
) -> str:
    """执行 Workflow 主流程。"""
    from agent.workflow import Workflow

    platform_config = config.get("platform", {})
    inputer = create_inputer(platform_config.get("inputer", {"type": "file"}))
    submitter = create_submitter(
        platform_config.get("submitter", {"type": "manual"}),
        user_interface=user_interface,
    )

    workflow = Workflow(
        config=config,
        user_interface=user_interface,
        inputer=inputer,
        submitter=submitter,
    )
    return workflow.solve(
        problem,
        resume_data=resume_data,
        question=question,
    )
