"""Typer 应用入口。"""

from __future__ import annotations

import importlib
import pkgutil
from pathlib import Path
from typing import Any

import typer

from cli.commands.checkpoint import app as checkpoint_app
from cli.commands.config_cmd import app as config_app
from cli.commands.resume import resume_command
from cli.commands.skill import app as skill_app
from cli.commands.solve import solve_command
from cli.commands.tools import app as tools_app

app = typer.Typer(
    name="buuctf-agent",
    help="BUUCTF Agent 命令行工具",
    no_args_is_help=True,
)

# 注册核心命令
app.command("solve")(solve_command)
app.command("resume")(resume_command)
app.add_typer(checkpoint_app, name="checkpoint")
app.add_typer(config_app, name="config")
app.add_typer(skill_app, name="skill")
app.add_typer(tools_app, name="tools")

# 自动发现平台 CLI 命令模块（定义了 register 函数的模块）
_commands_dir = Path(__file__).parent / "commands"
for _module_info in pkgutil.iter_modules([str(_commands_dir)]):
    _module = importlib.import_module(f"cli.commands.{_module_info.name}")
    _register_fn = getattr(_module, "register", None)
    if callable(_register_fn):
        _register_fn(app)


def run() -> None:
    """运行 CLI 应用。"""
    app()


if __name__ == "__main__":
    run()
