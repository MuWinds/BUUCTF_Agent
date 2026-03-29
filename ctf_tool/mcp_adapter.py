"""@brief MCP 服务器工具适配器实现。"""

import asyncio
import atexit
import logging
import os
from contextlib import AsyncExitStack
from typing import Any, Dict, List, Optional, Tuple

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

from ctf_tool.base_tool import BaseTool

logger = logging.getLogger(__name__)


class MCPServerAdapter(BaseTool):
    """@brief 适配 MCP 服务端工具到统一工具接口。"""

    def __init__(self, server_config: Dict[str, Any]):
        """@brief 初始化 MCP 适配器并建立连接。

        @param server_config MCP 服务配置。
        """
        super().__init__()
        self.server_name = server_config["name"]
        self.server_config = server_config
        self.communication_mode = server_config.get("type", "http")
        self.session: Optional[ClientSession] = None
        self.exit_stack = AsyncExitStack()
        self.tools: Dict[str, Dict[str, Any]] = {}
        self.loop = asyncio.new_event_loop()
        self.base_url = ""
        self.auth_token: Optional[str] = None

        self.loop.run_until_complete(self._initialize_server())
        atexit.register(self._cleanup)

    async def _initialize_server(self) -> None:
        """@brief 初始化服务器连接。"""
        if self.communication_mode == "stdio" and "command" in self.server_config:
            await self._connect_stdio_server()
        elif self.communication_mode == "http" and "url" in self.server_config:
            self.base_url = self.server_config["url"]
            auth_token_value = self.server_config.get("auth_token")
            self.auth_token = auth_token_value if isinstance(auth_token_value, str) else None
            await self._load_http_tools()
        else:
            logger.error("不支持的通信模式或缺少必要配置: %s", self.communication_mode)

    async def _connect_stdio_server(self) -> None:
        """@brief 连接到 stdio 模式的 MCP 服务器。

        @raises RuntimeError 连接失败时抛出。
        """
        command = self.server_config["command"]
        args = self.server_config.get("args", [])
        working_dir = self.server_config.get("working_directory", os.getcwd())

        logger.info("连接到stdio模式MCP服务器: %s", self.server_name)
        logger.debug("命令: %s %s", command, " ".join(args))
        logger.debug("工作目录: %s", working_dir)

        try:
            server_params = StdioServerParameters(
                command=command,
                args=args,
                env=None,
                cwd=working_dir,
            )

            stdio_transport = await self.exit_stack.enter_async_context(
                stdio_client(server_params)
            )
            self.stdio, self.write = stdio_transport

            self.session = await self.exit_stack.enter_async_context(
                ClientSession(self.stdio, self.write)
            )

            await self.session.initialize()
            await self._load_stdio_tools()
        except Exception as error:
            logger.error("连接MCP服务器失败: %s", str(error))
            raise RuntimeError(f"无法连接MCP服务器: {str(error)}") from error

    async def _load_http_tools(self) -> None:
        """@brief 通过 HTTP 加载工具列表。"""
        if not self.base_url:
            logger.error("无法加载工具: 未指定服务URL")
            return

        try:
            import requests

            headers = {}
            if self.auth_token and self.auth_token.strip():
                headers["Authorization"] = f"Bearer {self.auth_token}"
            response = requests.get(
                f"{self.base_url}/tools",
                headers=headers,
                timeout=10,
            )
            response.raise_for_status()
            tools_info = response.json()
            self._process_tools_info(tools_info)
        except Exception as error:
            logger.error("加载MCP工具失败: %s", str(error))

    async def _load_stdio_tools(self) -> None:
        """@brief 通过 stdio 加载工具列表。"""
        if not self.session:
            logger.error("无法加载工具: 未连接到stdio服务")
            return

        try:
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
        except Exception as error:
            logger.error("加载MCP工具失败: %s", str(error))

    def _process_tools_info(self, tools_info: List[Dict[str, Any]]) -> None:
        """@brief 处理并缓存工具信息。

        @param tools_info 工具信息列表。
        """
        for tool_info in tools_info:
            tool_name = f"{tool_info['name']}"
            self.tools[tool_name] = {
                "description": tool_info.get("description", ""),
                "parameters": tool_info.get("parameters", {}),
            }

    def execute(self, tool_name: str, arguments: Dict[str, Any]) -> Tuple[str, str]:
        """@brief 执行 MCP 服务器上的工具。

        @param tool_name 工具名。
        @param arguments 工具参数。
        @return Tuple[str, str] 标准输出与错误输出。
        """
        if tool_name not in self.tools:
            return "", f"错误：未知的MCP工具 '{tool_name}'"

        return self.loop.run_until_complete(self._execute(tool_name, arguments))

    async def _execute(
        self,
        tool_name: str,
        arguments: Dict[str, Any],
    ) -> Tuple[str, str]:
        """@brief 内部异步执行入口。

        @param tool_name 工具名。
        @param arguments 工具参数。
        @return Tuple[str, str] 标准输出与错误输出。
        """
        if self.communication_mode == "http":
            return await self._execute_http(tool_name, arguments)
        if self.communication_mode == "stdio":
            return await self._execute_stdio(tool_name, arguments)

        return "", f"错误：不支持的通信模式 '{self.communication_mode}'"

    async def _execute_http(
        self,
        tool_name: str,
        arguments: Dict[str, Any],
    ) -> Tuple[str, str]:
        """@brief 通过 HTTP 执行工具。

        @param tool_name 工具名。
        @param arguments 工具参数。
        @return Tuple[str, str] 标准输出与错误输出。
        """
        try:
            import requests

            payload = {"tool": tool_name, "arguments": arguments}
            headers = {"Content-Type": "application/json"}
            if self.auth_token and self.auth_token.strip():
                headers["Authorization"] = f"Bearer {self.auth_token}"

            response = requests.post(
                f"{self.base_url}/execute",
                json=payload,
                headers=headers,
                timeout=self.server_config.get("timeout", 30),
            )
            response.raise_for_status()
            result = response.json()
            output = result.get("output", "")
            error_message = result.get("error", "")
            return str(output), str(error_message)
        except Exception as error:
            logger.error("MCP工具执行失败: %s", str(error))
            return "", f"MCP工具执行错误: {str(error)}"

    async def _execute_stdio(
        self,
        tool_name: str,
        arguments: Dict[str, Any],
    ) -> Tuple[str, str]:
        """@brief 通过 stdio 执行工具。

        @param tool_name 工具名。
        @param arguments 工具参数。
        @return Tuple[str, str] 标准输出与错误输出。
        """
        if not self.session:
            return "", "错误：未连接到stdio服务"

        try:
            result = await self.session.call_tool(tool_name, arguments)
            return str(result.content), ""
        except Exception as error:
            logger.error("MCP工具执行失败: %s", str(error))
            return "", f"MCP工具执行错误: {str(error)}"

    @property
    def function_config(self) -> Dict[str, Any]:
        """@brief 返回适配器自身函数配置。

        @return Dict[str, Any] 适配器函数配置。
        """
        return {}

    def get_tool_configs(self) -> List[Dict[str, Any]]:
        """@brief 为每个 MCP 工具生成函数配置。

        @return List[Dict[str, Any]] 工具函数配置列表。
        """
        configs: List[Dict[str, Any]] = []
        for tool_name, tool_info in self.tools.items():
            parameters = tool_info["parameters"]["properties"]
            config = {
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": tool_info["description"],
                    "parameters": {
                        "type": "object",
                        "properties": parameters["properties"],
                        "required": parameters.get("required", []),
                    },
                },
            }
            configs.append(config)

        return configs

    def _cleanup(self) -> None:
        """@brief 清理资源并关闭事件循环。"""
        self.loop.close()
