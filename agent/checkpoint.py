"""
@brief 解题进度存档管理模块。
"""

import hashlib
import json
import logging
import os
from typing import Any, Dict, List, Optional

logger = logging.getLogger(__name__)


class CheckpointManager:
    """
    @brief 管理解题流程中的存档文件。
    """

    def __init__(self, checkpoint_dir: str = "./checkpoints") -> None:
        """
        @brief 初始化存档目录。
        @param checkpoint_dir 存档目录路径。
        @return None。
        """
        self.checkpoint_dir = checkpoint_dir
        os.makedirs(self.checkpoint_dir, exist_ok=True)

    def _get_path(self, problem: str) -> str:
        """
        @brief 根据题目内容计算存档文件路径。
        @param problem 题目文本。
        @return 存档文件绝对/相对路径。
        """
        md5_hash = hashlib.md5(problem.encode("utf-8")).hexdigest()
        return os.path.join(self.checkpoint_dir, f"ckpt_{md5_hash}.json")

    def save(
        self,
        problem: str,
        step_count: int,
        auto_mode: bool,
        memory_data: Dict[str, Any],
    ) -> None:
        """
        @brief 保存当前解题状态到存档文件。
        @param problem 题目文本。
        @param step_count 当前步骤号。
        @param auto_mode 当前是否为自动模式。
        @param memory_data 记忆模块序列化数据。
        @return None。
        """
        data = {
            "problem": problem,
            "step_count": step_count,
            "auto_mode": auto_mode,
            "memory": memory_data,
        }
        path = self._get_path(problem)
        with open(path, "w", encoding="utf-8") as file:
            json.dump(data, file, ensure_ascii=False, indent=2)
        logger.info("存档已保存: step %s -> %s", step_count, path)

    def load(self, problem: str) -> Optional[Dict[str, Any]]:
        """
        @brief 读取并校验指定题目的存档。
        @param problem 题目文本。
        @return 存档字典；若不存在或无效则返回 None。
        """
        path = self._get_path(problem)
        if not os.path.exists(path):
            return None

        try:
            with open(path, "r", encoding="utf-8") as file:
                data = json.load(file)
            if data.get("problem") != problem:
                logger.warning("存档题目不匹配，忽略")
                return None
            return data
        except (json.JSONDecodeError, IOError) as error:
            logger.error("读取存档失败: %s", error)
            return None

    def exists(self, problem: str) -> bool:
        """
        @brief 检查指定题目的存档是否存在。
        @param problem 题目文本。
        @return 若存在返回 True，否则返回 False。
        """
        return os.path.exists(self._get_path(problem))

    def delete(self, problem: str) -> None:
        """
        @brief 删除指定题目的存档。
        @param problem 题目文本。
        @return None。
        """
        path = self._get_path(problem)
        if os.path.exists(path):
            os.remove(path)
            logger.info("存档已删除: %s", path)

    def list_checkpoints(self) -> List[str]:
        """
        @brief 列出存档目录下所有存档文件名。
        @return 存档文件名列表。
        """
        if not os.path.exists(self.checkpoint_dir):
            return []

        return [
            file_name
            for file_name in os.listdir(self.checkpoint_dir)
            if file_name.startswith("ckpt_") and file_name.endswith(".json")
        ]

    def load_any(self) -> Optional[Dict[str, Any]]:
        """
        @brief 加载首个可用存档（无需指定题目）。
        @return 存档字典；若不存在或读取失败则返回 None。
        """
        files = self.list_checkpoints()
        if not files:
            return None

        path = os.path.join(self.checkpoint_dir, files[0])
        try:
            with open(path, "r", encoding="utf-8") as file:
                data = json.load(file)
            return data
        except (json.JSONDecodeError, IOError) as error:
            logger.error("读取存档失败: %s", error)
            return None
