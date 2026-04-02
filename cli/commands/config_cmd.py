"""config 子命令。"""

from __future__ import annotations

from typing import Any, Dict, List, Tuple

import typer
from rich.console import Console
from rich.panel import Panel
from rich.table import Table

from config import Config

app = typer.Typer(help="配置检查")


def _check_required_fields(config: Dict[str, Any]) -> List[Tuple[str, bool, str]]:
    """检查关键配置字段。"""
    llm = config.get("llm", {})
    platform = config.get("platform", {})
    inputer = platform.get("inputer", {})

    checks: List[Tuple[str, bool, str]] = [
        ("llm.model", isinstance(llm.get("model"), str) and bool(llm.get("model")), "模型名称"),
        ("llm.api_key", isinstance(llm.get("api_key"), str) and bool(llm.get("api_key")), "API Key"),
        ("llm.api_base", isinstance(llm.get("api_base"), str) and bool(llm.get("api_base")), "API Base"),
        (
            "platform.inputer.type",
            isinstance(inputer.get("type"), str) and bool(inputer.get("type")),
            "输入器类型",
        ),
    ]
    return checks


@app.command("check")
def check_command() -> None:
    """检查当前配置文件完整性。"""
    console = Console()
    try:
        config = Config.load_config()
    except Exception as error:
        console.print(
            Panel(
                str(error),
                title="配置读取失败",
                border_style="red",
            )
        )
        raise typer.Exit(code=1)

    checks = _check_required_fields(config)
    missing = [item for item in checks if not item[1]]

    summary = "配置完整" if not missing else f"存在 {len(missing)} 项缺失"
    console.print(Panel(summary, title="配置检查结果", border_style="green" if not missing else "yellow"))

    table = Table(title="关键配置项")
    table.add_column("字段", style="cyan")
    table.add_column("状态", style="magenta")
    table.add_column("说明", style="white")
    for field, ok, desc in checks:
        table.add_row(field, "OK" if ok else "缺失", desc)
    console.print(table)

    if missing:
        raise typer.Exit(code=1)

