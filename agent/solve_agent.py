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

logger = logging.getLogger(__name__)


class SolveAgent:
    def __init__(self, problem: str):
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.problem = problem
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

            if tool_name in self.tools:
                try:
                    tool = self.tools[tool_name]
                    # 统一调用方式
                    result = tool.execute(tool_name, arguments)
                    output = (
                        ToolUtils.output_summary(tool_name, tool_arg, think, result)
                        if result
                        else "注意！无输出内容！"
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
                    "tool_args": arguments,
                    "output": output,
                    "analysis": analysis_result,
                }
            )

            # 检查是否应该提前终止
            if analysis_result.get("terminate", False):
                print("LLM建议提前终止解题")
                return "未找到flag：提前终止"

    def manual_approval_step(
        self, next_step: Tuple[str, Dict]
    ) -> Tuple[bool, Tuple[str, Dict]]:
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
    
    def next_instruction(self) -> Tuple[str, Dict]:
        """
        生成下一步执行命令 - 一次性返回思考和工具调用
        :return: (思考内容, 工具调用参数)
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary(self.problem)
        
        # 获取相关知识库内容
        relevant_knowledge = self.knowledge_base.get_relevant_knowledge(self.problem)
        
        # 使用Jinja2渲染提示，要求LLM一次性返回思考和工具调用
        template = self.env.from_string(self.prompt.get("think_next", ""))
        
        # 渲染提示
        think_prompt = template.render(
            question=self.problem,
            history_summary=history_summary,
            relevant_knowledge=relevant_knowledge,
            tools=self.function_configs
        )
        
        # 调用LLM，要求一次性返回思考和工具调用
        response = self.solve_llm.text_completion(
            prompt=think_prompt,
            json_check=True,  # 要求返回JSON格式
        )
        
        # 解析LLM返回的思考内容和工具调用
        result = json_repair.loads(response.choices[0].message.content)
        
        think_content = result.get("think", "未返回思考内容")
        logger.info(f"思考内容: {think_content}")
        tool_arg = ToolUtils.parse_tool_response(response)
        
        return think_content, tool_arg

    def reflection(self, think: str, feedback: str) -> Tuple[str, Dict]:
        """
        根据用户反馈重新生成思考内容和工具调用
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary(self.problem)
        
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
            tools=self.function_configs
        )
        
        # 调用LLM，一次性返回思考和工具调用
        response = self.solve_llm.text_completion(
            prompt=reflection_prompt,
            json_check=True,  # 要求返回JSON格式
        )
        
        # 解析LLM返回的思考内容和工具调用
        try:
            import json
            result = json.loads(response.choices[0].message.content)
            
            think_content = result.get("think", "未返回思考内容")
            tool_call = result.get("tool_call", {})
            
            if not tool_call or tool_call.get("tool_name") == "无可用工具":
                logger.warning("LLM没有选择任何工具")
                tool_call = {
                    "tool_name": "无可用工具",
                    "arguments": {}
                }
            
            logger.info(f"重新思考内容: {think_content}")
            logger.info(f"重新选择的工具调用: {tool_call}")
            
            return think_content, tool_call
            
        except json.JSONDecodeError as e:
            logger.error(f"解析LLM返回的JSON失败: {e}")
            # 尝试从文本中提取思考和工具信息
            content = response.choices[0].message.content
            think_content = "解析错误，使用默认思考"
            
            # 尝试从文本中提取工具调用
            tool_arg = ToolUtils.parse_tool_response(content)
            
            return think_content, tool_arg