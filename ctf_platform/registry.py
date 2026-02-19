from typing import Dict, Type
from ctf_platform.base import QuestionInputer, FlagSubmitter

# 内置实现注册表
_inputer_registry: Dict[str, Type[QuestionInputer]] = {}
_submitter_registry: Dict[str, Type[FlagSubmitter]] = {}


def register_inputer(name: str):
    """装饰器：注册题目输入器"""
    def decorator(cls):
        _inputer_registry[name] = cls
        return cls
    return decorator


def register_submitter(name: str):
    """装饰器：注册flag提交器"""
    def decorator(cls):
        _submitter_registry[name] = cls
        return cls
    return decorator


def create_inputer(config: dict) -> QuestionInputer:
    """根据配置创建输入器实例"""
    type_name = config.get("type", "file")
    cls = _inputer_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的输入器类型: {type_name}")
    params = {k: v for k, v in config.items() if k != "type"}
    return cls(**params)


def create_submitter(config: dict, user_interface=None) -> FlagSubmitter:
    """根据配置创建提交器实例"""
    type_name = config.get("type", "manual")
    cls = _submitter_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的提交器类型: {type_name}")
    params = {k: v for k, v in config.items() if k != "type"}
    if type_name == "manual":
        params["user_interface"] = user_interface
    return cls(**params)
