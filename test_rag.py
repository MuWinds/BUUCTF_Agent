"""
测试 RAG 功能是否可用
"""
import logging
import sys
from config import Config
from rag.knowledge_base import KnowledgeBase

# 配置日志
logging.basicConfig(
    level=logging.DEBUG,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)

def test_rag():
    """测试 RAG 知识库功能"""
    try:
        # 1. 检查配置
        config = Config.load_config()
        if "embedding" not in config.get("llm", {}):
            logger.error("配置文件中缺少 'embedding' LLM 配置")
            return False
        
        embedding_config = config["llm"]["embedding"]
        if not embedding_config.get("api_key"):
            logger.warning("embedding API key 为空，可能无法调用")
        
        logger.info(f"Embedding 配置检查通过: {embedding_config.get('model')}")
        
        # 2. 测试知识库初始化
        logger.info("正在初始化知识库...")
        kb = KnowledgeBase()
        logger.info("知识库初始化成功")
        
        # 3. 测试添加知识
        logger.info("测试添加知识...")
        doc_id = kb.add_general_knowledge(
            "CTF Web题常见漏洞：SQL注入、XSS、文件上传、命令执行",
            tags=["web", "漏洞"]
        )
        logger.info(f"成功添加知识，文档ID: {doc_id}")
        
        # 4. 测试搜索知识
        logger.info("测试搜索知识...")
        results = kb.search_knowledge("SQL注入", n_results=3)
        if results:
            logger.info(f"搜索成功，找到 {len(results)} 条结果")
            for i, result in enumerate(results, 1):
                logger.info(f"  结果 {i}: {result.get('content', '')[:100]}...")
                logger.info(f"    相似度: {result.get('relevance_score', 0):.4f}")
        else:
            logger.warning("搜索未找到结果（可能是知识库为空）")
        
        # 5. 测试获取相关知识
        logger.info("测试获取相关知识...")
        knowledge = kb.get_relevant_knowledge("如何测试SQL注入漏洞", n_results=3)
        logger.info(f"获取相关知识成功")
        logger.info(f"知识内容:\n{knowledge}")
        
        return True
        
    except KeyError as e:
        logger.error(f"配置错误: {e}")
        logger.error("请检查 config.json 中是否包含 'embedding' 配置")
        return False
    except Exception as e:
        logger.error(f"RAG 测试失败: {e}", exc_info=True)
        return False

if __name__ == "__main__":
    success = test_rag()
    sys.exit(0 if success else 1)

