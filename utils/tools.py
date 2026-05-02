import importlib
import inspect
import json
import logging
import os
import re
from typing import Any, Callable, Dict, List, Optional, Tuple

import json_repair
import yaml
from jinja2 import Environment, FileSystemLoader

from config import Config
from ctf_tool.base_tool import BaseTool
from utils.llm_request import LLMRequest
from utils.text import fix_json_with_llm

logger = logging.getLogger(__name__)


class ToolUtils:
    """工具加载与工具响应处理工具类。

    负责加载本地工具与 MCP 工具、解析工具调用参数，
    并在输出过长时生成工具执行摘要。
    """

    def __init__(self):
        """初始化 ToolUtils。

        Raises:
            ValueError: 当配置文件不存在或读取失败时抛出。
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
        """加载工具并区分本地工具与 MCP 工具。

        Returns:
            二元组：(工具实例字典, 工具配置列表)。
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
    def format_tools_for_prompt(function_configs: List[Dict[str, Any]]) -> str:
        """将工具配置列表转为提示词可用的文本描述。

        Args:
            function_configs: 工具函数配置列表。

        Returns:
            格式化的工具描述文本。
        """
        if not function_configs:
            return "无可用工具"

        lines: List[str] = []
        for index, config in enumerate(function_configs, 1):
            func = config.get("function", {})
            name = func.get("name", "未知工具")
            desc = func.get("description", "无描述")
            params = func.get("parameters", {}).get("properties", {})
            required = func.get("parameters", {}).get("required", [])

            lines.append(f"{index}. {name}")
            lines.append(f"   描述: {desc}")
            if params:
                lines.append("   参数:")
                for param_name, param_info in params.items():
                    param_type = param_info.get("type", "string")
                    param_desc = param_info.get("description", "")
                    is_required = "必填" if param_name in required else "可选"
                    lines.append(
                        f"     - {param_name} ({param_type}, {is_required}): {param_desc}"
                    )
            lines.append("")

        return "\n".join(lines)

    @staticmethod
    def parse_tool_response(response: Any) -> List[Dict[str, Any]]:
        """从 LLM 文本响应中解析工具调用列表。

        支持从 message.content 文本中的 JSON 代码块提取 tool_calls 数组。
        向后兼容原生的 message.tool_calls 解析方式。

        Args:
            response: LLM 原始响应对象。

        Returns:
            工具调用列表，每项包含 tool_name 与 arguments。
        """
        message = response.choices[0].message

        # 优先尝试原生 tool_calls（兼容模式）
        if hasattr(message, "tool_calls") and message.tool_calls:
            tool_calls: List[Dict[str, Any]] = []
            for tool_call in message.tool_calls:
                func_name = tool_call.function.name
                raw_arguments = tool_call.function.arguments
                try:
                    args = json_repair.loads(raw_arguments)
                except json.JSONDecodeError as error:
                    repaired = fix_json_with_llm(raw_arguments, str(error))
                    args = json_repair.loads(repaired)
                tool_calls.append({"tool_name": func_name, "arguments": args})
            return tool_calls

        # 从文本内容中解析
        content = message.content
        if not content:
            logger.warning("消息内容为空，无法解析工具调用")
            return []

        content_str = content if isinstance(content, str) else str(content)

        # 尝试从 XML 块中提取
        xml_str = ToolUtils._extract_xml_block(content_str)
        if xml_str:
            return ToolUtils._parse_tool_calls_from_xml(xml_str)

        # 回退：尝试 JSON 代码块
        json_str = ToolUtils._extract_json_block(content_str)
        if json_str:
            return ToolUtils._parse_tool_calls_from_json(json_str)

        # 最后尝试整个内容作为 JSON
        return ToolUtils._parse_tool_calls_from_json(content_str)

    @staticmethod
    def _extract_xml_block(text: str) -> Optional[str]:
        """从文本中提取 <tool_calls> XML 块。

        Args:
            text: 原始文本。

        Returns:
            提取到的 XML 字符串，失败返回 None。
        """
        pattern = r"<tool_calls>\s*(.*?)\s*</tool_calls>"
        matches = re.findall(pattern, text, re.DOTALL)
        if matches:
            return f"<tool_calls>{matches[-1]}</tool_calls>"
        return None

    @staticmethod
    def _parse_tool_calls_from_xml(xml_str: str) -> List[Dict[str, Any]]:
        """
        @brief 从 XML 字符串中解析 tool_calls。

        支持的 XML 格式：
        <tool_calls>
          <tool_call name="工具名">
            <arg key="参数名">参数值</arg>
          </tool_call>
        </tool_calls>

        @param xml_str XML 字符串。
        @return 工具调用列表。
        """
        tool_calls: List[Dict[str, Any]] = []

        # 尝试用 xml.etree 解析
        try:
            import xml.etree.ElementTree as ET

            root = ET.fromstring(xml_str)
            for tc_elem in root.findall("tool_call"):
                tool_name = tc_elem.get("name", "")
                arguments: Dict[str, Any] = {}
                for arg_elem in tc_elem.findall("arg"):
                    key = arg_elem.get("key", "")
                    value = arg_elem.text or ""
                    if key:
                        arguments[key] = value
                if tool_name:
                    tool_calls.append({
                        "tool_name": tool_name,
                        "arguments": arguments,
                    })
        except Exception:
            logger.debug("xml.etree 解析失败，尝试正则回退")
            tool_calls = ToolUtils._parse_tool_calls_from_xml_regex(xml_str)

        for tool_call in tool_calls:
            logger.info("使用工具: %s", tool_call.get("tool_name"))
            logger.info("参数: %s", tool_call.get("arguments"))

        return tool_calls

    @staticmethod
    def _parse_tool_calls_from_xml_regex(xml_str: str) -> List[Dict[str, Any]]:
        """正则回退解析 XML 格式的 tool_calls。"""
        tool_calls: List[Dict[str, Any]] = []

        # 匹配每个 <tool_call name="...">...</tool_call>
        tc_pattern = r"<tool_call\s+name\s*=\s*\"(.*?)\">(.*?)</tool_call>"
        for match in re.finditer(tc_pattern, xml_str, re.DOTALL):
            tool_name = match.group(1)
            inner = match.group(2)
            arguments: Dict[str, Any] = {}

            # 匹配 <arg key="...">...</arg>
            arg_pattern = r"<arg\s+key\s*=\s*\"(.*?)\">(.*?)</arg>"
            for arg_match in re.finditer(arg_pattern, inner, re.DOTALL):
                key = arg_match.group(1)
                value = arg_match.group(2)
                arguments[key] = value

            if tool_name:
                tool_calls.append({
                    "tool_name": tool_name,
                    "arguments": arguments,
                })

        return tool_calls

    @staticmethod
    def _extract_json_block(text: str) -> Optional[str]:
        """从文本中提取 JSON 代码块。

        Args:
            text: 原始文本。

        Returns:
            提取到的 JSON 字符串，失败返回 None。
        """
        # 匹配 ```json ... ``` 代码块
        pattern = r"```json\s*\n(.*?)\n```"
        matches = re.findall(pattern, text, re.DOTALL)
        if matches:
            return matches[-1]

        # 匹配 ``` ... ``` 代码块
        pattern = r"```\s*\n(.*?)\n```"
        matches = re.findall(pattern, text, re.DOTALL)
        if matches:
            return matches[-1]

        return None

    @staticmethod
    def _parse_tool_calls_from_json(json_str: str) -> List[Dict[str, Any]]:
        """从 JSON 字符串中解析 tool_calls 数组。

        Args:
            json_str: JSON 字符串。

        Returns:
            工具调用列表。
        """
        tool_calls: List[Dict[str, Any]] = []
        try:
            data = json_repair.loads(json_str)
        except json.JSONDecodeError as error:
            repaired = fix_json_with_llm(json_str, str(error))
            try:
                data = json_repair.loads(repaired)
            except Exception:
                logger.warning("JSON修复后仍无法解析: %s", repaired[:200])
                return []

        if not isinstance(data, dict):
            logger.warning("解析结果不是字典类型")
            return []

        raw_tool_calls = data.get("tool_calls", [])
        if not isinstance(raw_tool_calls, list):
            logger.warning("tool_calls 不是数组类型")
            return []

        for tool_call in raw_tool_calls:
            if not isinstance(tool_call, dict):
                continue
            func_name = tool_call.get("tool_name", "")
            arguments = tool_call.get("arguments", {})
            if not isinstance(arguments, dict):
                arguments = {}
            tool_calls.append({"tool_name": func_name, "arguments": arguments})

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
        """执行一组工具调用并收集原始输出。

        Args:
            tools: 工具实例映射，key 为工具名。
            tool_calls: 工具调用计划列表，每项包含 tool_name 和 arguments。
            display_message: 可选的消息显示回调，用于输出执行进度。

        Returns:
            二元组：(工具结果列表, 合并后的原始输出字符串)。
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
