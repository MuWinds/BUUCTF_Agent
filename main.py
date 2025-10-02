from config import Config
from agent.workflow import Workflow
import sys

if __name__ == "__main__":
    config: dict = Config.load_config()
    print("输入题目标题和内容，支持多行输入（输入完成后按 Ctrl+D 或 Ctrl+Z 结束）：")
    question = sys.stdin.read().strip()
    Workflow(config=config).solve(question)
