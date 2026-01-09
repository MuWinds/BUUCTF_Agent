import os
import json
import yaml
import hashlib
import importlib
import inspect
import logging
import json_repair
import numpy as np
from config import Config
from typing import Dict, List, Optional, Tuple, Union
from ctf_tool.base_tool import BaseTool
from utils.llm_request import LLMRequest
from jinja2 import Environment, FileSystemLoader
from litellm import ModelResponse

logger = logging.getLogger(__name__)


class ToolUtils:
    def __init__(self):
        self.config = Config.load_config()
        self.analyzer_llm = LLMRequest("analyzer")
        self.embedding_llm = LLMRequest("embedding")  # 新增 Embedding 模型请求

        self.tools = {}  # 存储所有工具实例 {name: instance}

        # 分离两类配置
        self.local_function_configs = []  # 本地工具配置 (默认全注入)
        self.mcp_function_configs = []  # MCP工具配置 (需要检索)

        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None:
            raise ValueError("找不到配置文件")

        self.env = Environment(loader=FileSystemLoader("."))

        # 向量缓存文件
        self.embeddings_cache_file = os.path.join(
            os.path.dirname(__file__), "..", "tool_embeddings_cache.json"
        )
        self.tool_embeddings_map = self._load_embeddings_cache()

    def _calculate_tool_hash(self, tool_config: Dict) -> str:
        """计算单个工具配置的哈希值，用于判断描述是否变更"""
        # 主要基于名字和描述计算hash
        info = {
            "name": tool_config["function"]["name"],
            "description": tool_config["function"].get("description", ""),
            "parameters": str(tool_config["function"].get("parameters", {})),
        }
        return hashlib.md5(json.dumps(info, sort_keys=True).encode()).hexdigest()

    def _load_embeddings_cache(self) -> Dict:
        """加载向量缓存"""
        if os.path.exists(self.embeddings_cache_file):
            try:
                with open(self.embeddings_cache_file, "r", encoding="utf-8") as f:
                    return json.load(f)
            except Exception as e:
                logger.warning(f"加载向量缓存失败: {e}")
        return {}

    def _save_embeddings_cache(self):
        """保存向量缓存"""
        try:
            with open(self.embeddings_cache_file, "w", encoding="utf-8") as f:
                json.dump(self.tool_embeddings_map, f, ensure_ascii=False)
        except Exception as e:
            logger.error(f"保存向量缓存失败: {e}")

    def _get_embedding(self, text: str) -> List[float]:
        """调用模型获取向量 (参考 RAG Service)"""
        try:
            # 注意：这里假设 LLMRequest.embedding 返回格式与 litellm 或 openai 格式兼容
            # 如果是 list[str]，通常返回 data list
            response = self.embedding_llm.embedding(text=[text])
            # 根据 litellm 结构获取 embedding
            return response.data[0]["embedding"]
        except Exception as e:
            logger.error(f"获取嵌入向量失败: {e}")
            # 发生错误返回空向量或零向量，避免程序崩溃
            return []

    def _update_mcp_embeddings(self):
        """检查并更新 MCP 工具的向量"""
        has_updates = False

        for config in self.mcp_function_configs:
            tool_name = config["function"]["name"]
            description = config["function"].get("description", "")
            # 组合名称和描述以增强检索语义
            text_to_embed = f"{tool_name}: {description}"
            current_hash = self._calculate_tool_hash(config)

            # 检查缓存是否存在且未过期
            cached_data = self.tool_embeddings_map.get(tool_name)

            if not cached_data or cached_data.get("hash") != current_hash:
                logger.info(f"正在生成工具向量: {tool_name}")
                embedding = self._get_embedding(text_to_embed)
                if embedding:
                    self.tool_embeddings_map[tool_name] = {
                        "hash": current_hash,
                        "embedding": embedding,
                        "text": text_to_embed,
                    }
                    has_updates = True

        if has_updates:
            self._save_embeddings_cache()
            logger.info("工具向量缓存已更新")

    def load_tools(self) -> Tuple[Dict, list]:
        """
        加载工具，区分本地工具和MCP工具
        返回: (所有工具实例字典, 所有工具配置列表)
        """
        config = Config.load_config()
        tools_dir = os.path.join(os.path.dirname(__file__), "..", "ctf_tool")

        # 重置列表
        self.local_function_configs = []
        self.mcp_function_configs = []
        self.tools = {}

        # 1. 加载本地工具 (Local Tools) - 默认全注入
        for file_name in os.listdir(tools_dir):
            if file_name.endswith(".py") and file_name not in [
                "__init__.py",
                "base_tool.py",
                "mcp_adapter.py",
            ]:
                module_name = file_name[:-3]
                try:
                    module = importlib.import_module(f"ctf_tool.{module_name}")
                    for name, obj in inspect.getmembers(module):
                        if (
                            inspect.isclass(obj)
                            and issubclass(obj, BaseTool)
                            and obj != BaseTool
                        ):
                            # 实例化
                            if name in config.get("tool_config", {}):
                                tool_instance = obj(config["tool_config"][name])
                            else:
                                tool_instance = obj()

                            tool_name = tool_instance.function_config["function"][
                                "name"
                            ]

                            # 注册到工具字典
                            self.tools[tool_name] = tool_instance
                            # 添加到本地配置列表
                            self.local_function_configs.append(
                                tool_instance.function_config
                            )
                            logger.info(f"已加载本地工具: {tool_name}")
                except Exception as e:
                    logger.error(f"加载本地工具{module_name}失败: {str(e)}")

        # 2. 加载 MCP 工具 (MCP Tools) - 需要 RAG 检索
        mcp_servers: dict = config.get("mcp_server", {})
        for server_name, server_config in mcp_servers.items():
            try:
                from ctf_tool.mcp_adapter import MCPServerAdapter

                server_config["name"] = server_name
                adapter = MCPServerAdapter(server_config)

                for mcp_tool_config in adapter.get_tool_configs():
                    tool_name = mcp_tool_config["function"]["name"]
                    self.tools[tool_name] = adapter
                    self.mcp_function_configs.append(mcp_tool_config)

                logger.info(f"已加载MCP服务器: {server_name}")
            except Exception as e:
                logger.error(f"加载MCP服务器失败: {str(e)}")

        # 3. 更新 MCP 工具的向量
        if self.mcp_function_configs:
            self._update_mcp_embeddings()

        # 返回所有工具供 invoke 使用，列表返回全部以防万一
        all_configs = self.local_function_configs + self.mcp_function_configs
        return self.tools, all_configs

    def recommend_tools(self, query: str, top_k: int = 3) -> List[Dict]:
        """
        根据查询内容推荐工具
        逻辑：返回 [所有本地工具] + [Top-K 相似的 MCP 工具]
        """
        # 如果没有 MCP 工具，直接返回本地工具
        if not self.mcp_function_configs:
            return self.local_function_configs

        # 1. 获取 Query 向量
        query_embedding = self._get_embedding(query)
        if not query_embedding:
            logger.warning("无法获取查询向量，返回所有工具")
            return self.local_function_configs + self.mcp_function_configs

        # 2. 计算相似度
        scores = []
        for config in self.mcp_function_configs:
            tool_name = config["function"]["name"]
            cached = self.tool_embeddings_map.get(tool_name)

            if cached and "embedding" in cached:
                tool_vec = cached["embedding"]
                # 计算余弦相似度
                similarity = self._cosine_similarity(query_embedding, tool_vec)
                scores.append((similarity, config))
            else:
                # 如果没有向量，暂时给个低分或默认不推荐
                scores.append((-1.0, config))

        # 3. 排序并取 Top-K
        scores.sort(key=lambda x: x[0], reverse=True)
        top_mcp_tools = [item[1] for item in scores[:top_k]]
        logger.info(
            f"RAG 命中 MCP 工具: {[t['function']['name'] for t in top_mcp_tools]}"
        )

        # 4. 合并结果
        return self.local_function_configs + top_mcp_tools

    @staticmethod
    def _cosine_similarity(vec_a: List[float], vec_b: List[float]) -> float:
        """计算余弦相似度"""
        if not vec_a or not vec_b:
            return 0.0
        try:
            a = np.array(vec_a)
            b = np.array(vec_b)
            norm_a = np.linalg.norm(a)
            norm_b = np.linalg.norm(b)
            if norm_a == 0 or norm_b == 0:
                return 0.0
            return float(np.dot(a, b) / (norm_a * norm_b))
        except Exception:
            return 0.0

    @staticmethod
    def parse_tool_response(response: ModelResponse) -> List[Dict]:
        """统一解析工具调用响应，支持多个工具调用"""
        message = response.choices[0].message
        tool_calls = []

        # 处理原生tool_calls
        if hasattr(message, "tool_calls") and message.tool_calls:
            for tool_call in message.tool_calls:
                func_name = tool_call.function.name
                try:
                    args = json_repair.loads(tool_call.function.arguments)
                except json.JSONDecodeError as e:
                    args = ToolUtils.fix_json_with_llm(tool_call.function.arguments, e)

                tool_calls.append({"tool_name": func_name, "arguments": args})
        else:
            # 尝试从content中解析JSON
            content = message.content.strip()
            try:
                data = json_repair.loads(content)
            except json.JSONDecodeError as e:
                logger.warning("无法直接解析JSON，尝试修复")
                content = ToolUtils.fix_json_with_llm(content, e)
                try:
                    data = json_repair.loads(content)
                except:
                    return []

            # 解析tool_calls字段
            if "tool_calls" in data and isinstance(data["tool_calls"], list):
                for tool_call in data["tool_calls"]:
                    if isinstance(tool_call, dict):
                        func_name = tool_call.get("name")
                        args = tool_call.get("arguments", {})
                        tool_calls.append({"tool_name": func_name, "arguments": args})
            # 向后兼容：处理单个工具调用
            elif "tool_call" in data:
                tool_call = data.get("tool_call", {})
                func_name = tool_call.get("name", tool_call.get("tool_name"))
                args = tool_call.get("arguments", {})
                tool_calls.append({"tool_name": func_name, "arguments": args})

        for tool_call in tool_calls:
            logger.info(f"使用工具: {tool_call.get('tool_name')}")
            logger.info(f"参数: {tool_call.get('arguments')}")

        return tool_calls
    
    @staticmethod
    def output_summary(
        tool_results: Union[List[Dict], Dict, str, None] = None, 
        think: str = "", 
        tool_output: str = "",
        tool_name: str = None,
    ) -> str:
        # 处理多种参数格式
        all_tool_results = []
        combined_output = ""
        actual_think = think
        
        # 情况1：新方式 - tool_results是列表
        if isinstance(tool_results, list) and tool_results:
            all_tool_results = tool_results
            combined_output = tool_output

        # 如果没有工具结果，返回空字符串
        if not all_tool_results:
            return ""
        
        # 如果总输出长度不超过1024，直接返回
        if len(str(combined_output)) <= 1024:
            return str(combined_output)
        
        # 构建详细的工具调用信息
        tool_details = []
        for i, result in enumerate(all_tool_results, 1):
            tool_name = result.get("tool_name", f"工具{i}")
            arguments = result.get("arguments", {})
            raw_output = result.get("raw_output", "")
            output_preview = str(raw_output)[:200] + ("..." if len(str(raw_output)) > 200 else "")
            
            tool_details.append(f"工具{i} [{tool_name}]:\n参数: {arguments}\n输出预览: {output_preview}")
        
        tool_details_str = "\n---\n".join(tool_details)
        
        # 获取实际工具数量
        tool_count = len(all_tool_results)
        prompt = (
            "你是一个CTF解题助手，你的任务是总结多个工具执行后的输出。\n"
            "要求：\n"
            "1. 保留关键信息，如路径、端点、表单、漏洞线索、重要数据等\n"
            "2. 移除冗余信息，如大量重复日志、无关的调试信息等\n"
            "3. 如果多个工具的输出有关联，说明它们之间的关系\n"
            "4. 输出结构要清晰，包含每个工具的关键发现\n\n"
            f"执行思路：{actual_think}\n\n"
            f"工具调用详情：\n{tool_details_str}\n\n"
            f"合并原始输出：\n{combined_output}"
        )
    
        try:
            solve_llm = LLMRequest("solve_agent")
            response = solve_llm.text_completion(prompt, json_check=False)
            return response.choices[0].message.content
        except Exception as e:
            logger.error(f"工具输出总结失败: {e}")
            # 回退方案：返回每个工具的简化输出
            simplified_outputs = []
            for i, result in enumerate(all_tool_results, 1):
                tool_name = result.get("tool_name", f"工具{i}")
                raw_output = result.get("raw_output", "")
                simplified = str(raw_output)[:500] + ("..." if len(str(raw_output)) > 500 else "")
                simplified_outputs.append(f"{tool_name}: {simplified}")
            
            if tool_count > 1:
                return f"工具执行摘要（总结失败，显示简化输出，共{tool_count}个工具）：\n" + "\n---\n".join(simplified_outputs)
            else:
                return simplified_outputs[0] if simplified_outputs else "无输出"