# problem_analyzer.py
import litellm
import yaml
import json
from . import utils  # 同级包导入


class ProblemProcessor:
    def __init__(self, config: dict):
        self.config = config
        litellm.enable_json_schema_validation = True
        with open("./prompt.yaml", "r", encoding="utf-8") as f:
            self.prompt:dict = yaml.safe_load(f)
        if not self.config:
            raise ValueError("配置文件不存在")
        
    def summary(self,question:str) -> str:
        """
        总结题目
        """
        prompt = self.prompt["problem_summary"].replace("{question}", question)
        message = litellm.Message(role="user", content=prompt)
        response = litellm.completion(
            model=self.config.get("model"),
            api_key=self.config.get("api_key"),
            api_base=self.config.get("api_base"),
            messages=[message],
        )
        return response.choices[0].message.content

    def analyze(self, question: str) -> str:
        """
        分析题目
        :param question: 题干
        :param max_fix_round: 最多修复次数
        :return: 原始 LLM 返回字符串
        """
        prompt = self.prompt["problem_analyze"].replace("{question}", question)
        message = litellm.Message(role="user", content=prompt)
        response = litellm.completion(
            model=self.config.get("model"),
            api_key=self.config.get("api_key"),
            api_base=self.config.get("api_base"),
            messages=[message],
        )
        msg_result = response.choices[0].message.content

        # 尝试解析 + 自动修复
        while True:
            try:
                analyze_result = json.loads(msg_result)
                if isinstance(analyze_result,dict):
                    return analyze_result
            except (json.JSONDecodeError, KeyError) as e:
                # 调用 utils 修复
                print("修复json中：" + str(e))
                print(msg_result)
                msg_result = utils.fix_json_with_llm(msg_result,err_content=e,config=self.config)
