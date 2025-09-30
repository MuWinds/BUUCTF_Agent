import json
from .problem_analyzer import ProblemAnalyzer
from .solve_agent import SolveAgent

class Workflow:
    def __init__(self, config: dict):
        self.config = config
        if self.config is None:
            raise ValueError("配置文件不存在")

    def solve(self, question: str) -> str:
        # 获取题目分析结果
        analysis_res = ProblemAnalyzer(self.config).analyze(question)
        # analysis_res = """{"category": "分类", "solution": "解决思路"}"""
        analysis_result = json.loads(analysis_res)
        
        # 添加模式选择
        self.select_mode()
        
        # 创建SolveAgent实例并设置flag确认回调
        agent = SolveAgent(self.config)
        agent.confirm_flag_callback = self.confirm_flag
        
        # 将分类和解决思路传递给SolveAgent
        return agent.solve(analysis_result["category"], analysis_result["solution"])
    
    def select_mode(self):
        """让用户选择运行模式"""
        print("\n请选择运行模式:")
        print("1. 自动模式（Agent自动生成和执行所有命令）")
        print("2. 手动模式（每一步需要用户批准）")
        
        while True:
            choice = input("请输入选项编号: ").strip()
            if choice == "1":
                self.config["auto_mode"] = True
                print("已选择自动模式")
                return
            elif choice == "2":
                self.config["auto_mode"] = False
                print("已选择手动模式")
                return
            else:
                print("无效选项，请重新选择")

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
            if response == 'y':
                return True
            elif response == 'n':
                return False
            else:
                print("无效输入，请输入 'y' 或 'n'")