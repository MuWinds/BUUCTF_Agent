import time
import yaml
import logging
from config import Config
from ctf_tool.base_tool import BaseTool
from agent.analyzer import Analyzer
from typing import Dict, Tuple, Optional, List
from agent.memory import Memory
from utils.llm_request import LLMRequest
from jinja2 import Environment, FileSystemLoader
from utils.tools import ToolUtils

logger = logging.getLogger(__name__)


class SolveAgent:
    def __init__(self, problem: str):
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.problem = problem
        self.prompt: dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None:
            raise ValueError("找不到配置文件")

        # 初始化Jinja2模板环境
        self.env = Environment(loader=FileSystemLoader("."))

        # 初始化记忆系统
        self.memory = Memory(
            config=self.config,
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
        self.tools, self.function_configs, _ = self.tool.load_tools()

        # 添加模式设置
        self._select_mode()

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
        :param problem_class: 题目类别（Web/Crypto/Reverse等）
        :param solution_plan: 解题思路
        :return: 获取的flag
        """
        step_count = 0

        while True:
            step_count += 1
            print(f"\n正在思考第 {step_count} 步...")

            # 生成下一步执行命令
            next_step = None
            while next_step is None:
                next_step = self.next_instruction()
                if next_step:
                    think, tool_arg = next_step
                    break
                print("生成执行内容失败，10秒后重试...")
                time.sleep(10)

            # 提取工具名称和参数
            tool_name = tool_arg.get("tool_name")
            arguments: dict = tool_arg.get("arguments", {})
            output = ""
            # 手动模式：需要用户批准命令
            if not self.auto_mode:
                approved, next_step = self.manual_approval_step(next_step)
                think, tool_arg = next_step
                if not approved:
                    print("用户终止解题")
                    return "解题终止"
                # 更新参数
                tool_name = tool_arg.get("tool_name")
                arguments = tool_arg.get("arguments", {})

            # 获取工具类别
            tool_category = self.tool.get_tool_category(tool_name)

            if tool_name in self.tools:
                try:
                    tool = self.tools[tool_name]
                    # 统一调用方式
                    result = tool.execute(tool_name, arguments)
                    output = (
                        ToolUtils.output_summary(tool_name, tool_arg, think, result)
                        if result
                        else "无输出内容"
                    )  # 获取输出内容
                except Exception as e:
                    output = f"工具执行出错: {str(e)}"
            else:
                output = f"错误: 未找到工具 '{tool_name}'"

            logger.info(f"命令输出:\n{output}")

            # 使用LLM分析输出
            analysis_result: Dict = self.analyzer.analyze_step_output(
                self.memory, step_count, output, think
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

            # 添加执行历史到记忆系统（包含工具名称和类别）
            self.memory.add_step(
                {
                    "step": step_count,
                    "think": think,
                    "tool_name": tool_name,  # 新增：工具名称
                    "tool_category": tool_category,  # 新增：工具类别
                    "tool_args": arguments,
                    "output": output,
                    "analysis": analysis_result,
                }
            )

            # 检查是否应该提前终止
            if analysis_result.get("terminate", False):
                print("LLM建议提前终止解题")
                return "未找到flag：提前终止"

    def manual_approval_step(self, next_step: Tuple[str, Dict]) -> Tuple[bool, Tuple[str, Dict]]:
        """手动模式：让用户无限次反馈/重思，直到 ta 主动选 1 或 3"""
        while True:
            think, _ = next_step

            print("1. 批准并执行")
            print("2. 提供反馈并重新思考")
            print("3. 终止解题")
            choice = input("请输入选项编号: ").strip()

            if choice == "1":
                return True, next_step
            elif choice == "2":
                feedback = input("请提供改进建议: ").strip()
                next_step = self.reflection(think, feedback)
                if not next_step:
                    print("（思考失败，可继续反馈或选 3 终止）")
            elif choice == "3":
                return False, None
            else:
                print("无效选项，请重新选择")

    def reflection(self, think: str, feedback: str) -> Tuple[str, Dict]:
        """
        根据用户反馈重新生成命令，返回的是“LLM 重新思考后的 next_step”，后续仍需让用户再次确认。
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary(self.problem)
        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("think_next", ""))
        think_prompt = template.render(
            question=self.problem,
            history_summary=history_summary,
            original_purpose=think,
            feedback=feedback,
        )
        # 调用LLM思考下一步
        response = self.solve_llm.text_completion(prompt=think_prompt, json_check=False)
        think_content = response.choices[0].message.content
        logger.info(f"执行目的: {think_content}")

        # 对思考内容进行分类
        category = self.tool.classify_think(
            self.problem, think_content, history_summary
        )
        category_tools = self.tool.get_tools_by_category(category)

        think_content, tool_arg = self.tool_general(
            history_summary, think_content, category_tools
        )
        return think_content, tool_arg

    def next_instruction(self) -> Tuple[str, Dict]:
        """
        生成下一步执行命令
        :return: 下一步命令字典和工具类别
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary(self.problem)
        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("think_next", ""))
        think_prompt = template.render(
            question=self.problem, history_summary=history_summary
        )
        # 调用LLM思考下一步
        response = self.solve_llm.text_completion(prompt=think_prompt, json_check=False)
        think_content = response.choices[0].message.content
        logger.info(f"执行目的: {think_content}")

        # 对思考内容进行分类
        category = self.tool.classify_think(
            self.problem, think_content, history_summary
        )
        category_tools = self.tool.get_tools_by_category(category)

        think_content, tool_arg = self.tool_general(
            history_summary, think_content, category_tools
        )
        return think_content, tool_arg

    def tool_general(
        self, history_summary: str, think: str, tool_configs: List[Dict] = None
    ) -> Tuple[str, Dict]:
        """
        生成工具调用
        :param think: 思考内容
        :param tool_configs: 可用的工具配置列表，如果为None则使用所有工具
        :return: 思考内容和工具调用参数
        """
        if tool_configs is None:
            tool_configs = self.function_configs
        # 调用LLM生成下一步动作
        template = self.env.from_string(self.prompt.get("general_next", ""))
        step_prompt = template.render(
            question=self.problem,
            solution_plan=think,
            history_summary=history_summary,
        )
        response = self.solve_llm.text_completion(
            prompt=step_prompt,
            json_check=True,
            tools=tool_configs,
            tool_choice="auto",
        )
        return think, ToolUtils.parse_tool_response(response)
