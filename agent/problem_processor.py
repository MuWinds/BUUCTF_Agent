import litellm
import yaml
from .utils import optimize_text


class ProblemProcessor:
    def __init__(self, config: dict):
        self.config = config
        self.llm_config: dict = self.config["llm"]["problem_processor"]
        litellm.enable_json_schema_validation = True
        with open("./prompt.yaml", "r", encoding="utf-8") as f:
            self.prompt: dict = yaml.safe_load(f)
        if not self.config:
            raise ValueError("配置文件不存在")

    def summary(self, question: str) -> str:
        """
        总结题目
        """
        if len(question) < 128:
            return question
        prompt = self.prompt["problem_summary"].replace("{question}", question)
        message = litellm.Message(role="user", content=optimize_text(prompt))
        response = litellm.completion(
            model=self.llm_config.get("model"),
            api_key=self.llm_config.get("api_key"),
            api_base=self.llm_config.get("api_base"),
            messages=message,
        )
        return response.choices[0].message.content
