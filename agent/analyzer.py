"""
@brief 步骤输出分析模块。
"""

import json
from typing import Any, Dict

import yaml
from jinja2 import Environment, FileSystemLoader

from agent.memory import Memory
from utils.llm_request import LLMRequest
from utils.text import fix_json_with_llm


class Analyzer:
    """
    @brief 基于 LLM 对单步执行结果进行分析。
    """

    def __init__(self, config: Dict[str, Any], problem: str) -> None:
        """
        @brief 初始化分析器。
        @param config 全局配置字典。
        @param problem 当前题目描述。
        @return None。
        """
        self.config: Dict[str, Any] = config
        self.env = Environment(loader=FileSystemLoader("."))
        self.analyze_llm = LLMRequest("solve_agent")
        self.problem = problem
        with open("./prompt.yaml", "r", encoding="utf-8") as file:
            self.prompt: Dict[str, Any] = yaml.safe_load(file)

    def analyze_step_output(
        self,
        memory: Memory,
        think: str,
        content: str,
        output: str,
    ) -> Dict[str, Any]:
        """
        @brief 使用 LLM 分析步骤输出并返回结构化结果。
        @param memory 记忆对象，用于提供历史摘要。
        @param think 当前步骤思考内容。
        @param content 当前步骤执行内容。
        @param output 当前步骤输出内容。
        @return 分析结果字典。
        @raises json.JSONDecodeError 当修复后的结果仍无法解析时抛出。
        """
        history_summary = memory.get_summary()

        template = self.env.from_string(self.prompt.get("step_analysis", ""))
        prompt = template.render(
            question=self.problem,
            content=content,
            output=output[:4096],
            think=think,
            history_summary=history_summary,
        )

        response = self.analyze_llm.text_completion(prompt, json_check=True)
        content = response.choices[0].message.content
        content_text = content if isinstance(content, str) else str(content)

        try:
            result = json.loads(content_text)
            if isinstance(result, dict):
                return result
        except (json.JSONDecodeError, KeyError) as error:
            fixed_content = fix_json_with_llm(
                content_text,
                str(error),
            )
            fixed_result = json.loads(fixed_content)
            if isinstance(fixed_result, dict):
                return fixed_result

        return {}

