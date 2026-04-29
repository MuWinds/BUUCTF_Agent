"""
@brief 解题主代理模块。
"""

import logging
import re
import time
from typing import Any, Callable, Dict, List, Optional, Tuple, cast

import yaml
from jinja2 import Environment, FileSystemLoader

from agent.analyzer import Analyzer
from agent.checkpoint import CheckpointManager
from agent.memory import Memory
from config import Config
from ctf_tool.base_tool import BaseTool
from skill.manager import SkillManager
from utils.llm_request import LLMRequest
from utils.tools import ToolUtils
from utils.user_interface import ApprovedStep, UserInterface

logger = logging.getLogger(__name__)


class SolveAgent:
    """
    @brief 负责逐步生成、执行并分析解题动作。
    """

    def __init__(
        self,
        problem: str,
        user_interface: UserInterface,
    ) -> None:
        """
        @brief 初始化解题代理。
        @param problem 当前题目文本。
        @param user_interface 用户交互接口实现。
        @raises ValueError 当配置文件缺失时抛出。
        """
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.problem = problem
        self.user_interface = user_interface
        with open("./prompt.yaml", "r", encoding="utf-8") as file:
            self.prompt: Dict[str, Any] = yaml.safe_load(file)

        if self.config is None:
            raise ValueError("找不到配置文件")

        self.env = Environment(loader=FileSystemLoader("."))

        self.memory = Memory(
            context_window=self.config.get("context_window", 128000),
            compression_ratio=self.config.get("compression_ratio", 0.8),
        )

        self.tools: Dict[str, BaseTool] = {}
        self.function_configs: List[Dict[str, Any]] = []
        self.tool_classification: Dict[str, Any] = {}

        self.analyzer = Analyzer(config=self.config, problem=self.problem)

        self.tool = ToolUtils()
        self.tools, self.function_configs = self.tool.load_tools()

        skill_paths = self.config.get("skills", {}).get("paths", [])
        self.skill_manager = SkillManager(extra_paths=skill_paths)

        self.auto_mode = self.user_interface.select_mode()

        self.confirm_flag_callback: Optional[Callable[[str], bool]] = None

        self.checkpoint_manager = CheckpointManager(
            checkpoint_dir=self.config.get("checkpoint_dir", "./checkpoints")
        )

    def solve(self, resume_step: int = 0) -> str:
        """
        @brief 主解题循环，逐步调用工具并分析输出。
        @param resume_step 恢复时的起始步骤编号。
        @return 最终 flag 或终止原因说明。
        """
        step_count = resume_step

        while True:
            step_count += 1
            self.user_interface.display_message(f"\n正在思考第 {step_count} 步...")

            if not self.function_configs:
                self.user_interface.display_message("当前没有可用工具，无法继续解题")
                return "未找到flag：无可用工具"

            next_step = None
            while next_step is None:
                next_step = self.next_instruction()
                if next_step:
                    think, tool_calls = next_step
                    break
                self.user_interface.display_message("生成执行内容失败，10秒后重试...")
                time.sleep(10)

            if next_step is None:
                self.user_interface.display_message("生成执行内容失败")
                return "解题终止"

            think, tool_calls = next_step
            if not self.auto_mode:
                approved, approved_step = self.manual_approval_step(next_step)
                if not approved or approved_step is None:
                    self.user_interface.display_message("用户终止解题")
                    return "解题终止"
                think, tool_calls = approved_step

            self.memory.add_planned_step(step_count, think, tool_calls)
            all_tool_results, combined_raw_output = ToolUtils.execute_tools(
                tools=self.tools,
                tool_calls=tool_calls,
                display_message=self.user_interface.display_message,
            )

            output_summary = ToolUtils.output_summary(
                tool_results=all_tool_results,
                think=think,
                tool_output=combined_raw_output,
            )

            logger.info(
                "工具输出摘要（共%s个工具）:\n%s",
                len(all_tool_results),
                output_summary,
            )

            analysis_result: Dict[str, Any] = (
                self.analyzer.analyze_step_output(
                    self.memory,
                    str(step_count),
                    output_summary,
                    think,
                )
            )

            if analysis_result.get("flag_found", False):
                flag_value = analysis_result.get("flag")
                flag_candidate = flag_value if isinstance(flag_value, str) else ""
                logger.info("LLM报告发现flag: %s", flag_candidate)

                if self.confirm_flag_callback and self.confirm_flag_callback(
                    flag_candidate
                ):
                    self.checkpoint_manager.delete(self.problem)
                    return flag_candidate
                logger.info("用户确认flag不正确，继续解题")

            self.memory.update_step(
                step_count,
                {
                    "tool_args": (
                        tool_calls[0].get("arguments", {})
                        if tool_calls
                        else {}
                    ),
                    "output": output_summary,
                    "raw_outputs": combined_raw_output,
                    "analysis": analysis_result,
                    "status": "executed",
                },
            )

            self.checkpoint_manager.save(
                problem=self.problem,
                step_count=step_count,
                auto_mode=self.auto_mode,
                memory_data=self.memory.to_dict(),
            )

            if analysis_result.get("terminate", False):
                self.user_interface.display_message("LLM建议提前终止解题")
                self.checkpoint_manager.delete(self.problem)
                return "未找到flag：提前终止"

    def restore_from_checkpoint(self, data: Dict[str, Any]) -> int:
        """
        @brief 从存档恢复代理状态。
        @param data 存档管理器读取到的存档数据。
        @return 恢复后的 step_count。
        """
        memory_data = data.get("memory")
        if isinstance(memory_data, dict):
            self.memory.restore_from_dict(memory_data)

        auto_mode_value = data.get("auto_mode")
        if isinstance(auto_mode_value, bool):
            self.auto_mode = auto_mode_value

        step_count_value = data.get("step_count")
        return step_count_value if isinstance(step_count_value, int) else 0

    def manual_approval_step(
        self,
        next_step: Tuple[str, List[Dict[str, Any]]],
    ) -> Tuple[bool, Optional[ApprovedStep]]:
        """
        @brief 手动模式下处理用户批准、反馈与终止。
        @param next_step 候选步骤，包含思考和工具调用。
        @return (是否批准, 最终步骤数据)。
        """
        while True:
            think, tool_calls = next_step

            approved, data = self.user_interface.manual_approval_step(
                think,
                tool_calls,
            )

            if approved:
                if data is not None and len(data) == 2:
                    return True, cast(ApprovedStep, data)
                return False, None

            if data is None:
                return False, None

            if len(data) == 3:
                _, _, feedback = cast(Tuple[str, List[Dict[str, Any]], str], data)
                reflected_step = self.reflection(think, feedback)
                if reflected_step:
                    next_step = reflected_step
                else:
                    self.user_interface.display_message(
                        "（思考失败，可继续反馈或选 3 终止）"
                    )
                    next_step = (think, tool_calls)
            else:
                next_step = cast(Tuple[str, List[Dict[str, Any]]], data)

    @staticmethod
    def _extract_think(message_content: object) -> str:
        """
        @brief 从模型消息中提取思考文本（去除 JSON 代码块）。

        @param message_content 模型返回的 message.content。
        @return 清洗后的思考文本。
        """
        if message_content is None:
            return "未返回思考内容（仅返回工具调用）"

        if isinstance(message_content, str):
            content_str = message_content
        else:
            content_str = str(message_content)

        # 移除 XML 工具调用块和 Markdown 代码块，保留思考内容
        cleaned = re.sub(r"<tool_calls>.*?</tool_calls>", "", content_str, flags=re.DOTALL)
        cleaned = re.sub(r"```json\s*\n.*?\n```", "", cleaned, flags=re.DOTALL)
        cleaned = re.sub(r"```xml\s*\n.*?\n```", "", cleaned, flags=re.DOTALL)
        cleaned = re.sub(r"```\s*\n.*?\n```", "", cleaned, flags=re.DOTALL)
        think = cleaned.strip()

        return think or "未返回思考内容（仅返回工具调用）"

    def _request_tool_plan(
        self,
        prompt: str,
    ) -> Optional[Tuple[str, List[Dict[str, Any]]]]:
        """
        @brief 请求模型返回思考内容与工具调用计划（基于提示词而非原生 tool call）。

        @param prompt 给模型的提示词。
        @return (思考内容, 工具调用列表)；失败返回 None。
        """
        try:
            response = self.solve_llm.text_completion(
                prompt=prompt,
                json_check=False,
            )
        except Exception as error:
            logger.error("调用LLM失败: %s", error)
            return None

        think_content = self._extract_think(
            response.choices[0].message.content
        )
        tool_calls = ToolUtils.parse_tool_response(response)
        if tool_calls:
            return think_content, tool_calls

        logger.warning("LLM未返回有效tool_calls，重试一次")
        try:
            retry_response = self.solve_llm.text_completion(
                prompt=prompt,
                json_check=False,
            )
        except Exception as error:
            logger.error("重试LLM调用失败: %s", error)
            return None

        retry_think = self._extract_think(
            retry_response.choices[0].message.content
        )
        retry_tool_calls = ToolUtils.parse_tool_response(retry_response)
        if retry_tool_calls:
            return retry_think, retry_tool_calls

        logger.error("连续两次未返回有效tool_calls")
        return None

    def next_instruction(self) -> Optional[Tuple[str, List[Dict[str, Any]]]]:
        """
        @brief 生成下一步执行计划。
        @return (思考内容, 工具调用列表)；失败返回 None。
        """
        history_summary = self.memory.get_summary()
        tools_text = ToolUtils.format_tools_for_prompt(self.function_configs)
        skills_text = self.skill_manager.format_for_prompt()

        template = self.env.from_string(self.prompt.get("think_next", ""))
        think_prompt = template.render(
            question=self.problem,
            history_summary=history_summary,
            tools_text=tools_text,
            skills_text=skills_text,
        )

        result = self._request_tool_plan(think_prompt)
        if result is None:
            return None

        think_content, tool_calls = result
        logger.info("思考内容: %s", think_content)
        return think_content, tool_calls

    def reflection(
        self,
        think: str,
        feedback: str,
    ) -> Optional[Tuple[str, List[Dict[str, Any]]]]:
        """
        @brief 根据用户反馈重新生成步骤计划。
        @param think 原始思考目的。
        @param feedback 用户反馈。
        @return (新思考, 新工具调用列表)；失败返回 None。
        """
        history_summary = self.memory.get_summary()
        tools_text = ToolUtils.format_tools_for_prompt(self.function_configs)
        skills_text = self.skill_manager.format_for_prompt()

        template = self.env.from_string(self.prompt.get("reflection", ""))
        reflection_prompt = template.render(
            question=self.problem,
            history_summary=history_summary,
            original_purpose=think,
            feedback=feedback,
            tools_text=tools_text,
            skills_text=skills_text,
        )

        result = self._request_tool_plan(reflection_prompt)
        if result is None:
            logger.warning("反思阶段未能生成有效工具调用")
            return None

        think_content, tool_calls = result
        logger.info("重新思考内容: %s", think_content)
        for index, tool_call in enumerate(tool_calls):
            logger.info(
                "重新选择的工具 %s: %s",
                index + 1,
                tool_call.get("tool_name"),
            )

        return think_content, tool_calls
