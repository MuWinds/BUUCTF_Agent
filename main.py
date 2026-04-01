"""
@brief 程序入口模块。
"""

import logging
import os
import sys
from datetime import datetime
from typing import Any, Dict, Optional

from ctf_platform.base import Question

from agent.checkpoint import CheckpointManager
from agent.workflow import Workflow
from config import Config
from ctf_platform import create_inputer, create_submitter
from utils.user_interface import CommandLineInterface


class HTTPRequestToDebugFilter(logging.Filter):
    """
    @brief 将模型 HTTP 请求日志从 INFO 降级为 DEBUG。
    """

    def filter(self, record: logging.LogRecord) -> bool:
        """
        @brief 过滤并降级匹配日志级别。
        @param record 待处理日志记录。
        @return bool 始终返回 True，不拦截日志。
        """
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
    """
    @brief 初始化日志系统并设置第三方库日志级别。
    @return None。
    """
    log_dir = "logs"
    os.makedirs(log_dir, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_file = os.path.join(log_dir, f"log_{timestamp}.log")

    logging.basicConfig(
        level=logging.DEBUG,
        format="%(asctime)s - %(levelname)s - %(message)s",
        handlers=[
            logging.FileHandler(log_file),
            logging.StreamHandler(sys.stdout),
        ],
    )

    file_handler = logging.FileHandler(log_file, encoding="utf-8")
    file_handler.setLevel(logging.DEBUG)
    file_handler.setFormatter(
        logging.Formatter(
            "%(asctime)s - %(name)s - %(levelname)s - %(message)s"
        )
    )

    console_handler = logging.StreamHandler()
    console_handler.setLevel(logging.INFO)
    console_handler.setFormatter(
        logging.Formatter("%(asctime)s - %(levelname)s - %(message)s")
    )

    http_request_filter = HTTPRequestToDebugFilter()
    file_handler.addFilter(http_request_filter)
    console_handler.addFilter(http_request_filter)

    root_logger = logging.getLogger()
    for handler in root_logger.handlers[:]:
        root_logger.removeHandler(handler)

    root_logger.addHandler(file_handler)
    root_logger.addHandler(console_handler)

    logging.getLogger("httpx").setLevel(logging.DEBUG)
    logging.getLogger("httpcore").setLevel(logging.DEBUG)


def main() -> None:
    """
    @brief 执行解题主流程并输出最终结果。
    @return None。
    """
    setup_logging()
    logger = logging.getLogger(__name__)
    config: Dict[str, Any] = Config.load_config()

    cli = CommandLineInterface()

    platform_config_value = config.get("platform", {})
    platform_config: Dict[str, Any] = (
        platform_config_value if isinstance(platform_config_value, dict) else {}
    )

    inputer_config_value = platform_config.get("inputer", {"type": "file"})
    inputer_config: Dict[str, Any] = (
        inputer_config_value if isinstance(inputer_config_value, dict) else {"type": "file"}
    )
    inputer = create_inputer(inputer_config)

    submitter_config_value = platform_config.get("submitter", {"type": "manual"})
    submitter_config: Dict[str, Any] = (
        submitter_config_value
        if isinstance(submitter_config_value, dict)
        else {"type": "manual"}
    )
    submitter = create_submitter(
        submitter_config,
        user_interface=cli,
    )

    checkpoint_dir_value = config.get("checkpoint_dir", "./checkpoints")
    checkpoint_dir = (
        checkpoint_dir_value if isinstance(checkpoint_dir_value, str) else "./checkpoints"
    )
    checkpoint_mgr = CheckpointManager(checkpoint_dir=checkpoint_dir)
    checkpoint_data_raw = checkpoint_mgr.load_any()
    checkpoint_data: Optional[Dict[str, Any]] = (
        checkpoint_data_raw if isinstance(checkpoint_data_raw, dict) else None
    )

    resume_data: Optional[Dict[str, Any]] = None
    question_data: Optional[Question] = None
    question: str

    if checkpoint_data and cli.confirm_resume():
        problem_value = checkpoint_data.get("problem")
        if isinstance(problem_value, str):
            question = problem_value
            resume_data = checkpoint_data
            logger.info("用户选择从存档恢复")
        else:
            cli.display_message("存档数据无效，改为重新读取题目")
            checkpoint_mgr.delete(str(problem_value))
            cli.display_message("如题目中含有附件，可放到项目根目录的attachments文件夹下")
            cli.input_question_ready("将题目文本放在Agent根目录下的question.txt回车以结束")
            question_data = inputer.fetch_question()
            question = question_data.content
    else:
        if checkpoint_data:
            problem_value = checkpoint_data.get("problem")
            if isinstance(problem_value, str):
                checkpoint_mgr.delete(problem_value)

        cli.display_message("如题目中含有附件，可放到项目根目录的attachments文件夹下")
        cli.input_question_ready("将题目文本放在Agent根目录下的question.txt回车以结束")
        question_data = inputer.fetch_question()
        question = question_data.content

    logger.debug("题目内容：%s", question)

    workflow = Workflow(
        config=config,
        user_interface=cli,
        inputer=inputer,
        submitter=submitter,
    )
    result = workflow.solve(
        question,
        resume_data=resume_data,
        question=question_data,
    )

    logger.info("最终结果:%s", result)


if __name__ == "__main__":
    main()
