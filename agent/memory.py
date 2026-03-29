"""
@brief 解题过程记忆管理模块。
"""

import json
import logging
from typing import Any, Dict, List

import json_repair

from config import Config
from utils.llm_request import LLMRequest
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class Memory:
    """
    @brief 管理解题历史、关键事实与压缩记忆。
    """

    def __init__(
        self,
        max_steps: int = 15,
        compression_threshold: int = 7,
    ) -> None:
        """
        @brief 初始化记忆对象。
        @param max_steps 最大保存步骤数。
        @param compression_threshold 触发压缩的步骤阈值。
        @return None。
        """
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.max_steps = max_steps
        self.compression_threshold = compression_threshold
        self.history: List[Dict[str, Any]] = []
        self.compressed_memory: List[Dict[str, Any]] = []
        self.key_facts: Dict[str, str] = {}
        self.failed_attempts: Dict[str, int] = {}

    def add_step(self, step: Dict[str, Any]) -> None:
        """
        @brief 添加执行步骤并更新关键事实与失败统计。
        @param step 单步执行数据。
        @return None。
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

        if len(self.history) >= self.compression_threshold:
            self.compress_memory()

    def add_planned_step(
        self,
        step_num: int,
        think: str,
        tool_calls: List[Dict[str, Any]],
    ) -> None:
        """
        @brief 记录尚未执行的计划步骤。
        @param step_num 步骤编号。
        @param think 当前步骤思考内容。
        @param tool_calls 当前步骤工具调用列表。
        @return None。
        """
        self.history.append(
            {
                "step": step_num,
                "think": think,
                "tool_calls": tool_calls,
                "status": "planned",
            }
        )

    def update_step(self, step_num: int, fields: Dict[str, Any]) -> None:
        """
        @brief 按步骤号更新历史记录中的字段。
        @param step_num 目标步骤号。
        @param fields 需要更新的字段集合。
        @return None。
        """
        for index in range(len(self.history) - 1, -1, -1):
            step = self.history[index]
            if step.get("step") == step_num:
                step.update(fields)
                break

    def _extract_key_facts(self, step: Dict[str, Any]) -> None:
        """
        @brief 从步骤中提取关键命令、输出与分析信息。
        @param step 单步执行数据。
        @return None。
        """
        if "tool_calls" in step and step["tool_calls"]:
            tool_call_summary = []
            for index, tool_call in enumerate(step["tool_calls"], 1):
                tool_name = tool_call.get("tool_name", "未知工具")
                args = tool_call.get("arguments", {})
                tool_call_summary.append(f"{index}. {tool_name}({args})")

            self.key_facts["tool_calls"] = (
                f"工具调用: {', '.join(tool_call_summary)}"
            )
        elif "tool_args" in step and step["tool_args"]:
            command = str(step["tool_args"])
            self.key_facts["command"] = f"命令: {command}"

        if "output" in step:
            output = step["output"]
            output_summary = output[:256] + (
                "..." if len(output) > 256 else ""
            )
            self.key_facts["output_summary"] = f"输出摘要: {output_summary}"

        if "analysis" in step and "analysis" in step["analysis"]:
            analysis = step["analysis"]["analysis"]
            if "关键发现" in analysis:
                self.key_facts[f"finding:{hash(analysis)}"] = analysis

    def compress_memory(self) -> None:
        """
        @brief 调用 LLM 压缩历史并生成结构化记忆块。
        @return None。
        """
        logger.info("优化记忆压缩中...")
        if not self.history:
            return

        prompt = """
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
                """

        prompt += "关键事实摘要:\n"
        for _, value in list(self.key_facts.items())[-5:]:
            prompt += f"- {value}\n"

        for index, step in enumerate(
            self.history[-self.compression_threshold :]
        ):
            prompt += f"\n步骤 {index + 1}:\n"
            prompt += f"- 目的: {step.get('think', '未指定')}\n"
            if step.get("tool_calls"):
                tool_call_list = []
                for tool_call in step.get("tool_calls", []):
                    name = tool_call.get("tool_name", "未知工具")
                    args = tool_call.get("arguments", {})
                    tool_call_list.append(f"{name}({args})")
                prompt += f"- 命令: {', '.join(tool_call_list)}\n"
            else:
                prompt += f"- 命令: {step.get('tool_args', {})}\n"

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
            print(f"记忆压缩成功: 添加了{finding_count}个关键发现")

        except (json.JSONDecodeError, KeyError, TypeError):
            fallback = response_content.strip() if response_content else "压缩失败"
            self.compressed_memory.append(
                {
                    "fallback_summary": fallback,
                    "source_steps": len(self.history),
                }
            )
        except Exception as error:
            print(f"记忆压缩失败: {str(error)}")
            self.compressed_memory.append(
                {
                    "error": f"压缩失败: {str(error)}",
                    "source_steps": len(self.history),
                }
            )

        keep_last = min(4, len(self.history))
        self.history = self.history[-keep_last:]

    def to_dict(self) -> Dict[str, Any]:
        """
        @brief 导出当前记忆状态为字典。
        @return 序列化后的记忆状态。
        """
        return {
            "history": self.history,
            "compressed_memory": self.compressed_memory,
            "key_facts": self.key_facts,
            "failed_attempts": self.failed_attempts,
            "compression_threshold": self.compression_threshold,
        }

    def restore_from_dict(self, data: Dict[str, Any]) -> None:
        """
        @brief 从字典恢复记忆状态。
        @param data 序列化后的记忆状态。
        @return None。
        """
        self.history = data.get("history", [])
        self.compressed_memory = data.get("compressed_memory", [])
        self.key_facts = data.get("key_facts", {})
        self.failed_attempts = data.get("failed_attempts", {})
        self.compression_threshold = data.get(
            "compression_threshold",
            self.compression_threshold,
        )

    def get_summary(self, include_key_facts: bool = True) -> str:
        """
        @brief 生成综合记忆摘要文本。
        @param include_key_facts 是否包含关键事实部分。
        @return 面向后续推理的摘要字符串。
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
                if step.get("tool_calls"):
                    tool_call_list = []
                    for tool_call in step.get("tool_calls", []):
                        name = tool_call.get("tool_name", "未知工具")
                        args = tool_call.get("arguments", {})
                        tool_call_list.append(f"{name}({args})")
                    summary += f"- 命令: {', '.join(tool_call_list)}\n"
                else:
                    summary += f"- 命令: {step.get('tool_args', {})}\n"

                if "output" in step:
                    output = step["output"]
                    summary += (
                        f"- 输出: {output[:512]}"
                        f"{'...' if len(output) > 512 else ''}\n"
                    )

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
