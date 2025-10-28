import litellm
import re
import os
import json
import logging
import importlib
import inspect
from ctf_tool.base_tool import BaseTool
from json_repair import repair_json
from typing import Dict, Tuple
from config import Config

logger = logging.getLogger(__name__)


def fix_json_with_llm(json_str: str, err_content: str) -> str:
    """
    使用LLM修复格式错误的JSON
    :param json_str: 格式错误的JSON字符串
    :param config: LLM配置
    :return: 修复后的字典
    """
    config: dict = Config.load_config()
    litellm.enable_json_schema_validation = True
    prompt = (
        "以下是一个格式错误的JSON字符串，请修复它使其成为有效的JSON。"
        "只返回修复后的JSON，不要包含任何其他内容。"
        "确保保留所有原始键值对，不要改动里面的内容\n\n"
        f"错误JSON: {json_str}"
        f"错误信息: {err_content}"
    )
    llm_config = config["llm"]["pre_processor"]

    while True:
        try:
            if json.loads(repair_json(json_str)):
                return repair_json(json_str)
        except:
            response = litellm.completion(
                model=llm_config["model"],
                api_key=llm_config["api_key"],
                api_base=llm_config["api_base"],
                messages=[{"role": "user", "content": prompt}],
            )
            json_str = response.choices[0].message.content
            continue


def optimize_text(text: str) -> str:
    # 把连续 2 个及以上空格 → 先统一替换成一个特殊标记
    text = re.sub(r" {2,}", "\x00", text)  # \x00 几乎不会出现在正文里
    # 把特殊标记还原成唯一一个空格
    text = text.replace("\x00", " ")
    text = re.sub(r"\n+", "\n", text)
    lines = [line for line in text.splitlines() if line.strip()]
    return "\n".join(lines)


def load_tools() -> Tuple[Dict, list]:
    """动态加载tool文件夹中的所有工具"""
    # 加载配置文件
    config = Config.load_config()
    tools_dir = os.path.join(os.path.dirname(__file__), "..", "ctf_tool")

    # 初始化工具字典和配置列表
    tools = {}
    function_configs = []

    # 加载本地工具
    for file_name in os.listdir(tools_dir):
        if (
            file_name.endswith(".py")
            and file_name != "__init__.py"
            and file_name != "base_tool.py"
            and file_name != "mcp_adapter.py"  # 排除适MCP文件
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
                        tool_name = tool_instance.function_config["function"]["name"]
                        tools[tool_name] = tool_instance

                        # 添加工具配置
                        function_configs.append(tool_instance.function_config)

                        logger.info(f"已加载工具: {tool_name}")
            except Exception as e:
                logger.warning(f"加载工具{module_name}失败: {str(e)}")

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
                tool_description = mcp_tool_config["function"].get("description", "")

                # 适配器实例负责执行此工具
                tools[tool_name] = adapter
                function_configs.append(mcp_tool_config)
                logger.info(f"已加载MCP工具: {tool_name}")
        except Exception as e:
            logger.error(f"加载MCP服务器失败: {str(e)}")

    return tools, function_configs
