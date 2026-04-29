"""resume 子命令。"""

from __future__ import annotations

import typer

from cli.commands.solve import solve_command


def resume_command(
    auto: bool = typer.Option(False, "--auto", help="恢复后使用自动模式"),
    manual: bool = typer.Option(False, "--manual", help="恢复后使用手动模式"),
    plain: bool = typer.Option(False, "--plain", help="关闭彩色输出"),
    show_think: bool = typer.Option(
        True,
        "--show-think/--hide-think",
        help="是否显示思考摘要",
    ),
) -> None:
    """优先从存档恢复执行。"""
    solve_command(
        question_file=None,
        question=None,
        auto=auto,
        manual=manual,
        resume=True,
        show_think=show_think,
        plain=plain,
    )

