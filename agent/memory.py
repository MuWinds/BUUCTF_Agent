"""解题过程记忆管理模块。"""

import json
import logging
from typing import Any, Dict, List

import json_repair

from config import Config
from utils.llm_request import LLMRequest
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class Memory:
    """管理解题历史、关键事实与压缩记忆。

    基于 token 估算的上下文占用率触发压缩：
    当 get_summary() 的估算 token 数 >= context_window * compression_ratio 时自动压缩。
    """

    def __init__(
        self,
        context_window: int = 128000,
        compression_ratio: float = 0.8,
    ) -> None:
        """初始化记忆对象。

        Args:
            context_window: 模型上下文窗口大小（token 数）。
            compression_ratio: 触发压缩的上下文占用比例。
        """
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.context_window = context_window
        self.compression_ratio = compression_ratio
        self.history: List[Dict[str, Any]] = []
        self.compressed_memory: List[Dict[str, Any]] = []
        self.key_facts: Dict[str, str] = {}
        self.failed_attempts: Dict[str, int] = {}

        self._token_limit = int(context_window * compression_ratio)

    @staticmethod
    def _estimate_tokens(text: str) -> int:
        """简单估算文本的 token 数量（约 4 字符/token）。"""
        return max(1, len(text) // 4)

    def _current_usage_tokens(self) -> int:
        """估算当前 get_summary() 会占据的 token 数。"""
        summary = self.get_summary()
        return self._estimate_tokens(summary)

    def _should_compress(self) -> bool:
        """判断当前上下文占用是否达到压缩阈值。"""
        return self._current_usage_tokens() >= self._token_limit

    def add_step(self, step: Dict[str, Any]) -> None:
        """添加执行步骤并在 token 超限时触发压缩。

        Args:
            step: 步骤数据字典，包含工具调用、结果与分析等信息。
        """
        self.history.append(step)

        self._extract_key_facts(step)

        if "analysis" in step and "success" in step["analysis"]:
            if not step["analysis"]["success"]:
                if "tool_calls" in step and step["tool_calls"]:
                    command = str(
                        [
                            (t.get("tool_name"), t.get("arguments"))
                            for t in step["tool_calls"]
                        ]
                    )
                else:
                    command = str(step.get("tool_args", ""))
                self.failed_attempts[command] = (
                    self.failed_attempts.get(command, 0) + 1
                )

        if self._should_compress():
            self.compress_memory()

    def add_planned_step(
        self,
        step_num: int,
        think: str,
        tool_calls: List[Dict[str, Any]],
    ) -> None:
        """记录尚未执行的计划步骤，token 超限时触发压缩。

        Args:
            step_num: 步骤编号。
            think: 当前步骤的思考内容。
            tool_calls: 工具调用列表。
        """
        self.history.append(
            {
                "step": step_num,
                "think": think,
                "tool_calls": tool_calls,
                "status": "planned",
            }
        )

        if self._should_compress():
            self.compress_memory()

    def update_step(self, step_num: int, fields: Dict[str, Any]) -> None:
        """按步骤号更新历史记录并在 token 超限时触发压缩。

        Args:
            step_num: 要更新的步骤编号。
            fields: 需要更新的字段字典。
        """
        for index in range(len(self.history) - 1, -1, -1):
            step = self.history[index]
            if step.get("step") == step_num:
                step.update(fields)
                break

        if self._should_compress():
            self.compress_memory()

    def _extract_key_facts(self, step: Dict[str, Any]) -> None:
        """从步骤中提取关键命令、输出与分析信息。

        Args:
            step: 步骤数据字典。
        """
        if "tool_results" in step and step["tool_results"]:
            pairs = []
            for index, tr in enumerate(step["tool_results"], 1):
                name = tr.get("tool_name", "未知工具")
                args = tr.get("arguments", {})
                output = str(tr.get("output", ""))
                short_out = output
                pairs.append(f"{index}. {name}({args}) → {short_out}")
            self.key_facts["tool_results"] = "\n".join(pairs)
        elif "tool_calls" in step and step["tool_calls"]:
            tool_call_summary = []
            for index, tool_call in enumerate(step["tool_calls"], 1):
                tool_name = tool_call.get("tool_name", "未知工具")
                args = tool_call.get("arguments", {})
                tool_call_summary.append(f"{index}. {tool_name}({args})")
            self.key_facts["tool_calls"] = (
                f"工具调用: {', '.join(tool_call_summary)}"
            )

        if "analysis" in step and "analysis" in step["analysis"]:
            analysis = step["analysis"]["analysis"]
            if "关键发现" in analysis:
                self.key_facts[f"finding:{hash(analysis)}"] = analysis

    def compress_memory(self) -> None:
        """调用 LLM 压缩历史并生成结构化记忆块。"""
        logger.info("上下文占用达到 %.0f%%，开始压缩记忆...", self.compression_ratio * 100)
        if not self.history:
            return

        prompt = (
            "你是一个专业的CTF解题助手，需要压缩解题历史记录。请执行以下任务：\n"
            "1. 识别并提取关键的技术细节和发现\n"
            "2. 标记已尝试但失败的解决方案\n"
            "3. 总结当前解题状态和下一步建议\n"
            "4. 以JSON格式返回以下结构的数据：\n"
            "{\n"
            '  "key_findings": ["发现1", "发现2"],\n'
            '  "failed_attempts": ["命令1", "命令2"],\n'
            '  "current_status": "当前状态描述",\n'
            '  "next_steps": ["建议1", "建议2"]\n'
            "}\n\n"
            "历史记录:\n"
        )

        prompt += "关键事实摘要:\n"
        for _, value in list(self.key_facts.items())[-5:]:
            prompt += f"- {value}\n"

        for index, step in enumerate(
            self.history[-len(self.history):]
        ):
            prompt += f"\n步骤 {index + 1}:\n"
            prompt += f"- 目的: {step.get('think', '未指定')}\n"
            if "tool_results" in step and step["tool_results"]:
                for i, tr in enumerate(step["tool_results"], 1):
                    name = tr.get("tool_name", "未知工具")
                    args = tr.get("arguments", {})
                    output = str(tr.get("output", ""))
                    prompt += f"- 工具{i}: {name}({args}) → {output}\n"
            elif step.get("tool_calls"):
                tool_call_list = []
                for tool_call in step.get("tool_calls", []):
                    name = tool_call.get("tool_name", "未知工具")
                    args = tool_call.get("arguments", {})
                    tool_call_list.append(f"{name}({args})")
                prompt += f"- 命令: {', '.join(tool_call_list)}\n"

            if "analysis" in step:
                analysis = step["analysis"].get("analysis", "无分析")
                prompt += f"- 分析: {analysis}\n"

        response_content = ""
        try:
            response = self.solve_llm.text_completion(
                prompt=optimize_text(prompt),
                json_check=True,
                max_tokens=1024,
            )

            raw_content = response.choices[0].message.content
            response_content = raw_content if isinstance(raw_content, str) else str(raw_content)
            compressed_data_raw = json_repair.loads(response_content)
            compressed_data: Dict[str, Any] = (
                compressed_data_raw if isinstance(compressed_data_raw, dict) else {}
            )

            failed_attempts = compressed_data.get("failed_attempts", [])
            if isinstance(failed_attempts, list):
                for attempt in failed_attempts:
                    attempt_key = str(attempt)
                    self.failed_attempts[attempt_key] = (
                        self.failed_attempts.get(attempt_key, 0) + 1
                    )

            compressed_data["source_steps"] = len(self.history)
            self.compressed_memory.append(compressed_data)

            key_findings = compressed_data.get("key_findings", [])
            finding_count = len(key_findings) if isinstance(key_findings, list) else 0
            logger.info("记忆压缩成功: 添加了%d个关键发现", finding_count)

        except (json.JSONDecodeError, KeyError, TypeError):
            fallback = response_content.strip() if response_content else "压缩失败"
            self.compressed_memory.append(
                {
                    "fallback_summary": fallback,
                    "source_steps": len(self.history),
                }
            )
        except Exception as error:
            logger.error("记忆压缩失败: %s", error)
            self.compressed_memory.append(
                {
                    "error": f"压缩失败: {str(error)}",
                    "source_steps": len(self.history),
                }
            )

        # 压缩后清除详细历史，仅靠 compressed_memory + key_facts 提供摘要
        self.history = []

        # 压缩后再次检查是否需要继续压缩
        if self._should_compress():
            keep_last = min(2, len(self.compressed_memory))
            self.compressed_memory = self.compressed_memory[-keep_last:]

    def to_dict(self) -> Dict[str, Any]:
        """导出当前记忆状态为字典。

        Returns:
            包含历史、压缩记忆、关键事实等字段的字典。
        """
        return {
            "history": self.history,
            "compressed_memory": self.compressed_memory,
            "key_facts": self.key_facts,
            "failed_attempts": self.failed_attempts,
            "context_window": self.context_window,
            "compression_ratio": self.compression_ratio,
        }

    def restore_from_dict(self, data: Dict[str, Any]) -> None:
        """从字典恢复记忆状态。

        Args:
            data: 由 to_dict 导出的记忆状态字典。
        """
        self.history = data.get("history", [])
        self.compressed_memory = data.get("compressed_memory", [])
        self.key_facts = data.get("key_facts", {})
        self.failed_attempts = data.get("failed_attempts", {})
        self.context_window = data.get(
            "context_window", self.context_window
        )
        self.compression_ratio = data.get(
            "compression_ratio", self.compression_ratio
        )
        self._token_limit = int(self.context_window * self.compression_ratio)

    def get_summary(self, include_key_facts: bool = True) -> str:
        """生成综合记忆摘要文本。

        Args:
            include_key_facts: 是否在摘要中包含关键事实。

        Returns:
            综合记忆摘要字符串。
        """
        summary = ""

        if include_key_facts and self.key_facts:
            summary += "关键事实:\n"
            for _, value in list(self.key_facts.items())[-10:]:
                summary += f"- {value}\n"
            summary += "\n"

        if self.compressed_memory:
            summary += "压缩记忆块:\n"
            for index, mem in enumerate(self.compressed_memory[-3:]):
                summary += f"记忆块 #{len(self.compressed_memory) - index}:\n"

                if "key_findings" in mem:
                    summary += f"- 状态: {mem.get('current_status', '未知')}\n"
                    summary += f"- 关键发现: {', '.join(mem['key_findings'][:3])}"
                    if len(mem["key_findings"]) > 3:
                        summary += f" 等{len(mem['key_findings'])}项"
                    summary += "\n"

                if "failed_attempts" in mem:
                    failed_attempts = ", ".join(mem["failed_attempts"][:3])
                    summary += f"- 失败尝试: {failed_attempts}"
                    if len(mem["failed_attempts"]) > 3:
                        summary += f" 等{len(mem['failed_attempts'])}项"
                    summary += "\n"

                if "next_steps" in mem:
                    summary += f"- 建议步骤: {mem['next_steps'][0]}\n"

                summary += f"- 来源: 基于{mem['source_steps']}个历史步骤\n\n"

        if self.history:
            summary += "最近详细步骤:\n"
            for index, step in enumerate(self.history):
                step_num = len(self.history) - index
                summary += f"步骤 {step_num}:\n"
                summary += f"- 目的: {step.get('think', '未指定')}\n"

                if "tool_results" in step and step["tool_results"]:
                    for i, tr in enumerate(step["tool_results"], 1):
                        name = tr.get("tool_name", "未知工具")
                        args = tr.get("arguments", {})
                        output = str(tr.get("output", ""))
                        summary += f"- 工具{i}: {name}({args})\n"
                        summary += (
                            f"  输出: {output}\n"
                        )
                elif step.get("tool_calls"):
                    tool_call_list = []
                    for tool_call in step.get("tool_calls", []):
                        name = tool_call.get("tool_name", "未知工具")
                        args = tool_call.get("arguments", {})
                        tool_call_list.append(f"{name}({args})")
                    summary += f"- 命令: {', '.join(tool_call_list)}\n"

                if "analysis" in step:
                    analysis = step["analysis"].get("analysis", "无分析")
                    summary += f"- 分析: {analysis}\n"

                if (
                    "content" in step
                    and step["content"] in self.failed_attempts
                ):
                    summary += (
                        f"- 历史失败次数: "
                        f"{self.failed_attempts[step['content']]}\n"
                    )

                summary += "\n"

        return summary if summary else "无历史记录"
