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
from typing import Dict, Tuple, Optional, List
from .memory import Memory
from . import utils
from jinja2 import Environment, FileSystemLoader

logger = logging.getLogger(__name__)


class SolveAgent:
    def __init__(self, config: dict):
        self.config = config
        litellm.enable_json_schema_validation = True
        self.prompt:dict = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
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
            arguments = next_step.get("arguments", {})
            purpose = arguments.get("purpose", "未指定目的")
            content = arguments.get("content", "")

            logger.info(f"使用工具: {tool_name}")
            logger.info(f"命令目的: {purpose}")
            logger.info(f"执行命令:\n{content}")
            # 手动模式：需要用户批准命令
            if not self.auto_mode:
                approved, next_step = self.manual_approval_step(next_step)
                if not approved:
                    print("用户终止解题")
                    return "解题终止"
                # 更新参数
                tool_name = next_step.get("tool_name")
                arguments = next_step.get("arguments", {})
                purpose = arguments.get("purpose", "未指定目的")
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
            analysis_result = self.analyze_step_output(
                step_count, content, output, solution_plan
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
                # 仅重思，不立即执行
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
            original_purpose=purpose,
            feedback=feedback,
            history_summary=history_summary,
            tools=self.tools.values(),
        )

        response = litellm.completion(
            model=self.config["model"],
            api_key=self.config["api_key"],
            api_base=self.config["api_base"],
            messages=[{"role": "user", "content": prompt}],
            tools=self.function_configs,
            tool_choice="auto",
        )

        # 可能解析失败，返回 None 让外层感知
        return self.parse_tool_call(response)

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
            solution_plan=solution_plan,
            history_summary=history_summary,
            tools=self.tools.values(),
        )

        # 调用LLM生成下一步动作
        response = litellm.completion(
            model=self.config["model"],
            api_key=self.config["api_key"],
            api_base=self.config["api_base"],
            messages=[{"role": "user", "content": prompt}],
            tools=self.function_configs,
            tool_choice="auto",
        )

        # 解析工具调用响应
        return self.parse_tool_call(response)

    def parse_tool_call(self, response: ModelResponse) -> Dict:
        """解析工具调用响应"""
        message = response.choices[0].message

        # 检查是否有工具调用
        if not hasattr(message, "tool_calls") or not message.tool_calls:
            # 没有工具调用时尝试解析JSON
            return self.parse_next_step(message.content)

        # 处理第一个工具调用
        tool_call = message.tool_calls[0]
        func_name = tool_call.function.name
        try:
            args = json.loads(tool_call.function.arguments)
        except json.JSONDecodeError as e:
            args = utils.fix_json_with_llm(tool_call.function.arguments, e, self.config)

        # 确保参数中包含purpose和content
        args.setdefault("purpose", "执行操作")
        args.setdefault("content", "")

        return {"tool_name": func_name, "arguments": args}

    def parse_next_step(self, llm_output: str) -> Dict:
        """解析LLM输出的下一步命令，增加重试和修复机制"""
        while True:
            # 尝试提取JSON部分
            try:
                data: dict = json.loads(llm_output)
                tool_call: dict = data["tool_calls"][0]
                func_name = tool_call.get("name", "工具解析失败")
                args: dict = tool_call.get("arguments", {})
                # 确保参数中包含purpose和content
                args.setdefault("purpose", "执行操作")
                args.setdefault("content", "")
                return {"tool_name": func_name, "arguments": args}
            except json.JSONDecodeError as e:
                # JSON解析失败，尝试修复
                print(f"JSON解析失败: {str(e)}，尝试修复...")
                fixed_json = utils.fix_json_with_llm(
                    llm_output, err_content=e, config=self.config
                )
                if fixed_json:
                    llm_output = fixed_json
                    continue
            except Exception as e:
                # 其他异常，返回空字典
                print(f"解析下一步命令失败: {str(e)}")
                return {}

    def analyze_step_output(
        self, step_num: int, content: str, output: str, solution_plan: str
    ) -> Dict:
        """
        使用LLM分析步骤输出
        :param step_num: 步骤编号
        :param content: 执行的内容
        :param output: 命令输出
        :param solution_plan: 原始解题思路
        :return: 分析结果字典
        """
        # 获取记忆摘要
        history_summary = self.memory.get_summary()

        # 使用Jinja2渲染提示
        template = self.env.from_string(self.prompt.get("step_analysis", ""))
        prompt = template.render(
            step_num=step_num,
            content=content,
            output=output[:4096],
            solution_plan=solution_plan,
            history_summary=history_summary,
        )

        # 调用LLM进行分析
        response = litellm.completion(
            model=self.config["model"],
            api_key=self.config["api_key"],
            api_base=self.config["api_base"],
            messages=[{"role": "user", "content": prompt}],
        )

        # 解析分析结果
        try:
            result = json.loads(response.choices[0].message.content)
            if isinstance(result,dict):
                return result
        except (json.JSONDecodeError, KeyError):
            pass

        # 解析失败时返回默认结果
        return {
            "analysis": "分析失败",
            "terminate": False,
            "recommendations": "继续执行",
            "flag_found": False,
            "flag": "",
        }
