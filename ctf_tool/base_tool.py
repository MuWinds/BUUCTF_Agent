"""定义工具抽象基类。"""

from abc import ABC, abstractmethod
from typing import Any, Dict


class BaseTool(ABC):
    """所有工具实现的统一抽象接口。"""

    @abstractmethod
    def execute(self, *args: Any, **kwargs: Any) -> Any:
        """执行工具操作。

        Args:
            args: 位置参数。
            kwargs: 关键字参数。

        Returns:
            工具执行输出。
        """
        raise NotImplementedError

    @property
    @abstractmethod
    def function_config(self) -> Dict[str, Any]:
        """返回工具函数调用配置。

        Returns:
            函数调用配置。
        """
        raise NotImplementedError
