"""
@brief 解题进度存档管理模块。
"""

import json
import logging
import os
from datetime import datetime
from typing import Any, Dict, List, Optional

logger = logging.getLogger(__name__)


class CheckpointManager:
    """
    @brief 管理解题流程中的存档文件。
    """

    def __init__(self, checkpoint_dir: str = "./checkpoints") -> None:
        self.checkpoint_dir = checkpoint_dir
        os.makedirs(self.checkpoint_dir, exist_ok=True)

    def save(self, messages: List[Dict[str, Any]], problem: str = "") -> None:
        """
        @brief 保存当前对话消息列表到存档文件。
        @param messages OpenAI 格式的消息列表。
        @param problem 题目文本（用于元数据）。
        """
        # 将消息列表中的不可序列化对象转换为 dict
        serializable_msgs = []
        for msg in messages:
            if isinstance(msg, dict):
                serializable_msgs.append(msg)
            elif hasattr(msg, "model_dump"):
                serializable_msgs.append(msg.model_dump())
            else:
                serializable_msgs.append(str(msg))

        data = {
            "problem": problem,
            "messages": serializable_msgs,
            "saved_at": datetime.now().isoformat(),
        }
        timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
        path = os.path.join(self.checkpoint_dir, f"ckpt_{timestamp}.json")
        with open(path, "w", encoding="utf-8") as file:
            json.dump(data, file, ensure_ascii=False, indent=2)
        logger.info("存档已保存: %s", path)

    def load_latest(self) -> Optional[Dict[str, Any]]:
        """
        @brief 加载最新的存档。
        @return 存档字典；若不存在则返回 None。
        """
        files = self.list_checkpoints()
        if not files:
            return None

        # 按文件名排序取最新
        files.sort(reverse=True)
        path = os.path.join(self.checkpoint_dir, files[0])
        try:
            with open(path, "r", encoding="utf-8") as file:
                return json.load(file)
        except (json.JSONDecodeError, IOError) as error:
            logger.error("读取存档失败: %s", error)
            return None

    def delete_latest(self) -> None:
        """
        @brief 删除最新的存档文件。
        """
        files = self.list_checkpoints()
        if not files:
            return
        files.sort(reverse=True)
        path = os.path.join(self.checkpoint_dir, files[0])
        if os.path.isfile(path):
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
            f for f in os.listdir(self.checkpoint_dir)
            if f.startswith("ckpt_") and f.endswith(".json")
        ]
