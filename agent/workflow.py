"""
@brief 解题流程编排模块。
"""

import logging
import os
from typing import Optional

import yaml

from agent.solve_agent import SolveAgent
from ctf_platform.base import FlagSubmitter, Question, QuestionInputer
from ctf_platform.registry import create_inputer, create_submitter
from utils.llm_request import LLMRequest
from utils.text import optimize_text
from utils.user_interface import UserInterface

logger = logging.getLogger(__name__)


class Workflow:
    """
    @brief 负责衔接题目预处理、解题代理与 flag 提交流程。
    """

    def __init__(
        self,
        config: dict,
        user_interface: UserInterface,
        inputer: Optional[QuestionInputer] = None,
        submitter: Optional[FlagSubmitter] = None,
    ) -> None:
        """
        @brief 初始化流程编排对象。
        @param config 全局配置字典。
        @param user_interface 用户交互接口实现。
        @param inputer 题目输入器。
        @param submitter flag 提交器。
        @raises ValueError 当配置为空时抛出。
        """
        self.config = config
        self.processor_llm = LLMRequest("solve_agent")
        with open("./prompt.yaml", "r", encoding="utf-8") as file:
            self.prompt: dict = yaml.safe_load(file)
        self.user_interface = user_interface

        if self.config is None:
            raise ValueError("配置文件不存在")

        platform_config = config.get("platform", {})
        self.inputer = inputer or create_inputer(
            platform_config.get("inputer", {"type": "file"})
        )
        self.submitter = submitter or create_submitter(
            platform_config.get("submitter", {"type": "manual"}),
            user_interface=self.user_interface,
        )
        self.current_question: Optional[Question] = None

    def solve(
        self,
        problem: str,
        resume_data: Optional[dict] = None,
        question: Optional[Question] = None,
    ) -> str:
        """
        @brief 执行完整解题流程。
        @param problem 原始题目文本。
        @param resume_data 可选存档恢复数据。
        @param question 当前题目对象。
        @return 解题结果字符串。
        """
        self.current_question = question

        problem = self.summary_problem(problem)

        self.agent = SolveAgent(problem, user_interface=self.user_interface)
        self.agent.confirm_flag_callback = self.confirm_flag

        resume_step = 0
        if resume_data:
            resume_step = self.agent.restore_from_checkpoint(resume_data)
            self.user_interface.display_message(
                f"已恢复存档，从第 {resume_step} 步继续"
            )

        result = self.agent.solve(resume_step=resume_step)
        return result

    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        @brief 通过提交器验证候选 flag。
        @param flag_candidate 候选 flag 字符串。
        @return 验证成功返回 True，否则返回 False。
        """
        question = self.current_question
        if question is None:
            logger.warning("当前题目为空，无法提交flag")
            return False

        result = self.submitter.submit(flag_candidate, question)
        return result.success

    def summary_problem(self, problem: str) -> str:
        """
        @brief 对题目进行附件补充和必要摘要。
        @param problem 原始题目描述。
        @return 处理后的题目文本。
        """
        attachment_dir = "./attachments"
        if os.path.isdir(attachment_dir):
            attachment_files = os.listdir(attachment_dir)
            if attachment_files:
                problem += "\n题目包含附件如下："
                for filename in attachment_files:
                    problem += f"\n- {filename}"

        if len(problem) < 256:
            return problem

        prompt = str(self.prompt["problem_summary"]).replace(
            "{question}",
            problem,
        )
        response = self.processor_llm.text_completion(
            optimize_text(prompt),
            json_check=False,
        )
        content = response.choices[0].message.content
        return content if isinstance(content, str) else str(content)
