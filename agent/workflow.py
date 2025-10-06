import logging
from .problem_processor import ProblemProcessor
from .solve_agent import SolveAgent

logger = logging.getLogger(__name__)


class Workflow:
    def __init__(self, config: dict):
        self.config = config
        if self.config is None:
            raise ValueError("配置文件不存在")

    def solve(self, question: str) -> str:
        # 题目预处理
        problem_processor = ProblemProcessor(self.config)
        if len(question) > 128:
            question = problem_processor.summary(question)
        analysis_result = problem_processor.analyze(question)
        logger.info(
            f"题目分类：{analysis_result['category']}\n分析结果：{analysis_result['solution']}"
        )

        # 创建SolveAgent实例并设置flag确认回调
        agent = SolveAgent(self.config, question)
        agent.confirm_flag_callback = self.confirm_flag

        # 将分类和解决思路传递给SolveAgent
        return agent.solve(analysis_result["category"], analysis_result["solution"])

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
