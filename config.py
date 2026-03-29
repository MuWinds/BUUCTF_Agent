"""
@brief 项目配置管理模块。
"""

import json
import os
from typing import Any, Dict


class Config:
    """
    @brief 配置加载与访问工具类。
    """

    def __init__(self, config_path: str = "./config.json") -> None:
        """
        @brief 初始化配置对象并加载配置内容。
        @param config_path 配置文件路径。
        @raises ValueError 当配置文件不存在或 JSON 非法时抛出。
        """
        self.config_path = config_path
        self.config = self.load_config()

    @classmethod
    def load_config(cls, config_path: str = "./config.json") -> Dict[str, Any]:
        """
        @brief 从磁盘读取配置并做兼容性规范化处理。
        @param config_path 配置文件路径。
        @return 解析后的配置字典。
        @raises ValueError 当配置文件不存在或 JSON 非法时抛出。
        """
        if os.path.exists(config_path):
            with open(config_path, "r", encoding="utf-8") as file:
                try:
                    config: Dict[str, Any] = json.load(file)

                    if "llm" in config and isinstance(config["llm"], dict):
                        llm_config = config["llm"]

                        if "model" in llm_config and isinstance(
                            llm_config["model"], str
                        ):
                            model_name = llm_config["model"].strip()
                            if model_name.startswith("openai/"):
                                model_name = model_name[len("openai/") :]
                            llm_config["model"] = model_name
                        else:
                            for agent in llm_config.values():
                                if (
                                    isinstance(agent, dict)
                                    and "model" in agent
                                    and isinstance(agent["model"], str)
                                ):
                                    model_name = agent["model"].strip()
                                    if model_name.startswith("openai/"):
                                        model_name = model_name[
                                            len("openai/") :
                                        ]
                                    agent["model"] = model_name

                    return config
                except json.JSONDecodeError as error:
                    raise ValueError(
                        f"配置文件 {config_path} 不是有效的JSON格式"
                    ) from error

        raise ValueError(f"配置文件 {config_path} 不存在")

    @classmethod
    def get_tool_config(
        cls,
        tool_name: str,
        config_path: str = "./config.json",
    ) -> Dict[str, Any]:
        """
        @brief 获取指定工具的配置项。
        @param tool_name 工具名称。
        @param config_path 配置文件路径。
        @return 工具配置字典。
        """
        config = cls.load_config(config_path)
        return config["tool_config"][tool_name]

    def get(self, key: str, default: Any = None) -> Any:
        """
        @brief 获取配置值。
        @param key 配置键。
        @param default 键不存在时返回的默认值。
        @return 配置键对应的值或默认值。
        """
        return self.config.get(key, default)

    def set(self, key: str, value: Any) -> None:
        """
        @brief 设置配置值并持久化到配置文件。
        @param key 配置键。
        @param value 配置值。
        @return None。
        """
        self.config[key] = value
        with open(self.config_path, "w", encoding="utf-8") as file:
            json.dump(self.config, file, indent=4)
