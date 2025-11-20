import hashlib
import logging
from datetime import datetime
from rag.rag_service import BaseVectorStore
from typing import List, Dict, Any
logger = logging.getLogger(__name__)


class MemorySystem(BaseVectorStore):
    """记忆系统 - 存储智能体在解决问题过程中的具体步骤、观察和结论"""

    def __init__(self, persist_directory: str = "./rag_db"):
        self.persist_directory = persist_directory
        collection_name = "agent_memory"
        collection_description = "存储智能体在任务执行过程中的记忆和经验"

        # 步骤 1: 只调用一次父类初始化
        super().__init__(
            collection_name=collection_name,
            collection_description=collection_description,
        )

        # 步骤 2: 使用已初始化的 self.client 来删除可能存在的旧集合
        try:
            self.client.delete_collection(name=collection_name)
            logger.info(f"成功删除旧的集合: '{collection_name}'")
        except Exception as e:
            # 在首次运行时，集合不存在，删除会失败，这是正常现象。
            logger.info(f"尝试删除集合 '{collection_name}' 失败（可能是首次创建，此为正常现象）: {e}")

        # 步骤 3: 显式地重新创建集合，并确保 self.collection 指向这个新的、有效的集合
        self.collection = self.client.create_collection(
            name=collection_name,
            metadata={"description": collection_description} # 根据你使用的向量数据库API调整
        )
        logger.info(f"成功创建新的集合: '{collection_name}'")


    def add_problem_solution_memory(
        self,
        problem: str,
        solution: str,
        problem_type: str,
    ) -> str:
        """记录一个已完成问题的完整解决方案作为长期记忆"""
        content = f"问题: {problem}\n解决方案: {solution}"
        metadata = {
            "type": "problem_solution",
            "problem_type": problem_type,
            "timestamp": datetime.now().isoformat(),
        }

        doc_id = hashlib.md5(f"{content}{datetime.now()}".encode()).hexdigest()[:16]
        self._add(contents=[content], metadatas=[metadata], ids=[doc_id])
        return doc_id

    def add_step_memory(
        self, step: int, action: str, observation: str, thought: str
    ) -> str:
        """记录任务执行过程中的单步记忆"""
        content = f"步骤 {step}:\n思考: {thought}\n行动: {action}\n观察: {observation}"
        metadata = {
            "type": "step_memory",
            "step": step,
            "timestamp": datetime.now().isoformat(),
        }

        doc_id = hashlib.md5(content.encode()).hexdigest()[:16]
        self._add(contents=[content], metadatas=[metadata], ids=[doc_id])
        return doc_id

    def search_memories(
        self, query: str, n_results: int = 3, filters: Dict[str, Any] = None
    ) -> List[Dict[str, Any]]:
        """从记忆中搜索相关事件"""
        return self._search(query, n_results, filters)

    def get_relevant_memories_for_prompt(
        self, current_problem: str, n_results: int = 3
    ) -> str:
        """获取与当前问题相关的过往记忆，用于构建提示"""
        results = self.search_memories(f"与 '{current_problem}' 类似的问题", n_results)
        if not results:
            return "从记忆中未找到相关记忆。"

        memory_text = "相关过往记忆:\n"
        for i, result in enumerate(results, 1):
            # 假设 result 字典中包含 'content' 键
            # 如果你的 _search 返回的格式不同，请相应调整
            content = result.get('content', '内容未知')
            memory_text += f"{i}. {content}\n"
        logger.debug(memory_text)
        return memory_text