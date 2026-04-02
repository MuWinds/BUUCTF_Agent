"""Rich + prompt_toolkit 的命令行交互实现。"""

from __future__ import annotations

import json
from typing import Any, List, Optional

from rich.console import Console
from rich.panel import Panel
from rich.rule import Rule
from rich.table import Table
from rich.text import Text

from utils.user_interface import (
    ManualApprovalStepData,
    ToolCall,
    UserInterface,
)

try:
    from prompt_toolkit import PromptSession
    from prompt_toolkit.completion import WordCompleter
except ImportError:
    PromptSession = None
    WordCompleter = None


class RichPromptToolkitInterface(UserInterface):
    """基于 Rich 与 prompt_toolkit 的用户交互实现。"""

    def __init__(
        self,
        plain: bool = False,
        show_think: bool = True,
        forced_auto_mode: Optional[bool] = None,
        forced_resume: Optional[bool] = None,
    ) -> None:
        self.console = Console(no_color=plain, force_terminal=not plain)
        self.show_think = show_think
        self.forced_auto_mode = forced_auto_mode
        self.forced_resume = forced_resume
        self._session = PromptSession() if PromptSession else None

    def _prompt(self, text: str) -> str:
        """统一输入方法，支持 prompt_toolkit 不可用时回退。"""
        if self._session:
            return self._session.prompt(text)
        return input(text)

    def _prompt_choice(
        self,
        prompt_text: str,
        choices: List[str],
        default: Optional[str] = None,
    ) -> str:
        """读取单项选择输入。"""
        completer = WordCompleter(choices) if (WordCompleter and choices) else None

        while True:
            full_prompt = prompt_text
            if default:
                full_prompt = f"{prompt_text} [{default}] "
            response = (
                self._session.prompt(full_prompt, completer=completer)
                if self._session
                else input(full_prompt)
            )
            response = response.strip().lower()
            if not response and default:
                return default
            if response in choices:
                return response
            self.render_warning(f"无效输入：{response or '<空>'}，可选: {', '.join(choices)}")

    def render_info(self, message: str) -> None:
        """渲染普通信息。"""
        self.console.print(f"[cyan][INFO][/cyan] {message}")

    def render_success(self, message: str) -> None:
        """渲染成功信息。"""
        self.console.print(f"[green][OK][/green] {message}")

    def render_warning(self, message: str) -> None:
        """渲染警告信息。"""
        self.console.print(f"[yellow][WARN][/yellow] {message}")

    def render_error(self, message: str) -> None:
        """渲染错误信息。"""
        self.console.print(f"[bold red][ERROR][/bold red] {message}")

    def render_step_header(self, step_no: int) -> None:
        """渲染步骤标题。"""
        self.console.print(Rule(f"第 {step_no} 步"))

    def display_startup(
        self,
        mode_text: str,
        question_source: str,
        attachments_dir: str,
        checkpoint_status: str,
    ) -> None:
        """显示启动信息面板。"""
        panel_text = (
            f"模式: {mode_text}\n"
            f"题目来源: {question_source}\n"
            f"附件目录: {attachments_dir}\n"
            f"存档: {checkpoint_status}"
        )
        self.console.print(
            Panel(
                panel_text,
                title="BUUCTF Agent",
                border_style="blue",
            )
        )

    def confirm_flag(self, flag_candidate: str) -> bool:
        """候选 flag 确认。"""
        panel = Panel(
            Text(flag_candidate, style="bold yellow"),
            title="候选 Flag",
            border_style="yellow",
        )
        self.console.print(panel)
        choice = self._prompt_choice("确认该 flag 正确？(y/n): ", ["y", "n"], default="y")
        return choice == "y"

    def select_mode(self) -> bool:
        """选择自动/手动模式。"""
        if self.forced_auto_mode is not None:
            mode_name = "自动模式" if self.forced_auto_mode else "手动模式"
            self.render_info(f"已由命令参数指定：{mode_name}")
            return self.forced_auto_mode

        mode_table = Table(title="运行模式选择", show_header=True, header_style="bold magenta")
        mode_table.add_column("编号", style="cyan", width=6)
        mode_table.add_column("模式", style="white", width=10)
        mode_table.add_column("说明", style="green")
        mode_table.add_row("1", "自动", "自动执行全部步骤")
        mode_table.add_row("2", "手动", "每一步均需人工审批")
        self.console.print(mode_table)

        choice = self._prompt_choice("请选择模式 (1/2): ", ["1", "2"], default="1")
        return choice == "1"

    def input_question_ready(self, prompt: str) -> None:
        """等待用户确认题目已准备。"""
        self._prompt(prompt)

    def display_message(self, message: str) -> None:
        """根据消息内容进行轻量级分级渲染。"""
        normalized = message.strip()
        if not normalized:
            return

        if "错误" in normalized or "失败" in normalized:
            self.render_error(normalized)
            return
        if "警告" in normalized:
            self.render_warning(normalized)
            return
        if "正在思考第" in normalized:
            step_text = normalized.replace("\n", "").replace("正在思考第", "").replace("步...", "")
            if step_text.isdigit():
                self.render_step_header(int(step_text))
            else:
                self.console.print(Rule(normalized))
            return

        if "执行工具" in normalized:
            self.render_info(normalized)
            return

        self.console.print(normalized)

    def manual_approval(self, think: str, tool_calls: Any) -> tuple[bool, tuple[str, Any]]:
        """兼容旧接口：审批并返回固定结构。"""
        if self.show_think:
            self.console.print(
                Panel(think, title="思考摘要", border_style="cyan")
            )

        self.console.print(Panel(str(tool_calls), title="工具调用", border_style="blue"))
        choice = self._prompt_choice("是否批准执行？(y/n): ", ["y", "n"], default="y")
        return choice == "y", (think, tool_calls)

    @staticmethod
    def _args_preview(arguments: Any) -> str:
        """将参数压缩为短文本。"""
        preview = json.dumps(arguments, ensure_ascii=False)
        return preview if len(preview) <= 120 else f"{preview[:117]}..."

    def manual_approval_step(
        self,
        think: str,
        tool_calls: List[ToolCall],
    ) -> tuple[bool, ManualApprovalStepData]:
        """手动模式完整审批流程。"""
        if self.show_think:
            self.console.print(
                Panel(think, title="思考摘要", border_style="cyan")
            )

        tool_table = Table(title=f"计划调用工具 ({len(tool_calls)} 个)")
        tool_table.add_column("#", style="cyan", width=4)
        tool_table.add_column("工具名", style="green", width=30)
        tool_table.add_column("参数摘要", style="white")
        for index, tool_call in enumerate(tool_calls, start=1):
            tool_table.add_row(
                str(index),
                str(tool_call.get("tool_name", "")),
                self._args_preview(tool_call.get("arguments", {})),
            )
        self.console.print(tool_table)

        self.console.print("[bold]审批动作:[/bold] [Enter/y] 批准  [f] 反馈  [q] 终止")
        choice = self._prompt_choice("选择动作: ", ["", "y", "f", "q"], default="")

        if choice in {"", "y"}:
            return True, (think, tool_calls)
        if choice == "f":
            feedback = self._prompt("请提供改进建议: ").strip()
            return False, (think, tool_calls, feedback)
        return False, None

    def confirm_resume(self) -> bool:
        """确认是否恢复存档。"""
        if self.forced_resume is not None:
            msg = "恢复存档" if self.forced_resume else "不恢复存档"
            self.render_info(f"已由命令参数指定：{msg}")
            return self.forced_resume

        self.console.print(
            Panel(
                "检测到未完成的解题存档",
                title="存档恢复",
                border_style="yellow",
            )
        )
        choice = self._prompt_choice(
            "是否恢复存档？(y/n): ",
            ["y", "n"],
            default="y",
        )
        return choice == "y"

