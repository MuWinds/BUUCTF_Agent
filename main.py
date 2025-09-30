from config import Config
from agent.workflow import Workflow

if __name__ == "__main__":
    config:dict = Config.load_config()
    question = str(input("输入ctf的题目标题和内容:\n"))
    Workflow(config=config).solve(question)