"""Manage agent lifecycle for the WebUI."""
from __future__ import annotations

import os
import queue
import threading
from typing import Optional

from config import Config
from agent.workflow import Workflow
from .event_bus import EventBus


class AgentSession:
    """Orchestrates background execution and surfaces events to the WebUI."""

    def __init__(self, event_bus: EventBus) -> None:
        self._event_bus = event_bus
        self._thread: Optional[threading.Thread] = None
        self._stop_event = threading.Event()
        self._status_lock = threading.Lock()
        self._status = "idle"
        self._flag_lock = threading.Lock()
        self._flag_queue: Optional[queue.Queue] = None

        os.makedirs("attachments", exist_ok=True)

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------
    def is_running(self) -> bool:
        with self._status_lock:
            return self._status in {"running", "awaiting_flag", "terminating"}

    def get_status(self) -> str:
        with self._status_lock:
            return self._status

    def start(self, question: str) -> None:
        question = question.strip()
        if not question:
            raise ValueError("题面内容不能为空")

        if self.is_running():
            raise RuntimeError("Agent 正在运行，无法重复启动")

        self._stop_event.clear()
        self._set_status("running")

        self._thread = threading.Thread(
            target=self._run_agent,
            args=(question,),
            name="AgentRunner",
            daemon=True,
        )
        self._thread.start()

    def terminate(self) -> None:
        if not self.is_running():
            return

        self._set_status("terminating")
        self._stop_event.set()
        self._resolve_flag_decision(False)

    def decide_flag(self, approve: bool) -> bool:
        decision_taken = self._resolve_flag_decision(approve)
        if decision_taken and self.is_running():
            self._set_status("running")
        return decision_taken

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------
    def _run_agent(self, question: str) -> None:
        try:
            config = Config.load_config()
            original_mode = os.environ.get("AGENT_PYTHON_MODE")
            original_non_interactive = os.environ.get("AGENT_NON_INTERACTIVE")

            os.environ["AGENT_NON_INTERACTIVE"] = "1"
            python_tool = (
                config.get("tool_config", {}).get("python")
                or config.get("tool_config", {}).get("PythonTool")
                or {}
            )
            default_mode = python_tool.get("default_mode")
            if default_mode in {"local", "remote"}:
                os.environ["AGENT_PYTHON_MODE"] = default_mode
            elif original_mode is None:
                os.environ["AGENT_PYTHON_MODE"] = "local"

            workflow = Workflow(config=config)

            result = workflow.solve(
                question,
                auto_mode=True,
                event_callback=self._event_bus.publish,
                stop_event=self._stop_event,
                confirm_handler=self._handle_flag_prompt,
            )

            self._event_bus.publish({"type": "run_completed", "result": result})
        except Exception as exc:  # pragma: no cover - runtime safeguard
            self._event_bus.publish({"type": "run_error", "message": str(exc)})
        finally:
            if original_mode is None:
                os.environ.pop("AGENT_PYTHON_MODE", None)
            else:
                os.environ["AGENT_PYTHON_MODE"] = original_mode

            if original_non_interactive is None:
                os.environ.pop("AGENT_NON_INTERACTIVE", None)
            else:
                os.environ["AGENT_NON_INTERACTIVE"] = original_non_interactive

            self._stop_event.clear()
            with self._flag_lock:
                self._flag_queue = None
            self._set_status("idle")
            self._thread = None

    def _handle_flag_prompt(self, flag_candidate: str) -> bool:
        pending_queue: queue.Queue = queue.Queue()
        with self._flag_lock:
            self._flag_queue = pending_queue

        self._set_status("awaiting_flag")
        self._event_bus.publish(
            {"type": "flag_confirmation", "flag": flag_candidate}
        )

        try:
            while not self._stop_event.is_set():
                try:
                    decision = pending_queue.get(timeout=0.5)
                    return bool(decision)
                except queue.Empty:
                    continue
            return False
        finally:
            with self._flag_lock:
                self._flag_queue = None
            if self.is_running() and not self._stop_event.is_set():
                self._set_status("running")

    def _resolve_flag_decision(self, approve: bool) -> bool:
        with self._flag_lock:
            if not self._flag_queue:
                return False
            try:
                self._flag_queue.put_nowait(approve)
                return True
            finally:
                self._flag_queue = None

    def _set_status(self, status: str) -> None:
        with self._status_lock:
            self._status = status
        self._event_bus.publish({"type": "status", "status": status})
