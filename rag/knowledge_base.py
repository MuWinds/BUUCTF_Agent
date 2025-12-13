import logging
import hashlib
from datetime import datetime
from rag.rag_service import BaseVectorStore
from typing import List, Dict, Any

logger = logging.getLogger(__name__)

class KnowledgeBase(BaseVectorStore):
    """知识库系统 - 存储通用的、可复用的知识，如工具用法和解题模式"""

    def __init__(self):
        super().__init__(
            collection_name="ctf_knowledge",
            collection_description="存储CTF通用知识、工具用法和最佳实践"
        )

    def add_tool_knowledge(
        self,
        tool_name: str,
        usage_examples: List[str],
        best_practices: List[str] = None,
    ) -> str:
        """添加工具使用知识"""
        content = f"工具: {tool_name}\n使用示例: {'; '.join(usage_examples)}"
        if best_practices:
            content += f"\n最佳实践: {'; '.join(best_practices)}"
        
        metadata = {
            "type": "tool_knowledge",
            "tool_name": tool_name,
            "examples_count": len(usage_examples),
            "timestamp": datetime.now().isoformat(),
        }
        
        doc_id = hashlib.md5(content.encode()).hexdigest()[:16]
        self._add(contents=[content], metadatas=[metadata], ids=[doc_id])
        return doc_id

    def add_general_knowledge(self, content: str, tags: List[str] = None) -> str:
        """添加通用的CTF知识或技巧"""
        # ChromaDB metadata 不支持列表，需要转换为字符串
        tags_str = ",".join(tags) if tags else ""
        metadata = {
            "type": "general_knowledge",
            "tags": tags_str,  # 转换为逗号分隔的字符串
            "timestamp": datetime.now().isoformat(),
        }
        doc_id = hashlib.md5(content.encode()).hexdigest()[:16]
        self._add(contents=[content], metadatas=[metadata], ids=[doc_id])
        return doc_id

    def search_knowledge(self, query: str, n_results: int = 3, filters: Dict[str, Any] = None) -> List[Dict[str, Any]]:
        """从知识库中搜索相关知识"""
        return self._search(query, n_results, filters)

    def get_relevant_knowledge(self, query: str, n_results: int = 3) -> str:
        """获取格式化的相关知识，用于构建提示"""
        logger.debug("查询： %s", query)
        results = self.search_knowledge(query, n_results)
        if not results:
            return "从知识库中未找到相关信息。"

        knowledge_text = "相关知识仅供参考:\n"
        for i, result in enumerate(results, 1):
            knowledge_text += f"{i}. {result['content']}\n"
        return knowledge_text