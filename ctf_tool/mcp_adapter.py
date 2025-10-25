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
        return {
            "type": "function",
            "function": {
                "name": f"mcp_adapter_{self.server_name}",
                "description": f"MCP服务器适配器: {self.server_name}",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "purpose": {
                            "type": "string",
                            "description": "执行此步骤的目的",
                        },
                        "tool_name": {
                            "type": "string",
                            "description": "要执行的工具名称",
                        },
                        "arguments": {
                            "type": "object",
                            "description": "工具参数",
                        },
                    },
                    "required": ["purpose", "tool_name", "arguments"],
                },
            },
        }

    def get_tool_configs(self) -> List[Dict]:
        """为每个MCP工具生成函数配置"""
        configs = []
        for tool_name, tool_info in self.tools.items():
            config = {
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": tool_info["description"],
                    "parameters": {
                        "type": "object",
                        "properties": {**tool_info["parameters"]["properties"]},
                        "required": ["purpose", "tool_name"]
                        + tool_info["parameters"].get("required", []),
                    },
                },
            }
            configs.append(config)
        return configs

    def _cleanup(self):
        """清理资源，关闭连接"""

        async def async_cleanup():
            await self.exit_stack.aclose()

        self.loop.run_until_complete(async_cleanup())
        self.loop.close()
