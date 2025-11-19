import litellm
from config import Config
from utils.text import optimize_text
from litellm.utils import ModelResponse, CustomStreamWrapper
from typing import Union


class LLMRequest:
    def __init__(self, model: str):
        config: dict = Config.load_config()
        self.llm_config = config["llm"][model]

    def text_completion(
        self, prompt: str, json_check: bool, **kwargs
    ) -> Union[ModelResponse, CustomStreamWrapper]:
        if json_check == True:
            litellm.enable_json_schema_validation = True
        message = litellm.Message(role="user", content=optimize_text(prompt))
        response = litellm.completion(
            model=self.llm_config["model"],
            api_key=self.llm_config["api_key"],
            api_base=self.llm_config["api_base"],
            messages=[message],
            **kwargs
        )
        return response

    def embedding(
        self, prompt: str, **kwargs
    ) -> Union[ModelResponse, CustomStreamWrapper]:
        message = litellm.Message(role="user", content=optimize_text(prompt))
        response = litellm.embedding(
            model=self.llm_config["model"],
            api_key=self.llm_config["api_key"],
            api_base=self.llm_config["api_base"],
            messages=message,
            **kwargs
        )
        return response
