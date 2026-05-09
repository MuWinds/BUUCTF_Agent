import importlib
import inspect
import json
import logging
import os
from typing import Any, Callable, Dict, List, Optional, Tuple

from config import Config
from ctf_tool.base_tool import BaseTool

logger = logging.getLogger(__name__)


class ToolUtils:
    """
    @brief 工具加载与工具响应处理工具类。

    @details
    负责加载本地工具与 MCP 工具、解析工具调用参数，
    并在输出过长时生成工具执行摘要。
    """

    def __init__(self):
        """
        @brief 初始化 ToolUtils。
        @return 无返回值。
        @raises ValueError 当配置文件不存在或读取失败时抛出。
        """
        self.config = Config.load_config()
        self.analyzer_llm = LLMRequest("solve_agent")

        self.tools: Dict[str, Any] = {}
        self.local_function_configs: List[Dict[str, Any]] = []
        self.mcp_function_configs: List[Dict[str, Any]] = []

        with open("./prompt.yaml", "r", encoding="utf-8") as prompt_file:
            self.prompt: dict = yaml.safe_load(prompt_file)

        if self.config is None:
            raise ValueError("找不到配置文件")

        self.env = Environment(loader=FileSystemLoader("."))

    def load_tools(self) -> Tuple[Dict[str, Any], List[Dict[str, Any]]]:
        """
        @brief 加载工具并区分本地工具与 MCP 工具。
        @return 二元组：(工具实例字典, 工具配置列表)。
        """
        config = Config.load_config()
        tools_dir = os.path.join(os.path.dirname(__file__), "..", "ctf_tool")

        self.local_function_configs = []
        self.mcp_function_configs = []
        self.tools = {}

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
                            if name in config.get("tool_config", {}):
                                tool_instance = obj()
                            else:
                                tool_instance = obj()

                            tool_name = tool_instance.function_config["function"][
                                "name"
                            ]
                            self.tools[tool_name] = tool_instance
                            self.local_function_configs.append(
                                tool_instance.function_config
                            )
                            logger.info("已加载本地工具: %s", tool_name)
                except Exception as error:
                    logger.error("加载本地工具%s失败: %s", module_name, str(error))

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

                logger.info("已加载MCP服务器: %s", server_name)
            except Exception as error:
                logger.error("加载MCP服务器失败: %s", str(error))

        all_configs = self.local_function_configs + self.mcp_function_configs
        return self.tools, all_configs

    @staticmethod
    def parse_tool_response(response: Any) -> List[Dict[str, Any]]:
        """
        @brief 统一解析工具调用响应。

        @details
        当前仅支持从 response.choices[0].message.tool_calls 中读取工具调用。

        @param response LLM 原始响应对象。
        @return 工具调用列表，每项包含 tool_name 与 arguments。
        """
        message = response.choices[0].message
        tool_calls: List[Dict[str, Any]] = []

        if not (hasattr(message, "tool_calls") and message.tool_calls):
            logger.warning("未检测到 message.tool_calls")
            return []

        for tool_call in message.tool_calls:
            func_name = tool_call.function.name
            raw_arguments = tool_call.function.arguments
            try:
                args = json_repair.loads(raw_arguments)
            except json.JSONDecodeError as error:
                repaired = fix_json_with_llm(raw_arguments, str(error))
                args = json_repair.loads(repaired)

            tool_calls.append({"tool_name": func_name, "arguments": args})

        for tool_call in tool_calls:
            logger.info("使用工具: %s", tool_call.get("tool_name"))
            logger.info("参数: %s", tool_call.get("arguments"))

        return tool_calls

    @staticmethod
    def execute_tools(
        tools: Dict[str, BaseTool],
        tool_calls: list,
        display_message: Optional[Callable[[str], None]] = None,
    ) -> list:
        """
        @brief 并行执行一组工具调用，返回 OpenAI 格式的 tool result 消息列表。

        @param tools 工具实例映射，key 为工具名。
        @param tool_calls OpenAI 原生 tool_call 对象列表。
        @param display_message 可选的消息显示回调。
        @return OpenAI 格式的 tool result 消息列表。
        """
        from concurrent.futures import ThreadPoolExecutor, as_completed

        def _run_one(tc):
            func_name = tc.function.name
            try:
                args = json.loads(tc.function.arguments)
            except json.JSONDecodeError:
                import json_repair
                args = json_repair.loads(tc.function.arguments)

            if display_message:
                display_message(f"  执行工具: {func_name}")

            if func_name in tools:
                try:
                    result = tools[func_name].execute(func_name, args)
                    if not result:
                        result = "注意！无输出内容！"
                except Exception as error:
                    result = f"工具执行出错: {str(error)}"
            else:
                result = f"错误: 未找到工具 '{func_name}'"

            logger.info("工具 %s 输出:\n%s", func_name, result)
            return tc.id, result

        results = []
        with ThreadPoolExecutor(max_workers=len(tool_calls)) as executor:
            futures = {executor.submit(_run_one, tc): tc for tc in tool_calls}
            for future in as_completed(futures):
                tc_id, output = future.result(timeout=120)
                results.append({
                    "tool_call_id": tc_id,
                    "role": "tool",
                    "content": str(output),
                })

        # 按原始 tool_calls 顺序排列结果
        tc_id_order = {tc.id: i for i, tc in enumerate(tool_calls)}
        results.sort(key=lambda r: tc_id_order.get(r["tool_call_id"], 0))
        return results
