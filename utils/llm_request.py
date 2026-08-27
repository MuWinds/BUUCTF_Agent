"""LLM 请求封装模块，提供文本补全和向量化接口。"""

import logging
from typing import Any, Dict, List, Optional, cast

from openai import OpenAI

from config import Config
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class LLMRequest:
    """LLM 请求封装器。

    负责读取配置、初始化 OpenAI 客户端，并提供文本补全和向量化接口。
    """

    def __init__(self, model: str):
        """初始化 LLMRequest 实例。

        Args:
            model: 模型名或角色别名。

        Raises:
            KeyError: 当配置缺少必需字段时抛出。
        """
        config: dict = Config.load_config()

        model_alias = {
            "analyzer": "solve_agent",
            "pre_processor": "solve_agent",
        }
        resolved_model = model_alias.get(model, model)

        llm_config = config["llm"]
        if isinstance(llm_config, dict) and "model" in llm_config:
            self.llm_config = llm_config
        else:
            self.llm_config = llm_config[resolved_model]

        self.client = OpenAI(
            api_key=self.llm_config.get("api_key"),
            base_url=self.llm_config.get("api_base"),
        )

    def text_completion(
        self, prompt: str, json_check: bool, **kwargs: Any
    ) -> Any:
        """发起文本补全请求。

        Args:
            prompt: 用户提示词。
            json_check: 是否启用 JSON 输出约束。
            kwargs: 透传给 OpenAI chat.completions.create 的额外参数。

        Returns:
            OpenAI 原始响应对象。
        """
        request_kwargs = dict(kwargs)

        has_tools = (
            "tools" in request_kwargs and request_kwargs.get("tools") is not None
        )
        if json_check and not has_tools and "response_format" not in request_kwargs:
            request_kwargs["response_format"] = {"type": "json_object"}

        response = self.client.chat.completions.create(
            model=self.llm_config["model"],
            messages=[{"role": "user", "content": optimize_text(prompt)}],
            **request_kwargs,
        )
        logger.debug("LLM Response Message: %s", response.choices[0].message.content)
        return response

    def chat_completion(
        self,
        messages: List[Dict[str, Any]],
        tools: Optional[List[Dict[str, Any]]] = None,
        tool_choice: str = "auto",
    ) -> Any:
        """使用完整消息列表发起对话补全请求。

        Args:
            messages: OpenAI 格式的消息列表。
            tools: 工具定义列表。
            tool_choice: 工具选择策略。

        Returns:
            OpenAI 原始响应对象。
        """
        kwargs: Dict[str, Any] = {}
        if tools:
            kwargs["tools"] = tools
            kwargs["tool_choice"] = tool_choice

        response = self.client.chat.completions.create(
            model=self.llm_config["model"],
            messages=cast(Any, messages),
            **kwargs,
        )
        logger.debug("LLM Response Message: %s", response.choices[0].message.content)
        return response
