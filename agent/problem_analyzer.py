import litellm
import yaml
import json
class ProblemAnalyzer:
    def __init__(self, config: dict):
        self.config = config
        self.prompt = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None or len(self.config) == 0:
            raise ValueError("配置文件不存在")

    def analyze(self, question: str) -> str:
        prompt = self.prompt["problem_analyze"].replace("{question}", question)
        message = litellm.Message(role="user", content=prompt)
        response = litellm.completion(
            model=self.config.get("model"),
            api_key=self.config.get("api_key"),
            api_base=self.config.get("api_base"),
            messages=[message],
        )
        msg_result = response.choices[0].message.content
        analyze_result = json.loads(msg_result)
        print("题目分析结果：\n"+"分类："+analyze_result['category']+"\n分析："+analyze_result['solution'])
        return msg_result