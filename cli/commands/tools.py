"""tools 子命令。"""

from __future__ import annotations

import importlib
import inspect
import os
from typing import Any, Dict

import typer
from rich.console import Console
from rich.table import Table

from ctf_tool.base_tool import BaseTool

app = typer.Typer(help="工具信息")


@app.command("list")
def list_command() -> None:
    """列出可用工具。"""
    project_root = os.path.dirname(os.path.dirname(os.path.dirname(__file__)))
    tools_dir = os.path.join(project_root, "ctf_tool")

    function_configs = []
    for file_name in os.listdir(tools_dir):
        if file_name in {"__init__.py", "base_tool.py", "mcp_adapter.py"} or not file_name.endswith(".py"):
            continue

        module_name = file_name[:-3]
        try:
            module = importlib.import_module(f"ctf_tool.{module_name}")
        except Exception:
            continue

        for _, obj in inspect.getmembers(module):
            if inspect.isclass(obj) and issubclass(obj, BaseTool) and obj is not BaseTool:
                try:
                    instance = obj()
                    function_configs.append(instance.function_config)
                except Exception:
                    continue

    console = Console()
    if not function_configs:
        console.print("[yellow]未检测到可用工具[/yellow]")
        return

    table = Table(title="可用工具列表")
    table.add_column("工具名", style="cyan")
    table.add_column("描述", style="white")
    table.add_column("参数", style="green")
    table.add_column("启用", style="magenta", width=6)

    for cfg in function_configs:
        function_data: Dict[str, Any] = cfg.get("function", {})
        name = str(function_data.get("name", ""))
        description = str(function_data.get("description", ""))
        parameters = function_data.get("parameters", {})
        if isinstance(parameters, dict):
            props = parameters.get("properties", {})
            if isinstance(props, dict):
                param_summary = ", ".join(props.keys())
            else:
                param_summary = "-"
        else:
            param_summary = "-"

        table.add_row(name, description, param_summary or "-", "是")

    console.print(table)
