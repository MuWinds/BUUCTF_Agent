"""Rich + prompt_toolkit 的命令行交互实现。"""

from __future__ import annotations

from typing import List, Optional

from rich.console import Console
from rich.panel import Panel
from rich.rule import Rule
from rich.text import Text

from utils.user_interface import UserInterface

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
