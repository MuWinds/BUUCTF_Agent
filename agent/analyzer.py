import yaml
import json
import litellm
from . import utils
from typing import Dict
from .memory import Memory
from jinja2 import Environment, FileSystemLoader
from .utils import optimize_text

litellm.enable_json_schema_validation = True


class Analyzer:
    def __init__(self, config: dict, problem: str):
        self.config: dict = config
        self.env = Environment(loader=FileSystemLoader("."))
        self.llm_config: dict = self.config["llm"]["analyzer"]
        self.problem = problem
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        litellm.enable_json_schema_validation = True

    def problem_analyze(self):
        prompt = self.prompt["problem_analyze"].replace("{question}", self.problem)
        message = [{"role": "user", "content": optimize_text(prompt)}]
        response = litellm.completion(
            model=self.llm_config.get("model"),
            api_key=self.llm_config.get("api_key"),
            api_base=self.llm_config.get("api_base"),
            messages=message,
        )
        msg_result = response.choices[0].message.content
        try:
            analyze_result = json.loads(msg_result)
        except (json.JSONDecodeError, KeyError) as e:
            msg_result = utils.fix_json_with_llm(msg_result, e)
            analyze_result = json.loads(msg_result)

        return analyze_result

    def analyze_step_output(
        self,
        memory: Memory,
        step_num: int,
        content: str,
        output: str,
        solution_plan: str,
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
        history_summary = memory.get_summary()

        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("step_analysis", ""))
        prompt = template.render(
            question=self.problem,
            step_num=step_num,
            content=content,
            output=output[:4096],
            solution_plan=solution_plan,
            history_summary=history_summary,
        )

        # 调用LLM进行分析
        response = litellm.completion(
            model=self.llm_config["model"],
            api_key=self.llm_config["api_key"],
            api_base=self.llm_config["api_base"],
            messages=[{"role": "user", "content": optimize_text(prompt)}],
        )
        # 解析分析结果
        try:
            result = json.loads(response.choices[0].message.content)
            if isinstance(result, dict):
                return result
        except (json.JSONDecodeError, KeyError) as e:
            content = utils.fix_json_with_llm(response.choices[0].message.content, e)
            result = json.loads(content)
            return result
