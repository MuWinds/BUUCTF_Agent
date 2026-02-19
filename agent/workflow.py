import logging
import litellm
import yaml
import os
from agent.solve_agent import SolveAgent
from utils.text import optimize_text
from utils.user_interface import UserInterface, CommandLineInterface
from ctf_platform.base import QuestionInputer, FlagSubmitter, Question
from ctf_platform.registry import create_inputer, create_submitter

logger = logging.getLogger(__name__)


class Workflow:
    def __init__(self, config: dict, user_interface: UserInterface = None,
                 inputer: QuestionInputer = None, submitter: FlagSubmitter = None):
        self.config = config
        self.processor_llm: dict = self.config["llm"]["pre_processor"]
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        self.user_interface = user_interface or CommandLineInterface()
        if self.config is None:
            raise ValueError("配置文件不存在")

        # 通过参数传入或从配置创建
        platform_config = config.get("platform", {})
        self.inputer = inputer or create_inputer(
            platform_config.get("inputer", {"type": "file"})
        )
        self.submitter = submitter or create_submitter(
            platform_config.get("submitter", {"type": "manual"}),
            user_interface=self.user_interface
        )
        self.current_question: Question = None

    def solve(self, problem: str, resume_data: dict = None,
              question: "Question | None" = None) -> str:
        # 记录当前题目，供 submitter 使用
        self.current_question = question

        problem = self.summary_problem(problem)

        # 创建SolveAgent实例并设置flag确认回调和用户接口
        self.agent = SolveAgent(problem, user_interface=self.user_interface)
        self.agent.confirm_flag_callback = self.confirm_flag

        # 如果有存档数据，恢复状态
        resume_step = 0
        if resume_data:
            resume_step = self.agent.restore_from_checkpoint(resume_data)
            self.user_interface.display_message(f"已恢复存档，从第 {resume_step} 步继续")

        # 将分类和解决思路传递给SolveAgent
        result = self.agent.solve(resume_step=resume_step)

        return result

    def confirm_flag(self, flag_candidate: str) -> bool:
        """使用submitter提交flag并返回结果"""
        result = self.submitter.submit(flag_candidate, self.current_question)
        return result.success

    def summary_problem(self, problem: str) -> str:
        """
        总结题目
        """
        if len(os.listdir("./attachments")) > 0:
            problem += "\n题目包含附件如下："
            for filename in os.listdir("./attachments"):
                problem += f"\n- {filename}"
        if len(problem) < 256:
            return problem
        prompt = str(self.prompt["problem_summary"]).replace("{question}", problem)
        message = litellm.Message(role="user", content=optimize_text(prompt))
        response = litellm.completion(
            model=self.processor_llm["model"],
            api_key=self.processor_llm["api_key"],
            api_base=self.processor_llm["api_base"],
            messages=[message],
        )
        return response.choices[0].message.content
