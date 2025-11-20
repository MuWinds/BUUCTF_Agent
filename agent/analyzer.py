import yaml
import json
from typing import Dict
from agent.memory import Memory
from utils.llm_request import LLMRequest
from jinja2 import Environment, FileSystemLoader
from utils.text import fix_json_with_llm


class Analyzer:
    def __init__(self, config: dict, problem: str):
        self.config: dict = config
        self.env = Environment(loader=FileSystemLoader("."))
        self.analyze_llm = LLMRequest("analyzer")
        self.problem = problem
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))

    def analyze_step_output(
        self, memory: Memory, think: str, content: str, output: str
    ) -> Dict:
        """
        使用LLM分析步骤输出
        :param step_num: 步骤编号
        :param content: 执行的内容
        :param output: 命令输出
        :param solution_plan: 解题思路
        :return: 分析结果字典
        """
        # 获取记忆摘要
        history_summary = memory.get_summary(current_problem=self.problem)

        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("step_analysis", ""))
        prompt = template.render(
            question=self.problem,
            content=content,
            output=output[:4096],
            think=think,
            history_summary=history_summary,
        )

        # 调用LLM进行分析
        response = self.analyze_llm.text_completion(prompt, json_check=True)
        # 解析分析结果
        try:
            result = json.loads(response.choices[0].message.content)
            if isinstance(result, dict):
                return result
        except (json.JSONDecodeError, KeyError) as e:
            content = fix_json_with_llm(response.choices[0].message.content, e)
            result = json.loads(content)
            return result
