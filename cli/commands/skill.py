"""skill 子命令。"""

from __future__ import annotations

import typer
from rich.console import Console
from rich.table import Table

from config import Config
from skill.manager import SkillManager

app = typer.Typer(help="Skill 知识管理")


def _get_manager() -> SkillManager:
    """创建 SkillManager 实例。"""
    config = Config.load_config()
    skill_paths = config.get("skills", {}).get("paths", [])
    return SkillManager(extra_paths=skill_paths)


@app.command("list")
def list_command() -> None:
    """列出所有可用 skill。"""
    console = Console()
    manager = _get_manager()
    skills = manager.get_all()

    if not skills:
        console.print("[yellow]未找到任何 skill[/yellow]")
        console.print(
            "请在 skills/ 目录下创建 SKILL.md 文件，"
            "或在 config.json 中配置 skills.paths"
        )
        return

    table = Table(title="可用 Skill 列表")
    table.add_column("名称", style="cyan")
    table.add_column("描述", style="white")
    table.add_column("位置", style="dim")

    for skill in skills:
        table.add_row(
            skill.name,
            skill.description,
            skill.location,
        )

    console.print(table)


@app.command("show")
def show_command(
    name: str = typer.Argument(help="Skill 名称"),
) -> None:
    """查看指定 skill 的详细内容。"""
    manager = _get_manager()
    skill = manager.get(name)

    if skill is None:
        typer.echo(f"未找到 skill: {name}")
        available = manager.get_names()
        if available:
            typer.echo(f"可用 skill: {', '.join(available)}")
        raise typer.Exit(1)

    # 使用 typer.echo 输出，自动处理编码
    typer.echo(f"Skill: {skill.name}")
    typer.echo(f"位置: {skill.location}")
    typer.echo(f"\n{skill.description}\n")

    if skill.content:
        typer.echo(skill.content)
