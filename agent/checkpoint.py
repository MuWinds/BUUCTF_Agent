import os
import json
import hashlib
import logging
from typing import Optional, List, Dict, Any

logger = logging.getLogger(__name__)


class CheckpointManager:
    """解题进度存档管理器"""

    def __init__(self, checkpoint_dir: str = "./checkpoints"):
        self.checkpoint_dir = checkpoint_dir
        os.makedirs(self.checkpoint_dir, exist_ok=True)

    def _get_path(self, problem: str) -> str:
        """根据题目生成存档文件路径"""
        md5 = hashlib.md5(problem.encode("utf-8")).hexdigest()
        return os.path.join(self.checkpoint_dir, f"ckpt_{md5}.json")

    def save(
        self,
        problem: str,
        step_count: int,
        auto_mode: bool,
        memory_data: Dict[str, Any],
    ) -> None:
        """保存存档"""
        data = {
            "problem": problem,
            "step_count": step_count,
            "auto_mode": auto_mode,
            "memory": memory_data,
        }
        path = self._get_path(problem)
        with open(path, "w", encoding="utf-8") as f:
            json.dump(data, f, ensure_ascii=False, indent=2)
        logger.info(f"存档已保存: step {step_count} -> {path}")

    def load(self, problem: str) -> Optional[Dict[str, Any]]:
        """读取并校验存档"""
        path = self._get_path(problem)
        if not os.path.exists(path):
            return None
        try:
            with open(path, "r", encoding="utf-8") as f:
                data = json.load(f)
            if data.get("problem") != problem:
                logger.warning("存档题目不匹配，忽略")
                return None
            return data
        except (json.JSONDecodeError, IOError) as e:
            logger.error(f"读取存档失败: {e}")
            return None

    def exists(self, problem: str) -> bool:
        """检查存档是否存在"""
        return os.path.exists(self._get_path(problem))

    def delete(self, problem: str) -> None:
        """删除存档"""
        path = self._get_path(problem)
        if os.path.exists(path):
            os.remove(path)
            logger.info(f"存档已删除: {path}")

    def list_checkpoints(self) -> List[str]:
        """列出所有存档文件"""
        if not os.path.exists(self.checkpoint_dir):
            return []
        return [
            f for f in os.listdir(self.checkpoint_dir) if f.startswith("ckpt_") and f.endswith(".json")
        ]

    def load_any(self) -> Optional[Dict[str, Any]]:
        """加载第一个可用的存档（不需要指定题目）

        Returns:
            存档数据，如果没有存档则返回 None
        """
        files = self.list_checkpoints()
        if not files:
            return None
        path = os.path.join(self.checkpoint_dir, files[0])
        try:
            with open(path, "r", encoding="utf-8") as f:
                data = json.load(f)
            return data
        except (json.JSONDecodeError, IOError) as e:
            logger.error(f"读取存档失败: {e}")
            return None
