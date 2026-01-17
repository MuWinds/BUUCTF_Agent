import time
import yaml
import logging
import json_repair
from config import Config
from rag.knowledge_base import KnowledgeBase
from ctf_tool.base_tool import BaseTool
from agent.analyzer import Analyzer
from typing import Dict, Tuple, List
from agent.memory import Memory
from utils.llm_request import LLMRequest
from jinja2 import Environment, FileSystemLoader
from utils.tools import ToolUtils
from utils.user_interface import UserInterface, CommandLineInterface

logger = logging.getLogger(__name__)


class SolveAgent:
    def __init__(self, problem: str, user_interface: UserInterface = None): 
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.problem = problem
        self.user_interface = user_interface or CommandLineInterface()
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        self.knowledge_base = KnowledgeBase()  # 在此处初始化知识库
        if self.config is None:
            raise ValueError("找不到配置文件")

        # 初始化Jinja2模板环境
        self.env = Environment(loader=FileSystemLoader("."))

        # 初始化记忆系统
        self.memory = Memory(
            max_steps=self.config.get("max_history_steps", 10),
            compression_threshold=self.config.get("compression_threshold", 5),
        )
        # 动态加载工具和分类信息
        self.tools: Dict[str, BaseTool] = {}  # 工具名称 -> 工具实例
        self.function_configs: List[Dict] = []  # 函数调用配置列表
        self.tool_classification: Dict = {}  # 工具分类信息

        # 动态加载工具
        self.analyzer = Analyzer(config=self.config, problem=self.problem)

        # 加载ctf_tools文件夹中的所有工具
        self.tool = ToolUtils()
        self.tools, self.function_configs = self.tool.load_tools()

        # 添加模式设置
        self.auto_mode = self.user_interface.select_mode()

        # 添加flag确认回调函数
        self.confirm_flag_callback = None  # 将由Workflow设置

    def _select_mode(self):
        """让用户选择运行模式"""
        print("\n请选择运行模式:")
        print("1. 自动模式（Agent自动生成和执行所有命令）")
        print("2. 手动模式（每一步需要用户批准）")

        while True:
            choice = input("请输入选项编号: ").strip()
            if choice == "1":
                self.auto_mode = True
                logger.info("已选择自动模式")
                return
            elif choice == "2":
                self.auto_mode = False
                logger.info("已选择手动模式")
                return
            else:
                print("无效选项，请重新选择")

    def solve(self) -> str:
        """
        主解题函数 - 采用逐步执行方式
        :return: 获取的flag
        """
        step_count = 0

        while True:
            step_count += 1
            self.user_interface.display_message(f"\n正在思考第 {step_count} 步...")

            # 生成下一步执行命令
            next_step = None
            while next_step is None:
                next_step = self.next_instruction()
                if next_step:
                    think, tool_calls = next_step
                    break
                self.user_interface.display_message("生成执行内容失败，10秒后重试...")
                time.sleep(10)

            if not self.auto_mode:
                approved, next_step = self.manual_approval_step(next_step)
                think, tool_calls = next_step
                if not approved:
                    self.user_interface.display_message("用户终止解题")
                    return "解题终止"

            self.memory.add_planned_step(step_count, think, tool_calls)

            # 执行所有工具调用
            all_tool_results = []
            combined_raw_output = ""
            
            for i, tool_call in enumerate(tool_calls):
                self.user_interface.display_message(f"\n执行工具 {i+1}/{len(tool_calls)}: {tool_call.get('tool_name')}")
                
                tool_name = tool_call.get("tool_name")
                arguments: dict = tool_call.get("arguments", {})
                
                if tool_name in self.tools:
                    try:
                        tool = self.tools[tool_name]
                        result = tool.execute(tool_name, arguments)
                        if not result:
                            result = "注意！无输出内容！"
                        
                        # 保存每个工具的结果
                        tool_result = {
                            "tool_name": tool_name,
                            "arguments": arguments,
                            "raw_output": result
                        }
                        all_tool_results.append(tool_result)
                        
                        # 构建原始输出字符串
                        combined_raw_output += str(result) + "\n---\n"
                        
                    except Exception as e:
                        error_msg = f"工具执行出错: {str(e)}"
                        tool_result = {
                            "tool_name": tool_name,
                            "arguments": arguments,
                            "raw_output": error_msg
                        }
                        all_tool_results.append(tool_result)
                        combined_raw_output += error_msg + "\n---\n"
                else:
                    error_msg = f"错误: 未找到工具 '{tool_name}'"
                    tool_result = {
                        "tool_name": tool_name,
                        "arguments": arguments,
                        "raw_output": error_msg
                    }
                    all_tool_results.append(tool_result)
                    combined_raw_output += error_msg + "\n---\n"
                
                logger.info(f"工具 {tool_name} 原始输出:\n{all_tool_results[-1]['raw_output']}")

            # 使用通用output_summary函数
            output_summary = ToolUtils.output_summary(
                tool_results=all_tool_results,
                think=think,
                tool_output=combined_raw_output
            )

            logger.info(f"工具输出摘要（共{len(all_tool_results)}个工具）:\n{output_summary}")

            # 使用LLM分析所有工具的输出
            analysis_result: Dict = self.analyzer.analyze_step_output(
                self.memory, step_count, output_summary, think
            )

            # 检查LLM是否在输出中发现了flag
            if analysis_result.get("flag_found", False):
                flag_candidate = analysis_result.get("flag", "")
                logger.info(f"LLM报告发现flag: {flag_candidate}")

                # 使用回调函数确认flag
                if self.confirm_flag_callback and self.confirm_flag_callback(
                    flag_candidate
                ):
                    return flag_candidate
                else:
                    logger.info("用户确认flag不正确，继续解题")

            self.memory.update_step(step_count, {
                "tool_args": tool_calls[0].get("arguments", {}) if tool_calls else {},
                "output": output_summary,
                "raw_outputs": combined_raw_output,
                "analysis": analysis_result,
                "status": "executed"
            })

            # 检查是否应该提前终止
            if analysis_result.get("terminate", False):
                self.user_interface.display_message("LLM建议提前终止解题")
                return "未找到flag：提前终止"

    def manual_approval_step(
        self, next_step: Tuple[str, List[Dict]]
    ) -> Tuple[bool, Tuple[str, List[Dict]]]:
        """手动模式：让用户无限次反馈/重思，直到 ta 主动选 1 或 3"""
        while True:
            think, tool_calls = next_step
            
            # 使用用户接口获取批准结果
            result = self.user_interface.manual_approval_step(think, tool_calls)
            approved, data = result
            
            if approved:
                # 用户批准执行
                return True, data
            elif data is None:
                # 用户选择终止
                return False, None
            else:
                # 用户提供了反馈，需要重新思考
                if len(data) == 3:
                    # 包含反馈的情况
                    _, _, feedback = data
                    next_step = self.reflection(think, feedback)
                    if not next_step:
                        self.user_interface.display_message("（思考失败，可继续反馈或选 3 终止）")
                else:
                    # 其他情况
                    next_step = data

    def next_instruction(self) -> Tuple[str, List[Dict]]:
        """
        生成下一步执行命令 - 返回思考和多个工具调用
        :return: (思考内容, 工具调用列表)
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary()

        # 获取相关知识库内容
        relevant_knowledge = self.knowledge_base.get_relevant_knowledge(self.problem)

        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("think_next", ""))

        # 渲染提示
        think_prompt = template.render(
            question=self.problem,
            history_summary=history_summary,
            relevant_knowledge=relevant_knowledge,
            tools=self.function_configs,
        )

        # 调用LLM，要求返回思考和工具调用列表
        response = self.solve_llm.text_completion(
            prompt=think_prompt,
            json_check=True,  # 要求返回JSON格式
        )

        # 解析LLM返回的思考内容和工具调用列表
        result = json_repair.loads(response.choices[0].message.content)

        think_content = result.get("think", "未返回思考内容")
        logger.info(f"思考内容: {think_content}")

        # 解析工具调用列表
        tool_calls = ToolUtils.parse_tool_response(response)
        return think_content, tool_calls

    def reflection(self, think: str, feedback: str) -> Tuple[str, List[Dict]]:
        """
        根据用户反馈重新生成思考内容和工具调用
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary()

        # 获取相关知识库内容
        relevant_knowledge = self.knowledge_base.get_relevant_knowledge(self.problem)

        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("reflection", ""))

        # 渲染提示
        reflection_prompt = template.render(
            question=self.problem,
            history_summary=history_summary,
            relevant_knowledge=relevant_knowledge,
            original_purpose=think,
            feedback=feedback,
            tools=self.function_configs,
        )

        # 调用LLM，返回思考和工具调用列表
        response = self.solve_llm.text_completion(
            prompt=reflection_prompt,
            json_check=True,
        )

        # 解析LLM返回的思考内容和工具调用列表
        try:
            result = json_repair.loads(response.choices[0].message.content)
            think_content = result.get("think", "未返回思考内容")
            # 使用统一解析逻辑规范化工具调用结构
            tool_calls = ToolUtils.parse_tool_response(response)
            if not tool_calls:
                logger.warning("LLM没有选择任何工具")
                tool_calls = [{"tool_name": "无可用工具", "arguments": {}}]

            logger.info(f"重新思考内容: {think_content}")
            for i, tool_call in enumerate(tool_calls):
                logger.info(
                    f"重新选择的工具 {i+1}: {tool_call.get('name', tool_call.get('tool_name'))}"
                )

            return think_content, tool_calls

        except Exception as e:
            logger.error(f"解析LLM返回的JSON失败: {e}")
            # 尝试从文本中提取思考和工具信息
            content = response.choices[0].message.content
            think_content = "解析错误，使用默认思考"

            # 尝试从文本中提取工具调用
            tool_arg = ToolUtils.parse_tool_response(content)
            tool_calls = [tool_arg] if tool_arg else []

            return think_content, tool_calls
