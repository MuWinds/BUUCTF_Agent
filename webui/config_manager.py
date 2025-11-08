"""Utilities for reading and writing the agent configuration file."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict

CONFIG_PATH = Path("config.json")
TEMPLATE_PATH = Path("config_template.json")


def load_config() -> Dict[str, Any]:
    """Load the current configuration without mutating model names."""
    config_path = CONFIG_PATH if CONFIG_PATH.exists() else TEMPLATE_PATH
    if not config_path.exists():
        raise FileNotFoundError("找不到配置文件或模板：请先创建 config.json")

    with config_path.open("r", encoding="utf-8") as file:
        return json.load(file)


def save_config(config: Dict[str, Any]) -> None:
    """Persist configuration changes back to config.json."""
    CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
    with CONFIG_PATH.open("w", encoding="utf-8") as file:
        json.dump(config, file, indent=4, ensure_ascii=False)
        file.write("\n")


def active_config_path() -> Path:
    """Expose the path currently used when reading configuration."""
    return CONFIG_PATH if CONFIG_PATH.exists() else TEMPLATE_PATH
