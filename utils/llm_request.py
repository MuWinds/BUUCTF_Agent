import litellm
import logging
from config import Config
from utils.text import optimize_text
from litellm.utils import ModelResponse, CustomStreamWrapper
from typing import Union

logger = logging.getLogger(__name__)

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
        logger.debug(f"LLM Response Message: {response.choices[0].message.content}")
        return response

    def embedding(
        self, text: str, **kwargs
    ) -> Union[ModelResponse, CustomStreamWrapper]:
        response = litellm.embedding(
            model=self.llm_config["model"],
            api_key=self.llm_config["api_key"],
            api_base=self.llm_config["api_base"],
            input=text,
            **kwargs
        )
        return response
