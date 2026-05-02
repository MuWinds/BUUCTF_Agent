"""输入器、提交器与平台注册表及工厂函数。"""

import importlib
import pkgutil
from pathlib import Path
from typing import Any, Callable, Dict, Optional, Type, TypeVar

from ctf_platform.base import FlagSubmitter, Platform, QuestionInputer

_inputer_registry: Dict[str, Type[QuestionInputer]] = {}
_submitter_registry: Dict[str, Type[FlagSubmitter]] = {}
_platform_registry: Dict[str, Type[Platform]] = {}
# 平台类型 → 对应的 CLI 命令名
_platform_cli_map: Dict[str, str] = {}

_I = TypeVar("_I", bound=QuestionInputer)
_S = TypeVar("_S", bound=FlagSubmitter)
_P = TypeVar("_P", bound=Platform)

# 不作为平台模块自动导入的文件
_SKIP_MODULES = {"__init__", "base", "registry"}


def _auto_discover() -> None:
    """自动发现并导入 ctf_platform 包内的所有平台模块。

    导入后各模块中的 @register_* 装饰器会自动执行注册。
    """
    package_dir = Path(__file__).parent
    for module_info in pkgutil.iter_modules([str(package_dir)]):
        if module_info.name not in _SKIP_MODULES:
            importlib.import_module(f"ctf_platform.{module_info.name}")


def register_inputer(
    name: str,
) -> Callable[[Type[_I]], Type[_I]]:
    """注册题目输入器类的装饰器。

    Args:
        name: 输入器类型名。

    Returns:
        装饰器函数。
    """

    def decorator(cls: Type[_I]) -> Type[_I]:
        _inputer_registry[name] = cls
        return cls

    return decorator


def register_submitter(
    name: str,
) -> Callable[[Type[_S]], Type[_S]]:
    """注册 Flag 提交器类的装饰器。

    Args:
        name: 提交器类型名。

    Returns:
        装饰器函数。
    """

    def decorator(cls: Type[_S]) -> Type[_S]:
        _submitter_registry[name] = cls
        return cls

    return decorator


def register_platform(
    name: str,
) -> Callable[[Type[_P]], Type[_P]]:
    """注册平台编排器类的装饰器。

    Args:
        name: 平台类型名。

    Returns:
        装饰器函数。
    """

    def decorator(cls: Type[_P]) -> Type[_P]:
        _platform_registry[name] = cls
        return cls

    return decorator


def create_inputer(config: Dict[str, Any]) -> QuestionInputer:
    """根据配置创建输入器实例。

    Args:
        config: 输入器配置，至少包含 type 字段。

    Returns:
        输入器实例。

    Raises:
        ValueError: 当输入器类型未知时抛出。
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
    """根据配置创建提交器实例。

    Args:
        config: 提交器配置，至少包含 type 字段。
        user_interface: 用户交互接口实例（manual 类型需要）。

    Returns:
        提交器实例。

    Raises:
        ValueError: 当提交器类型未知时抛出。
    """
    type_name = config.get("type", "manual")
    cls = _submitter_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的提交器类型: {type_name}")

    params = {key: value for key, value in config.items() if key != "type"}
    if type_name == "manual":
        params["user_interface"] = user_interface

    return cls(**params)


def create_platform(
    name: str,
    inputer: QuestionInputer,
    submitter: FlagSubmitter,
    user_interface: Any,
) -> Platform:
    """根据名称创建平台编排器实例。

    Args:
        name: 平台类型名。
        inputer: 题目输入器。
        submitter: Flag 提交器。
        user_interface: 用户交互接口。

    Returns:
        平台编排器实例。

    Raises:
        ValueError: 当平台类型未知时抛出。
    """
    cls = _platform_registry.get(name)
    if cls is None:
        raise ValueError(f"未知的平台类型: {name}")

    return cls(inputer=inputer, submitter=submitter, user_interface=user_interface)


def register_platform_cli(platform_type: str, cli_command: str) -> None:
    """注册平台类型到 CLI 命令名的映射。

    Args:
        platform_type: 平台类型名（与 @register_platform 使用的一致）。
        cli_command: 对应的 CLI 子命令名。
    """
    _platform_cli_map[platform_type] = cli_command


def get_platform_cli(platform_type: str) -> Optional[str]:
    """查询平台类型对应的 CLI 命令名。

    Args:
        platform_type: 平台类型名。

    Returns:
        CLI 命令名，未注册则返回 None。
    """
    return _platform_cli_map.get(platform_type)


def get_all_platform_cli() -> Dict[str, str]:
    """返回所有已注册的平台 CLI 命令映射。"""
    return dict(_platform_cli_map)
