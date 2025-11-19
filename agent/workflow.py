import logging
import litellm
import yaml
import os
from agent.solve_agent import SolveAgent
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class Workflow:
    def __init__(self, config: dict):
        self.config = config
        self.processor_llm: dict = self.config["llm"]["pre_processor"]
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None:
            raise ValueError("配置文件不存在")

    def solve(self, problem: str) -> str:
        problem = self.summary_problem(problem)

        # 创建SolveAgent实例并设置flag确认回调
        agent = SolveAgent(problem)
        agent.confirm_flag_callback = self.confirm_flag

        # 将分类和解决思路传递给SolveAgent
        result = agent.solve()

        return result

    def confirm_flag(self, flag_candidate: str) -> bool:
        """
        让用户确认flag是否正确
        :param flag_candidate: 候选flag
        :return: 用户确认结果
        """
        print(f"\n发现flag：\n{flag_candidate}")
        print("请确认这个flag是否正确？")

        while True:
            response = input("输入 'y' 确认正确，输入 'n' 表示不正确: ").strip().lower()
            if response == "y":
                return True
            elif response == "n":
                return False
            else:
                print("无效输入，请输入 'y' 或 'n'")

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
