"""命令参数到 Workflow 调用的适配。"""

from __future__ import annotations

import json
import logging
import os
import re
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

    checkpoint_data = checkpoint_mgr.load_latest()
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
    """执行 Agent 自主解题流程。"""
    import yaml
    from jinja2 import Environment, BaseLoader

    from agent.agent_core import run
    from agent.checkpoint import CheckpointManager
    from agent.skill import discover_skills
    from ctf_platform.registry import create_submitter
    from ctf_tool.load_skill import LoadSkillTool
    from utils.llm_request import LLMRequest
    from utils.tools import ToolUtils

    # 加载系统提示模板
    with open("./prompt/system_prompt.yaml", "r", encoding="utf-8") as f:
        prompt_data = yaml.safe_load(f)

    # 发现并加载 skills
    skill_registry = discover_skills(config=config, project_dir=".")

    # 加载工具（含 load_skill）
    tool_utils = ToolUtils()
    tools, tool_defs = tool_utils.load_tools()

    # 注册 load_skill 工具
    if skill_registry.skills:
        load_skill_tool = LoadSkillTool(skill_registry)
        tools["load_skill"] = load_skill_tool
        tool_defs.append(load_skill_tool.function_config)
        user_interface.display_message(
            f"已加载 {len(skill_registry.skills)} 个 skills: "
            + ", ".join(skill_registry.names())
        )

    if not tool_defs:
        user_interface.display_message("当前没有可用工具，无法解题")
        return "未找到flag：无可用工具"

    # 渲染系统提示（注入 skills 列表）
    env = Environment(loader=BaseLoader())
    template = env.from_string(prompt_data["system_prompt"])
    skills_info = [{"name": s.name, "description": s.description} for s in skill_registry.all()]
    system_prompt = template.render(
        question=problem,
        tools=tool_defs,
        skills=skills_info,
    )

    # 构建初始消息
    if resume_data and "messages" in resume_data:
        messages = resume_data["messages"]
        user_interface.display_message("已恢复存档，继续解题")
    else:
        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": problem},
        ]

    # 初始化 LLM
    llm = LLMRequest("solve_agent")

    # 初始化存档管理器
    checkpoint_dir = config.get("checkpoint_dir", "./checkpoints")
    checkpoint_mgr = CheckpointManager(checkpoint_dir=checkpoint_dir)

    # 设置 flag 确认回调
    platform_config = config.get("platform", {})
    submitter = create_submitter(
        platform_config.get("submitter", {"type": "manual"}),
        user_interface=user_interface,
    )

    # 运行 agent 循环
    result = run(
        messages=messages,
        tools=tools,
        tool_defs=tool_defs,
        llm=llm,
        on_message=user_interface.display_message,
        checkpoint_mgr=checkpoint_mgr,
        problem=problem,
    )

    # 尝试提交 flag
    if "flag{" in result.lower() or "FLAG_FOUND" in result:
        flag_candidate = _extract_flag_from_result(result)
        if flag_candidate:
            submit_result = submitter.submit(flag_candidate, question)
            if submit_result.success:
                checkpoint_mgr.delete_latest()
                return flag_candidate

    return result


def _extract_flag_from_result(result: str) -> Optional[str]:
    """从结果字符串中提取 flag。"""
    match = re.search(r"flag\{[^}]+\}", result, re.IGNORECASE)
    if match:
        return match.group(0)
    match = re.search(r"FLAG_FOUND:\s*(.+)", result)
    if match:
        return match.group(1).strip()
    return None
