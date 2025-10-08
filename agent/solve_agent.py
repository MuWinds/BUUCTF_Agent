import litellm
import json
import time
import yaml
import os
import logging
import inspect
import importlib
from ctf_tool.base_tool import BaseTool
from litellm import ModelResponse
from .analyzer import Analyzer
from typing import Dict, Tuple, Optional, List
from .memory import Memory
from .utils import optimize_text
from . import utils
from jinja2 import Environment, FileSystemLoader

logger = logging.getLogger(__name__)
litellm.enable_json_schema_validation = True


class SolveAgent:
    def __init__(self, config: dict, problem: str):
        self.config = config
        self.llm_config = self.config["llm"]["solve_agent"]
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

        # 动态加载工具
        self.tools: Dict[str, BaseTool] = {}  # 工具名称 -> 工具实例
        self.function_configs: List[Dict] = []  # 函数调用配置列表
        self.analyzer = Analyzer(config=self.config, problem=self.problem)

        # 加载ctf_tools文件夹中的所有工具
        self._load_tools()

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

    def _load_tools(self):
        """动态加载tool文件夹中的所有工具"""
        tools_dir = os.path.join(os.path.dirname(__file__), "..", "ctf_tool")

        for file_name in os.listdir(tools_dir):
            if (
                file_name.endswith(".py")
                and file_name != "__init__.py"
                and file_name != "base_tool.py"
            ):
                module_name = file_name[:-3]  # 移除.py
                try:
                    # 导入模块
                    module = importlib.import_module(f"ctf_tool.{module_name}")

                    # 查找所有继承自BaseTool的类
                    for name, obj in inspect.getmembers(module):
                        if (
                            inspect.isclass(obj)
                            and issubclass(obj, BaseTool)
                            and obj != BaseTool
                        ):
                            # 检查是否需要特殊配置
                            if name in self.config.get("tool_config", {}):
                                # 使用配置创建实例
                                tool_config = self.config["tool_config"][name]
                                tool_instance = obj(tool_config)
                            else:
                                # 创建默认实例
                                tool_instance = obj()

                            # 添加到工具字典
                            tool_name = tool_instance.function_config["function"][
                                "name"
                            ]
                            self.tools[tool_name] = tool_instance

                            # 添加工具配置
                            self.function_configs.append(tool_instance.function_config)

                            logger.info(f"已加载工具: {tool_name}")
                except Exception as e:
                    logger.warning(f"加载工具{module_name}失败: {str(e)}")

    def solve(self, problem_class: str, solution_plan: str) -> str:
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
                next_step = self.generate_next_step(problem_class, solution_plan)
                if next_step:
                    break
                print("生成执行内容失败，10秒后重试...")
                time.sleep(10)

            # 提取工具名称和参数
            tool_name = next_step.get("tool_name")
            arguments: dict = next_step.get("arguments", {})
            content = arguments.get("content", "")

            # 手动模式：需要用户批准命令
            if not self.auto_mode:
                approved, next_step = self.manual_approval_step(next_step)
                if not approved:
                    print("用户终止解题")
                    return "解题终止"
                # 更新参数
                tool_name = next_step.get("tool_name")
                arguments = next_step.get("arguments", {})
                content = arguments.get("content", "")

            # 执行命令
            output = ""
            if tool_name in self.tools:
                try:
                    tool = self.tools[tool_name]
                    result = tool.execute(arguments)
                    stdout, stderr = result
                    output = stdout + stderr
                except Exception as e:
                    output = f"工具执行出错: {str(e)}"
            else:
                output = f"错误: 未找到工具 '{tool_name}'"

            logger.info(f"命令输出:\n{output}")

            # 使用LLM分析输出
            analysis_result = self.analyzer.analyze_step_output(
                self.memory, step_count, content, output, solution_plan
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

            # 添加执行历史到记忆系统
            self.memory.add_step(
                {
                    "step": step_count,
                    "purpose": arguments.get("purpose", "未指定目的"),
                    "content": content,
                    "output": output,
                    "analysis": analysis_result,
                }
            )

            # 检查是否应该提前终止
            if analysis_result.get("terminate", False):
                print("LLM建议提前终止解题")
                return "未找到flag：提前终止"

    def manual_approval_step(self, next_step: Dict) -> Tuple[bool, Optional[Dict]]:
        """手动模式：让用户无限次反馈/重思，直到 ta 主动选 1 或 3"""
        while True:
            arguments: dict = next_step.get("arguments", {})
            purpose = arguments.get("purpose", "未指定目的")

            print("1. 批准并执行")
            print("2. 提供反馈并重新思考")
            print("3. 终止解题")
            choice = input("请输入选项编号: ").strip()

            if choice == "1":
                return True, next_step
            elif choice == "2":
                feedback = input("请提供改进建议: ").strip()
                next_step = self.reflection(purpose, feedback)
                if not next_step:
                    print("（思考失败，可继续反馈或选 3 终止）")
            elif choice == "3":
                return False, None
            else:
                print("无效选项，请重新选择")

    def reflection(self, purpose: str, feedback: str) -> Dict:
        """
        根据用户反馈重新生成命令，返回的是“LLM 重新思考后的 next_step”，后续仍需让用户再次确认。
        """
        history_summary = self.memory.get_summary()

        template = self.env.from_string(self.prompt.get("reflection", ""))
        prompt = template.render(
            question=self.problem,
            original_purpose=purpose,
            feedback=feedback,
            history_summary=history_summary,
            tools=self.tools.values(),
        )

        response = litellm.completion(
            model=self.llm_config["model"],
            api_key=self.llm_config["api_key"],
            api_base=self.llm_config["api_base"],
            messages=[{"role": "user", "content": optimize_text(prompt)}],
            tools=self.function_configs,
            tool_choice="auto",
        )

        # 可能解析失败，返回 None 让外层感知
        return self.parse_tool_response(response)

    def generate_next_step(self, problem_class: str, solution_plan: str) -> Dict:
        """
        生成下一步执行命令
        :param problem_class: 题目类别
        :param solution_plan: 解题思路
        :return: 下一步命令字典
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary()

        # 根据题目类别选择不同的prompt模板
        prompt_key = problem_class.lower() + "_next"
        if prompt_key not in self.prompt:
            prompt_key = "general_next"

        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get(prompt_key, ""))
        prompt = template.render(
            question=self.problem,
            solution_plan=solution_plan,
            history_summary=history_summary,
            tools=self.tools.values(),
        )

        # 调用LLM生成下一步动作
        response = litellm.completion(
            model=self.llm_config["model"],
            api_key=self.llm_config["api_key"],
            api_base=self.llm_config["api_base"],
            messages=[{"role": "user", "content": optimize_text(prompt)}],
            tools=self.function_configs,
            tool_choice="auto",
        )

        # 解析工具调用响应
        return self.parse_tool_response(response)

    def parse_tool_response(self, response: ModelResponse) -> Dict:
        """统一解析工具调用响应，处理两种格式的响应"""
        message = response.choices[0].message

        # 情况1：直接工具调用格式（tool_calls）
        if hasattr(message, "tool_calls") and message.tool_calls:
            tool_call = message.tool_calls[0]
            func_name = tool_call.function.name
            try:
                args = json.loads(tool_call.function.arguments)
            except json.JSONDecodeError as e:
                args = utils.fix_json_with_llm(tool_call.function.arguments, e)

            # 确保参数中包含purpose和content
            args.setdefault("purpose", "执行操作")
            args.setdefault("content", "")

        # 情况2：JSON字符串格式
        else:
            content = message.content.strip()
            # 尝试直接解析JSON
            try:
                data = json.loads(content)
            except json.JSONDecodeError as e:
                print("无法解析工具调用响应，尝试修复")
                content = utils.fix_json_with_llm(content, e)
                data = json.loads(content)
            except Exception as e:
                print(f"无法解析工具调用响应：{e}")
                return {}
            if "tool_calls" in data and data["tool_calls"]:
                tool_call: dict = data["tool_calls"][0]
                func_name: dict = tool_call.get("name", "工具解析失败")
                args: dict = tool_call.get("arguments", {})
                args.setdefault("purpose", "执行操作")
                args.setdefault("content", "")

        logger.info(f"使用工具: {func_name}")
        logger.info(f"命令目的: {args['purpose']}")
        logger.info(f"执行命令:\n{args['content']}")
        return {"tool_name": func_name, "arguments": args}
