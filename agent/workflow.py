import logging
import litellm
import yaml
import os
from threading import Event
from typing import Callable, Optional
from .analyzer import Analyzer
from .solve_agent import SolveAgent
from .utils import optimize_text

logger = logging.getLogger(__name__)


class Workflow:
    def __init__(self, config: dict):
        self.config = config
        self.processor_llm: dict = self.config["llm"]["pre_processor"]
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None:
            raise ValueError("配置文件不存在")

    def solve(
        self,
        problem: str,
        auto_mode: Optional[bool] = None,
        event_callback: Optional[Callable[[dict], None]] = None,
        stop_event: Optional[Event] = None,
        confirm_handler: Optional[Callable[[str], bool]] = None,
    ) -> str:
        if event_callback:
            event_callback({"type": "problem_received", "content": problem})

        problem = self.summary_problem(problem)
        if event_callback:
            event_callback({"type": "problem_summary", "content": problem})

        #  分析题目
        analyzer = Analyzer(self.config, problem)
        analysis_result = analyzer.problem_analyze()
        logger.info(
            f"题目分类：{analysis_result['category']}\n分析结果：{analysis_result['solution']}"
        )
        if event_callback:
            event_callback(
                {
                    "type": "analysis_complete",
                    "analysis": analysis_result,
                }
            )

        # 创建SolveAgent实例并设置flag确认回调
        agent = SolveAgent(
            self.config,
            problem,
            auto_mode=auto_mode,
            event_callback=event_callback,
            stop_event=stop_event,
        )
        agent.confirm_flag_callback = (
            confirm_handler if confirm_handler else self.confirm_flag
        )

        # 将分类和解决思路传递给SolveAgent
        result = agent.solve(
            analysis_result["category"], analysis_result["solution"]
        )

        if event_callback:
            event_callback({"type": "solve_finished", "result": result})

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
        if len(problem) < 128:
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
