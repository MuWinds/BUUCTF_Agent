# -*- coding: utf-8 -*-
# 工具基类
from abc import ABC, abstractmethod
from typing import Dict, Tuple


class BaseTool(ABC):
    @abstractmethod
    def execute(self, *args, **kwargs) -> Tuple[str, str]:
        """执行工具操作"""
        pass

    @property
    @abstractmethod
    def function_config(self) -> Dict:
        """返回工具的函数调用配置"""
        pass
