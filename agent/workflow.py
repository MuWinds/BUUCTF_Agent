"""解题流程编排模块。"""

import logging
from typing import Callable, Optional

import yaml

from agent.solve_agent import SolveAgent
from ctf_platform.base import FlagSubmitter, Question, QuestionInputer
from ctf_platform.registry import create_inputer, create_submitter
from utils.llm_request import LLMRequest
from utils.text import optimize_text
from utils.user_interface import UserInterface

logger = logging.getLogger(__name__)

# 解题结束回调签名：(question, result) -> None
OnQuestionDone = Callable[[Question, str], None]


class Workflow:
    """负责衔接题目预处理、解题代理与 flag 提交流程。"""

    def __init__(
        self,
        config: dict,
        user_interface: UserInterface,
        inputer: Optional[QuestionInputer] = None,
        submitter: Optional[FlagSubmitter] = None,
        inputer_config: Optional[dict] = None,
        submitter_config: Optional[dict] = None,
    ) -> None:
        """初始化流程编排对象。

        Args:
            config: 全局配置字典。
            user_interface: 用户交互接口实现。
            inputer: 题目输入器。
            submitter: flag 提交器。
            inputer_config: 输入器配置（覆盖 config 中的 platform.inputer）。
            submitter_config: 提交器配置（覆盖 config 中的 platform.submitter）。

        Raises:
            ValueError: 当配置为空时抛出。
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
            inputer_config or platform_config.get("inputer", {"type": "file"})
        )
        self.submitter = submitter or create_submitter(
            submitter_config or platform_config.get("submitter", {"type": "manual"}),
            user_interface=self.user_interface,
        )
        self.current_question: Optional[Question] = None
        self.on_question_done: Optional[OnQuestionDone] = None

    def solve(
        self,
        question: Question,
        resume_data: Optional[dict] = None,
    ) -> str:
        """执行单题解题流程。

        Args:
            question: 题目对象（含题目文本、靶机地址等完整信息）。
            resume_data: 可选存档恢复数据。

        Returns:
            解题结果字符串。
        """
        self.current_question = question
        problem = self.summary_problem(question.content)

        self.agent = SolveAgent(problem, user_interface=self.user_interface)
        self.agent.confirm_flag_callback = self.confirm_flag

        resume_step = 0
        if resume_data:
            resume_step = self.agent.restore_from_checkpoint(resume_data)
            self.user_interface.display_message(
                f"已恢复存档，从第 {resume_step} 步继续"
            )

        result = self.agent.solve(resume_step=resume_step)

        if self.on_question_done is not None:
            self.on_question_done(question, result)

        return result

    def confirm_flag(self, flag_candidate: str) -> bool:
        """通过提交器验证候选 flag。

        Args:
            flag_candidate: 候选 flag 字符串。

        Returns:
            验证成功返回 True，否则返回 False。
        """
        question = self.current_question
        if question is None:
            logger.warning("当前题目为空，无法提交flag")
            return False

        result = self.submitter.submit(flag_candidate, question)
        return result.success

    def summary_problem(self, problem: str) -> str:
        """对题目进行必要摘要。

        Args:
            problem: 原始题目描述。

        Returns:
            处理后的题目文本。
        """
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
