"""checkpoint 子命令。"""

from __future__ import annotations

import os
from datetime import datetime
from typing import Any, Dict, Optional

import typer
from rich.console import Console
from rich.table import Table

from agent.checkpoint import CheckpointManager
from cli.adapters.workflow_runner import clear_all_checkpoints, load_checkpoint_file
from config import Config

app = typer.Typer(help="存档管理")


def _checkpoint_manager() -> CheckpointManager:
    config = Config.load_config()
    checkpoint_dir = config.get("checkpoint_dir", "./checkpoints")
    if not isinstance(checkpoint_dir, str):
        checkpoint_dir = "./checkpoints"
    return CheckpointManager(checkpoint_dir=checkpoint_dir)


@app.command("list")
def list_command() -> None:
    """列出全部存档。"""
    manager = _checkpoint_manager()
    files = manager.list_checkpoints()

    console = Console()
    table = Table(title="存档列表")
    table.add_column("文件名", style="cyan")
    table.add_column("步骤", style="green", width=8)
    table.add_column("模式", style="magenta", width=8)
    table.add_column("更新时间", style="white")
    table.add_column("题目摘要", style="yellow")

    if not files:
        console.print("[yellow]当前没有存档[/yellow]")
        return

    for file_name in files:
        data: Optional[Dict[str, Any]] = load_checkpoint_file(manager, file_name)
        if not data:
            table.add_row(file_name, "-", "-", "-", "读取失败")
            continue

        file_path = os.path.join(manager.checkpoint_dir, file_name)
        mtime = datetime.fromtimestamp(os.path.getmtime(file_path)).strftime("%Y-%m-%d %H:%M:%S")

        step = str(data.get("step_count", "-"))
        mode = "自动" if data.get("auto_mode") else "手动"
        problem = str(data.get("problem", "")).replace("\n", " ").strip()
        preview = problem[:40] + ("..." if len(problem) > 40 else "")
        table.add_row(file_name, step, mode, mtime, preview)

    console.print(table)


@app.command("clear")
def clear_command(
    yes: bool = typer.Option(False, "--yes", "-y", help="跳过确认并直接清空"),
) -> None:
    """清空全部存档。"""
    manager = _checkpoint_manager()
    files = manager.list_checkpoints()
    if not files:
        typer.echo("当前没有存档，无需清理")
        return

    if not yes:
        confirmed = typer.confirm(f"确认清空 {len(files)} 个存档吗？")
        if not confirmed:
            typer.echo("已取消")
            return

    deleted = clear_all_checkpoints(manager)
    typer.echo(f"已清空 {deleted} 个存档")

