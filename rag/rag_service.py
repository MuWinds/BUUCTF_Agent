import chromadb
import logging
from config import Config
from chromadb.config import Settings
from chromadb import ClientAPI
from typing import List, Dict, Any, Optional
from utils.llm_request import LLMRequest

logger = logging.getLogger(__name__)


class BaseVectorStore:
    """向量存储的基类，处理与ChromaDB的通用交互"""

    def __init__(self, collection_name: str, collection_description: str):
        self.config = Config.load_config()
        self.persist_directory = self.config.get("persist_directory", "./rag_db")
        self.embedding_llm = LLMRequest("embedding")
        self.client = None
        self.collection: Optional[chromadb.Collection] = None
        self.collection_name = collection_name
        self.collection_description = collection_description
        self._initialize_database()

    def _initialize_database(self):
        """初始化向量数据库并获取指定的集合"""
        try:
            self.client: ClientAPI = chromadb.PersistentClient(
                path=self.persist_directory,
                settings=Settings(anonymized_telemetry=False),
            )
            # 根据子类提供的名称创建或获取集合
            self.collection = self.client.get_or_create_collection(
                name=self.collection_name,
                metadata={"description": self.collection_description},
            )
            logger.info(f"向量存储集合 '{self.collection_name}' 初始化完成")
        except Exception as e:
            logger.error(f"向量存储 '{self.collection_name}' 初始化失败: {str(e)}")
            raise

    def get_embeddings(self, texts: List[str]) -> List[List[float]]:
        """获取文本的嵌入向量"""
        try:
            response = self.embedding_llm.embedding(text=texts)
            return [item["embedding"] for item in response.data]
        except Exception as e:
            logger.error(f"获取嵌入向量失败: {e}")
            raise

    def _add(
        self, contents: List[str], metadatas: List[Dict[str, Any]], ids: List[str]
    ):
        """向集合中添加文档（底层方法）"""
        try:
            embeddings = self.get_embeddings(contents)
            self.collection.add(
                documents=contents,
                embeddings=embeddings,
                metadatas=metadatas,
                ids=ids,
            )
            logger.info(
                f"成功向集合 '{self.collection_name}' 添加了 {len(contents)} 条目"
            )
        except Exception as e:
            logger.error(f"向集合 '{self.collection_name}' 添加条目失败: {str(e)}")
            raise

    def _search(
        self, query: str, n_results: int = 3, filters: Optional[Dict[str, Any]] = None
    ) -> List[Dict[str, Any]]:
        """在集合中搜索相关文档（底层方法）"""
        try:
            query_embedding = self.get_embeddings([query])[0]
            results = self.collection.query(
                query_embeddings=[query_embedding],
                n_results=n_results,
                where=filters,
                include=["documents", "metadatas", "distances"],
            )
            # 格式化结果
            formatted_results = []
            if not results or not results.get("documents"):
                return []

            for i in range(len(results["documents"][0])):
                distance = results["distances"][0][i]
                formatted_results.append(
                    {
                        "content": results["documents"][0][i],
                        "metadata": results["metadatas"][0][i],
                        "distance": results["distances"][0][i],
                        "relevance_score": 1 - results["distances"][0][i],
                    }
                )
            return formatted_results
        except Exception as e:
            logger.error(f"在集合 '{self.collection_name}' 中搜索失败: {str(e)}")
            return []

    def delete_collection(self):
        """删除整个集合"""
        try:
            self.client.delete_collection(name=self.collection_name)
            logger.info(f"集合 '{self.collection_name}' 已被删除")
        except Exception as e:
            logger.error(f"删除集合 '{self.collection_name}' 失败: {str(e)}")
            raise
