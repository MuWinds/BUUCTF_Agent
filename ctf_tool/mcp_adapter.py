import asyncio
from contextlib import AsyncExitStack
from typing import Dict, List, Optional
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client
from ctf_tool.base_tool import BaseTool
import logging
import os
import atexit

logger = logging.getLogger(__name__)


class MCPServerAdapter(BaseTool):
    def __init__(self, server_config: dict):
        super().__init__()
        self.server_name = server_config["name"]
        self.server_config = server_config
        self.communication_mode = server_config.get("type", "http")
        self.session: Optional[ClientSession] = None
        self.exit_stack = AsyncExitStack()
        self.tools = {}
        self.loop = asyncio.new_event_loop()

        # 初始化服务器连接
        self.loop.run_until_complete(self._initialize_server())

        # 注册退出时的清理函数
        atexit.register(self._cleanup)

    async def _initialize_server(self):
        """初始化服务器连接"""
        if self.communication_mode == "stdio" and "command" in self.server_config:
            await self._connect_stdio_server()
        elif self.communication_mode == "http" and "url" in self.server_config:
            self.base_url = self.server_config["url"]
            self.auth_token = self.server_config.get("auth_token", None)
            await self._load_http_tools()
        else:
            logger.error(f"不支持的通信模式或缺少必要配置: {self.communication_mode}")

    async def _connect_stdio_server(self):
        """连接到stdio模式的MCP服务器"""
        command = self.server_config["command"]
        args = self.server_config.get("args", [])
        working_dir = self.server_config.get("working_directory", os.getcwd())

        logger.info(f"连接到stdio模式MCP服务器: {self.server_name}")
        logger.debug(f"命令: {command} {' '.join(args)}")
        logger.debug(f"工作目录: {working_dir}")

        try:
            server_params = StdioServerParameters(
                command=command, args=args, env=None, cwd=working_dir
            )

            # 创建stdio连接
            stdio_transport = await self.exit_stack.enter_async_context(
                stdio_client(server_params)
            )
            self.stdio, self.write = stdio_transport

            # 创建客户端会话
            self.session = await self.exit_stack.enter_async_context(
                ClientSession(self.stdio, self.write)
            )

            # 初始化会话并加载工具
            await self.session.initialize()
            await self._load_stdio_tools()

        except Exception as e:
            logger.error(f"连接MCP服务器失败: {str(e)}")
            raise RuntimeError(f"无法连接MCP服务器: {str(e)}")

    async def _load_http_tools(self):
        """通过HTTP加载工具列表"""
        if not self.base_url:
            logger.error("无法加载工具: 未指定服务URL")
            return

        try:
            # 使用mcp的HTTP客户端加载工具
            # 注意: 这里假设mcp库有HTTP客户端实现
            # 如果没有，我们可以使用requests作为临时方案
            import requests

            headers = (
                {"Authorization": f"Bearer {self.auth_token}"}
                if self.auth_token
                else {}
            )
            response = requests.get(
                f"{self.base_url}/tools", headers=headers, timeout=10
            )
            response.raise_for_status()
            tools_info = response.json()
            self._process_tools_info(tools_info)
        except Exception as e:
            logger.error(f"加载MCP工具失败: {str(e)}")

    async def _load_stdio_tools(self):
        """通过stdio加载工具列表"""
        if not self.session:
            logger.error("无法加载工具: 未连接到stdio服务")
            return

        try:
            # 列出可用的工具
            response = await self.session.list_tools()
            tools_info = [
                {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": {
                        "properties": tool.inputSchema,
                        "required": list(tool.inputSchema.keys()),
                    },
                }
                for tool in response.tools
            ]

            self._process_tools_info(tools_info)
        except Exception as e:
            logger.error(f"加载MCP工具失败: {str(e)}")

    def _process_tools_info(self, tools_info: list):
        """处理工具信息"""
        for tool_info in tools_info:
            tool_name = f"{tool_info['name']}"
            self.tools[tool_name] = {
                "description": tool_info.get("description", ""),
                "parameters": tool_info.get("parameters", {}),
            }
            logger.info(f"已加载MCP工具: {tool_name}")

    def execute(self, tool_name: str, arguments: dict) -> tuple[str, str]:
        """执行MCP服务器上的工具"""
        if tool_name not in self.tools:
            return "", f"错误：未知的MCP工具 '{tool_name}'"
        return self.loop.run_until_complete(self._execute(tool_name, arguments))

    async def _execute(self, tool_name: str, arguments: dict):
        """内部异步执行方法"""
        if self.communication_mode == "http":
            return await self._execute_http(tool_name, arguments)
        elif self.communication_mode == "stdio":
            return await self._execute_stdio(tool_name, arguments)
        else:
            return "", f"错误：不支持的通信模式 '{self.communication_mode}'"

    async def _execute_http(self, tool_name: str, arguments: dict):
        """通过HTTP执行工具"""
        # 使用mcp的HTTP客户端执行工具
        # 如果没有HTTP客户端实现，使用requests作为临时方案
        try:
            import requests

            payload = {"tool": tool_name, "arguments": arguments}

            headers = {"Content-Type": "application/json"}
            if self.auth_token:
                headers["Authorization"] = f"Bearer {self.auth_token}"

            response = requests.post(
                f"{self.base_url}/execute",
                json=payload,
                headers=headers,
                timeout=self.server_config.get("timeout", 30),
            )
            response.raise_for_status()
            result = response.json()
            return result.get("output", ""), result.get("error", "")
        except Exception as e:
            logger.error(f"MCP工具执行失败: {str(e)}")
            return "", f"MCP工具执行错误: {str(e)}"

    async def _execute_stdio(self, tool_name: str, arguments: dict):
        """通过stdio执行工具"""
        if not self.session:
            return "", "错误：未连接到stdio服务"

        try:
            # 调用工具
            result = await self.session.call_tool(tool_name, arguments)
            return result.content, ""
        except Exception as e:
            logger.error(f"MCP工具执行失败: {str(e)}")
            return "", f"MCP工具执行错误: {str(e)}"

    @property
    def function_config(self) -> Dict:
        """实现BaseTool要求的属性 - 返回适配器本身的配置"""
        return {}

    def get_tool_configs(self) -> List[Dict]:
        """为每个MCP工具生成函数配置"""
        # configs = []
        configs: List[Dict[str, Any]] = []
        for tool_name, tool_info in self.tools.items():
            # 1. 安全提取核心字段（避免 KeyError，提升健壮性）
            tool_desc = tool_info.get("description", f"执行{tool_name}工具")
            tool_params = tool_info.get("parameters", {})
            raw_props = tool_params.get("properties", {})

            # 2. 核心处理：确保 properties 是「参数名→描述」的扁平字典（关键适配 tool_calls）
            valid_props = {}
            if isinstance(raw_props, dict):
                # 处理可能的嵌套 properties（如之前的套娃问题）
                if "properties" in raw_props and isinstance(raw_props["properties"], dict):
                    valid_props = raw_props["properties"]  # 扁平化嵌套
                else:
                    valid_props = {**raw_props}  # 解包原始字典（保留原有逻辑）
            elif isinstance(raw_props, list):
                # 处理数组格式：转为字典（用 name 作为键，适配 arguments 结构）
                for param in raw_props:
                    if isinstance(param, dict) and "name" in param:
                        param_name = param.pop("name")
                        # 确保参数有 type（OpenAI 要求，否则模型可能忽略）
                        if "type" not in param:
                            param["type"] = "string"
                        valid_props[param_name] = param

            # 3. 处理 required：确保是数组，避免非法格式（适配模型参数校验）
            required_params = tool_params.get("required", [])
            required_params = required_params if isinstance(required_params, list) else []

            # 4. 构建最终配置（严格适配 OpenAI Tool 规范，确保生成正确的 tool_calls）
            config = {
                "type": "function",
                "function": {
                    "name": tool_name,  # 对应 tool_calls[0].name
                    "description": tool_desc,
                    "parameters": {
                        "type": "object",  # 强制参数为对象，对应 tool_calls[0].arguments
                        "properties": valid_props,  # 对应 arguments 中的键值对（如 purpose、content）
                        "required": required_params,  # 模型会自动校验必填参数
                        "additionalProperties": False  # 禁止额外参数，避免 arguments 冗余
                    }
                }
            }
            configs.append(config)
        return configs

    def _cleanup(self):
        """清理资源，关闭连接"""
        self.loop.close()
