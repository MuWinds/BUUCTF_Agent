import importlib
import inspect
import json
import logging
import os
from typing import Any, Callable, Dict, List, Optional, Tuple, Union

import json_repair
import yaml
from jinja2 import Environment, FileSystemLoader

from config import Config
from ctf_tool.base_tool import BaseTool
from utils.llm_request import LLMRequest
from utils.text import fix_json_with_llm

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
        tool_calls: List[Dict[str, Any]],
        display_message: Optional[Callable[[str], None]] = None,
    ) -> Tuple[List[Dict[str, Any]], str]:
        """
        @brief 执行一组工具调用并收集原始输出。

        @param tools 工具实例映射，key 为工具名。
        @param tool_calls 工具调用计划列表，每项包含 tool_name 和 arguments。
        @param display_message 可选的消息显示回调，用于输出执行进度。
        @return 二元组：(工具结果列表, 合并后的原始输出字符串)。
        """
        all_tool_results: List[Dict[str, Any]] = []
        combined_raw_output = ""

        for index, tool_call in enumerate(tool_calls):
            tool_name = tool_call.get("tool_name")
            arguments: Dict[str, Any] = tool_call.get("arguments", {})

            if display_message is not None:
                display_message(
                    f"\n执行工具 {index + 1}/{len(tool_calls)}: "
                    f"{tool_name}"
                )

            if tool_name in tools:
                try:
                    tool = tools[tool_name]
                    result = tool.execute(tool_name, arguments)
                    if not result:
                        result = "注意！无输出内容！"

                    tool_result = {
                        "tool_name": tool_name,
                        "arguments": arguments,
                        "raw_output": result,
                    }
                    all_tool_results.append(tool_result)
                    combined_raw_output += str(result) + "\n---\n"
                except Exception as error:
                    error_msg = f"工具执行出错: {str(error)}"
                    tool_result = {
                        "tool_name": tool_name,
                        "arguments": arguments,
                        "raw_output": error_msg,
                    }
                    all_tool_results.append(tool_result)
                    combined_raw_output += error_msg + "\n---\n"
            else:
                error_msg = f"错误: 未找到工具 '{tool_name}'"
                tool_result = {
                    "tool_name": tool_name,
                    "arguments": arguments,
                    "raw_output": error_msg,
                }
                all_tool_results.append(tool_result)
                combined_raw_output += error_msg + "\n---\n"

            logger.info(
                "工具 %s 原始输出:\n%s",
                tool_name,
                all_tool_results[-1]["raw_output"],
            )

        return all_tool_results, combined_raw_output

    @staticmethod
    def output_summary(
        tool_results: Union[List[Dict[str, Any]], Dict[str, Any], str, None] = None,
        think: str = "",
        tool_output: str = "",
        tool_name: Optional[str] = None,
    ) -> str:
        """
        @brief 汇总工具输出。

        @details
        当工具合并输出较短时直接返回；当输出过长时调用 LLM 生成摘要，
        若摘要失败则回退为简化截断输出。

        @param tool_results 工具执行结果列表。
        @param think 当前执行思路。
        @param tool_output 合并后的原始输出文本。
        @param tool_name 兼容旧接口保留参数（当前未使用）。
        @return 工具输出摘要文本。
        """
        del tool_name

        all_tool_results: List[Dict[str, Any]] = []
        combined_output = ""
        actual_think = think

        if isinstance(tool_results, list) and tool_results:
            all_tool_results = tool_results
            combined_output = tool_output

        if not all_tool_results:
            return ""

        if len(str(combined_output)) <= 1024:
            return str(combined_output)

        tool_details = []
        for index, result in enumerate(all_tool_results, start=1):
            current_tool_name = result.get("tool_name", f"工具{index}")
            arguments = result.get("arguments", {})
            raw_output = result.get("raw_output", "")
            output_preview = str(raw_output)[:200] + (
                "..." if len(str(raw_output)) > 200 else ""
            )

            tool_details.append(
                f"工具{index} [{current_tool_name}]:\n"
                f"参数: {arguments}\n"
                f"输出预览: {output_preview}"
            )

        tool_details_str = "\n---\n".join(tool_details)

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
            content = response.choices[0].message.content
            return content if isinstance(content, str) else str(content)
        except Exception as error:
            logger.error("工具输出总结失败: %s", error)
            simplified_outputs = []
            for index, result in enumerate(all_tool_results, start=1):
                current_tool_name = result.get("tool_name", f"工具{index}")
                raw_output = result.get("raw_output", "")
                simplified = str(raw_output)[:500] + (
                    "..." if len(str(raw_output)) > 500 else ""
                )
                simplified_outputs.append(f"{current_tool_name}: {simplified}")

            if tool_count > 1:
                return (
                    "工具执行摘要（总结失败，显示简化输出，"
                    f"共{tool_count}个工具）：\n"
                    + "\n---\n".join(simplified_outputs)
                )
            return simplified_outputs[0] if simplified_outputs else "无输出"
