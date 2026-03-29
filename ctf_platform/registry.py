"""@brief 输入器与提交器注册表及工厂函数。"""

from typing import Any, Callable, Dict, Optional, Type

from ctf_platform.base import FlagSubmitter, QuestionInputer

_inputer_registry: Dict[str, Type[QuestionInputer]] = {}
_submitter_registry: Dict[str, Type[FlagSubmitter]] = {}


def register_inputer(
    name: str,
) -> Callable[[Type[QuestionInputer]], Type[QuestionInputer]]:
    """@brief 注册题目输入器类的装饰器。

    @param name 输入器类型名。
    @return Callable 装饰器函数。
    """

    def decorator(cls: Type[QuestionInputer]) -> Type[QuestionInputer]:
        _inputer_registry[name] = cls
        return cls

    return decorator


def register_submitter(
    name: str,
) -> Callable[[Type[FlagSubmitter]], Type[FlagSubmitter]]:
    """@brief 注册 Flag 提交器类的装饰器。

    @param name 提交器类型名。
    @return Callable 装饰器函数。
    """

    def decorator(cls: Type[FlagSubmitter]) -> Type[FlagSubmitter]:
        _submitter_registry[name] = cls
        return cls

    return decorator


def create_inputer(config: Dict[str, Any]) -> QuestionInputer:
    """@brief 根据配置创建输入器实例。

    @param config 输入器配置，至少包含 type 字段。
    @return QuestionInputer 输入器实例。
    @raises ValueError 当输入器类型未知时抛出。
    """
    type_name = config.get("type", "file")
    cls = _inputer_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的输入器类型: {type_name}")

    params = {key: value for key, value in config.items() if key != "type"}
    return cls(**params)


def create_submitter(
    config: Dict[str, Any],
    user_interface: Optional[Any] = None,
) -> FlagSubmitter:
    """@brief 根据配置创建提交器实例。

    @param config 提交器配置，至少包含 type 字段。
    @param user_interface 用户交互接口实例（manual 类型需要）。
    @return FlagSubmitter 提交器实例。
    @raises ValueError 当提交器类型未知时抛出。
    """
    type_name = config.get("type", "manual")
    cls = _submitter_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的提交器类型: {type_name}")

    params = {key: value for key, value in config.items() if key != "type"}
    if type_name == "manual":
        params["user_interface"] = user_interface

    return cls(**params)
