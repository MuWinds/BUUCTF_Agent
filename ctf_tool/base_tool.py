"""@brief 定义工具抽象基类。"""

from abc import ABC, abstractmethod
from typing import Any, Dict


class BaseTool(ABC):
    """@brief 所有工具实现的统一抽象接口。"""

    @abstractmethod
    def execute(self, *args: Any, **kwargs: Any) -> Any:
        """@brief 执行工具操作。

        @param args 位置参数。
        @param kwargs 关键字参数。
        @return 工具执行输出。
        """
        raise NotImplementedError

    @property
    @abstractmethod
    def function_config(self) -> Dict[str, Any]:
        """@brief 返回工具函数调用配置。

        @return Dict[str, Any] 函数调用配置。
        """
        raise NotImplementedError
