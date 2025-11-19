import os
import json
import yaml
import hashlib
import importlib
import inspect
import logging
import json_repair
from config import Config
from typing import Dict, List, Tuple
from ctf_tool.base_tool import BaseTool
from utils.llm_request import LLMRequest
from jinja2 import Environment, FileSystemLoader
from utils.text import fix_json_with_llm
from litellm import ModelResponse, ChatCompletionMessageToolCall

logger = logging.getLogger(__name__)


class ToolUtils:
    def __init__(self):
        self.config = Config.load_config()
        self.analyzer_llm = LLMRequest("analyzer")
        self.function_config = []
        self.tools = {}
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None:
            raise ValueError("找不到配置文件")

        # 初始化Jinja2模板环境
        self.env = Environment(loader=FileSystemLoader("."))
        self.tool_classification: Dict = {}  # 工具分类信息
        self.classification_file = os.path.join(
            os.path.dirname(__file__), "..", "tool_classification.json"
        )
        self.tools_hash_file = os.path.join(
            os.path.dirname(__file__), "..", "tools_hash.txt"
        )

    def _calculate_tools_hash(self, tools_info: List[Dict]) -> str:
        """计算工具信息的哈希值"""
        tools_str = json.dumps(tools_info, sort_keys=True)
        return hashlib.md5(tools_str.encode()).hexdigest()

    def _get_tools_info(self, function_configs: List[Dict]) -> List[Dict]:
        """获取工具的基本信息用于分类"""
        tools_info = []
        for config in function_configs:
            tool_info = {
                "name": config["function"]["name"],
                "description": config["function"].get("description", ""),
                "parameters": config["function"].get("parameters", {}),
            }
            tools_info.append(tool_info)
        return tools_info

    def _needs_reclassification(self, current_hash: str) -> bool:
        """判断是否需要重新分类"""
        if not os.path.exists(self.classification_file) or not os.path.exists(
            self.tools_hash_file
        ):
            return True

        try:
            with open(self.tools_hash_file, "r") as f:
                saved_hash = f.read().strip()
            return saved_hash != current_hash
        except:
            return True

    def _save_classification(self, classification: Dict, tools_hash: str):
        """保存分类结果"""
        # 确保目录存在
        os.makedirs(os.path.dirname(self.classification_file), exist_ok=True)

        with open(self.classification_file, "w", encoding="utf-8") as f:
            json.dump(classification, f, indent=2, ensure_ascii=False)

        with open(self.tools_hash_file, "w") as f:
            f.write(tools_hash)

    def _load_classification(self) -> Dict:
        """加载已有的分类结果"""
        logger.info("加载已有的工具分类结果")
        try:
            with open(self.classification_file, "r", encoding="utf-8") as f:
                return json.load(f)
        except:
            return {}

    def _classify_tools_with_llm(self, tools_info: List[Dict]) -> Dict:
        """使用大模型对工具进行分类"""

        # 第一步：确定工具类别
        category_prompt = f"""
        请分析以下CTF工具，确定它们应该分为哪些类别。请考虑工具的功能、用途和CTF比赛的类型。
        例如：nmap、ffuf属于web侦查与发现类，sqlmap、dirb属于web测试类，而ssh命令执行等属于通用类
        当一个工具不确定什么类别的时候，请返回通用类
        工具列表如下：
        {json.dumps(tools_info, indent=2, ensure_ascii=False)}
        请只返回一个JSON格式的类别列表，不要包含其他内容：
        {{
            "categories": ["类别1", "类别2", "类别3", ...]
        }}
        """

        try:
            # 获取类别
            category_response = self.analyzer_llm.text_completion(
                prompt=category_prompt, json_check=True
            )
            category_result = category_response.choices[0].message.content
            category_json = json_repair.loads(category_result)
            categories = category_json.get("categories", {})
        except Exception as e:
            logger.warning(f"分类失败，使用默认类别: {e}")
            categories = ["crypto", "web", "pwn", "reverse", "forensics", "通用"]

        # 第二步：对工具进行分类
        classification_prompt = f"""
        请将以下CTF工具按照确定的类别进行分类。每个工具只能属于一个最相关的类别。
        可用类别：{", ".join(categories)}
        工具列表：
        {json.dumps(tools_info, indent=2, ensure_ascii=False)}
        请返回JSON格式的分类结果：
        {{
            "categories": {categories},
            "classification": {{
                "工具名称1": "类别名称",
                "工具名称2": "类别名称",
                ...
            }}
        }}
        """
        try:
            classification_response = self.analyzer_llm.text_completion(
                prompt=classification_prompt, json_check=True
            )

            classification_result = classification_response.choices[0].message.content
            return json_repair.loads(classification_result)
        except Exception as e:
            logger.warning(f"工具分类失败: {e}")
            # 返回默认分类
            default_classification = {}
            for tool_info in tools_info:
                default_classification[tool_info["name"]] = "通用"
            return {"categories": categories, "classification": default_classification}

    def classify_tools(self, function_configs: List[Dict]) -> Dict:
        """分类工具的主要接口"""
        tools_info = self._get_tools_info(function_configs)
        current_hash = self._calculate_tools_hash(tools_info)

        # 检查是否需要重新分类
        if not self._needs_reclassification(current_hash):
            return self._load_classification()

        # 需要重新分类
        logger.info("工具发生变化，正在重新分类...")
        classification = self._classify_tools_with_llm(tools_info)

        # 保存分类结果
        self._save_classification(classification, current_hash)
        logger.info("工具分类完成并已保存")

        return classification

    def load_tools(self) -> Tuple[Dict, list, Dict]:
        """动态加载tool文件夹中的所有工具，并返回工具字典、配置列表和分类信息"""
        # 加载配置文件
        config = Config.load_config()
        tools_dir = os.path.join(os.path.dirname(__file__), "..", "ctf_tool")

        # 加载本地工具
        for file_name in os.listdir(tools_dir):
            if (
                file_name.endswith(".py")
                and file_name != "__init__.py"
                and file_name != "base_tool.py"
                and file_name != "mcp_adapter.py"  # 排除MCP文件
            ):
                module_name = file_name[:-3]  # 移除.py
                try:
                    # 导入模块
                    module = importlib.import_module(f"ctf_tool.{module_name}")

                    # 查找所有继承自BaseTool的类
                    for name, obj in inspect.getmembers(module):
                        if (
                            inspect.isclass(obj)
                            and issubclass(obj, BaseTool)
                            and obj != BaseTool
                        ):
                            # 检查是否需要特殊配置
                            if name in config.get("tool_config", {}):
                                # 使用配置创建实例
                                tool_config = config["tool_config"][name]
                                tool_instance = obj(tool_config)
                            else:
                                # 创建默认实例
                                tool_instance = obj()

                            # 添加到工具字典
                            tool_name = tool_instance.function_config["function"][
                                "name"
                            ]
                            self.tools[tool_name] = tool_instance

                            # 添加工具配置
                            self.function_config.append(tool_instance.function_config)

                            logger.info(f"已加载工具: {tool_name}")
                except Exception as e:
                    print(f"加载工具{module_name}失败: {str(e)}")

        # 加载MCP服务器工具
        mcp_servers: dict = config.get("mcp_server", {})  # 注意键名大小写

        # 遍历MCP服务器配置
        for server_name, server_config in mcp_servers.items():
            try:
                from ctf_tool.mcp_adapter import MCPServerAdapter

                # 添加服务器名称到配置
                server_config["name"] = server_name
                adapter = MCPServerAdapter(server_config)

                # 添加MCP工具的函数配置
                for mcp_tool_config in adapter.get_tool_configs():
                    tool_name = mcp_tool_config["function"]["name"]
                    # 适配器实例负责执行此工具
                    self.tools[tool_name] = adapter
                    self.function_config.append(mcp_tool_config)

                logger.info(f"已加载MCP服务器: {server_name}")
            except Exception as e:
                print(f"加载MCP服务器失败: {str(e)}")
        # 工具分类
        self.tool_classification = self.classify_tools(self.function_config)

        return self.tools, self.function_config, self.tool_classification

    def classify_think(
        self, problem: str, think_content: str, history_summary: str
    ) -> str:
        """
        对思考内容进行分类，确定应该使用哪类工具
        :param think_content: 思考内容
        :return: 工具类别名称
        """
        if not self.tool_classification or not self.tool_classification.get(
            "categories"
        ):
            logger.warning("没有可用的工具分类信息，使用所有工具")
            return "all"  # 如果没有分类信息，使用所有工具
        analyzer_llm = LLMRequest("analyzer")
        categories = self.tool_classification.get("categories", [])

        # 使用LLM对思考内容进行分类
        template = self.env.from_string(self.prompt.get("classify_think", ""))
        classify_prompt = template.render(
            think_content=think_content,
            history_summary=history_summary,
            categories=categories,
            problem=problem,
        )

        try:
            response = analyzer_llm.text_completion(
                prompt=classify_prompt, json_check=False
            )

            category = response.choices[0].message.content.strip()

            # 验证分类结果是否在可用类别中
            if category in categories:
                logger.info(f"思考内容分类为: {category}")
                return category
            else:
                logger.warning(f"分类结果 '{category}' 不在可用类别中，使用所有工具")
                return "all"

        except Exception as e:
            logger.error(f"思考内容分类失败: {e}，使用所有工具")
            return "all"

    def get_tools_by_category(self, category: str) -> List[Dict]:
        """
        根据类别获取对应的工具配置
        :param category: 工具类别
        :return: 该类别下的工具配置列表
        """
        if category == "all" or category not in self.tool_classification.get(
            "categories", []
        ):
            return self.function_config  # 返回所有工具

        classification_map = self.tool_classification.get("classification", {})
        category_tools = []

        for tool_config in self.function_config:
            tool_name = tool_config["function"]["name"]
            if classification_map.get(tool_name) == category:
                category_tools.append(tool_config)

        logger.info(f"类别 '{category}' 下有 {len(category_tools)} 个工具")
        return category_tools

    def get_tool_category(self, tool_name: str) -> str:
        """
        根据工具名称获取工具类别
        :param tool_name: 工具名称
        :return: 工具类别名称
        """
        if not self.tool_classification:
            return "unknown"

        classification_map: dict = self.tool_classification.get("classification", {})
        return classification_map.get(tool_name, "unknown")

    @staticmethod
    def parse_tool_response(response: ModelResponse) -> Dict:
        """统一解析工具调用响应，处理两种格式的响应"""
        message = response.choices[0].message
        func_name = None
        # 情况1：直接工具调用格式（tool_calls）
        if hasattr(message, "tool_calls") and message.tool_calls:
            tool_call: ChatCompletionMessageToolCall = message.tool_calls[0]
            func_name = tool_call.function.name
            try:
                args = json.loads(tool_call.function.arguments)
            except json.JSONDecodeError as e:
                args = fix_json_with_llm(tool_call.function.arguments, e)

        # 情况2：JSON字符串格式
        else:
            content = message.content.strip()
            # 尝试直接解析JSON
            try:
                data = json.loads(content)
            except json.JSONDecodeError as e:
                print("无法解析工具调用响应，尝试修复")
                content = fix_json_with_llm(content, e)
                data = json.loads(content)
            except Exception as e:
                print(f"无法解析工具调用响应：{e}")
                return {}
            if "tool_calls" in data and data["tool_calls"]:
                tool_call: dict = data["tool_calls"][0]
                func_name: dict = tool_call.get("name", "工具解析失败")
                args: dict = tool_call.get("arguments", {})

        logger.info(f"使用工具: {func_name}")
        logger.info(f"执行内容:\n{args}")
        return {"tool_name": func_name, "arguments": args}

    @staticmethod
    def output_summary(
        tool_name: str, tool_arg: str, think: str, tool_output: str
    ) -> str:
        if len(str(tool_output)) <= 1024:
            return str(tool_output)
        prompt = (
            "你是一个CTF解题助手，你的任务是总结工具执行后的输出。"
            "要求：一定要保留诸如路径、端点、表单等关键信息，信息一定要完整，但不需要给操作建议"
            f"思路：{think}"
            f"工具名称：{tool_name}"
            f"工具参数：{tool_arg}"
            f"工具输出：{tool_output[:20480]}"
        )
        try:
            solve_llm = LLMRequest("solve_agent")
            response = solve_llm.text_completion(prompt, json_check=False)
            return response.choices[0].message.content
        except:
            return str(tool_output)
