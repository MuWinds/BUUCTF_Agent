import logging
import litellm
import yaml
import os
from agent.solve_agent import SolveAgent
from utils.text import optimize_text
from utils.user_interface import UserInterface, CommandLineInterface

logger = logging.getLogger(__name__)


class Workflow:
    def __init__(self, config: dict, user_interface: UserInterface = None):
        self.config = config
        self.processor_llm: dict = self.config["llm"]["pre_processor"]
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        self.user_interface = user_interface or CommandLineInterface()
        if self.config is None:
            raise ValueError("配置文件不存在")

    def solve(self, problem: str) -> str:
        problem = self.summary_problem(problem)

        # 创建SolveAgent实例并设置flag确认回调和用户接口
        self.agent = SolveAgent(problem, user_interface=self.user_interface)
        self.agent.confirm_flag_callback = self.confirm_flag

        # 将分类和解决思路传递给SolveAgent
        result = self.agent.solve()

        return result

    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        让用户确认flag是否正确
        :param flag_candidate: 候选flag
        :return: 用户确认结果
        """
        return self.user_interface.confirm_flag(flag_candidate)

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
