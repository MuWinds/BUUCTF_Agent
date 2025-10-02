import json
import os


class Config:
    def __init__(self, config_path="./config.json"):
        self.config_path = config_path
        self.config = self.load_config()

    @classmethod
    def load_config(cls, config_path="./config.json") -> dict:
        if os.path.exists(config_path):
            with open(config_path, "r", encoding="utf-8") as f:
                try:
                    return json.load(f)
                except json.JSONDecodeError:
                    raise ValueError(f"配置文件 {config_path} 不是有效的JSON格式")
        else:
            os.makedirs(os.path.dirname(config_path), exist_ok=True)
            default_config = {
                "model": "",
                "api_key": "",
                "api_base": "",
                "tool_config": {},
            }
            with open(config_path, "w", encoding="utf-8") as f:
                json.dump(default_config, f, indent=4)
            return default_config

    @classmethod
    def get_tool_config(cls, tool_name: str, config_path="./config.json") -> dict:
        config = cls.load_config(config_path)
        return config["tool_config"]

    def get(self, key, default=None):
        return self.config.get(key, default)

    def set(self, key, value):
        self.config[key] = value
        with open(self.config_path, "w", encoding="utf-8") as f:
            json.dump(self.config, f, indent=4)
