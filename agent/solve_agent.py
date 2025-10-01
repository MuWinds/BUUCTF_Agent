import litellm
import re
import json
import time
import yaml
import os
import inspect
import importlib
from ctf_tool.base_tool import BaseTool
from typing import Dict, Tuple, Optional, List
from .memory import Memory
from jinja2 import Environment, FileSystemLoader

class SolveAgent:
    def __init__(self, config: dict):
        self.config = config
        self.prompt = yaml.safe_load(open("./prompt.yaml", "r", encoding="utf-8"))
        if self.config is None:
            raise ValueError("找不到配置文件")
        
        # 初始化Jinja2模板环境
        self.env = Environment(loader=FileSystemLoader('.'))
        
        # 初始化记忆系统
        self.memory = Memory(
            config=self.config,
            max_steps=self.config.get("max_history_steps", 10),
            compression_threshold=self.config.get("compression_threshold", 5)
        )
        
        # 动态加载工具
        self.tools: Dict[str, BaseTool] = {}  # 工具名称 -> 工具实例
        self.function_configs: List[Dict] = []  # 函数调用配置列表
        
        # 加载ctf_tools文件夹中的所有工具
        self._load_tools()
        
        # 添加模式设置
        self.auto_mode = self.config.get("auto_mode", True)  # 默认为自动模式

        # 添加flag确认回调函数
        self.confirm_flag_callback = None  # 将由Workflow设置

    def _load_tools(self):
        """动态加载tool文件夹中的所有工具"""
        tools_dir = os.path.join(os.path.dirname(__file__), "..", "ctf_tool")
        
        for file_name in os.listdir(tools_dir):
            if file_name.endswith(".py") and file_name != "__init__.py" and file_name != "base_tool.py":
                module_name = file_name[:-3]  # 移除.py
                try:
                    # 导入模块
                    module = importlib.import_module(f"ctf_tool.{module_name}")
                    
                    # 查找所有继承自BaseTool的类
                    for name, obj in inspect.getmembers(module):
                        if inspect.isclass(obj) and issubclass(obj, BaseTool) and obj != BaseTool:
                            # 检查是否需要特殊配置
                            if name in self.config.get("tool_config", {}):
                                # 使用配置创建实例
                                tool_config = self.config["tool_config"][name]
                                tool_instance = obj(tool_config)
                            else:
                                # 创建默认实例
                                tool_instance = obj()
                            
                            # 添加到工具字典
                            tool_name = tool_instance.function_config["function"]["name"]
                            self.tools[tool_name] = tool_instance
                            
                            # 添加工具配置
                            self.function_configs.append(tool_instance.function_config)
                            
                            print(f"已加载工具: {tool_name}")
                except Exception as e:
                    print(f"加载工具{module_name}失败: {str(e)}")

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
            else:
                print(f"使用工具: {tool_name}")
                print(f"命令目的: {purpose}")
                print(f"执行命令: {content}")
            # 执行命令
            output = ""
            if tool_name in self.tools:
                try:
                    tool = self.tools[tool_name]
                    result = tool.execute(arguments)
                    if isinstance(result, tuple) and len(result) == 2:
                        stdout, stderr = result
                        output = stdout + stderr
                    else:
                        output = str(result)
                except Exception as e:
                    output = f"工具执行出错: {str(e)}"
            else:
                output = f"错误: 未找到工具 '{tool_name}'"
            
            print(f"命令输出:\n{output}")
            
            # 使用LLM分析输出
            analysis_result = self.analyze_step_output(
                step_count, 
                content, 
                output, 
                solution_plan
            )
            
            # 检查LLM是否在输出中发现了flag
            if analysis_result.get("flag_found", False):
                flag_candidate = analysis_result.get("flag", "")
                print(f"LLM报告发现flag: {flag_candidate}")
                
                # 使用回调函数确认flag
                if self.confirm_flag_callback and self.confirm_flag_callback(flag_candidate):
                    return flag_candidate
                else:
                    print("用户确认flag不正确，继续解题")
            
            # 添加执行历史到记忆系统
            self.memory.add_step({
                "step": step_count,
                "content": content,
                "output": output,
                "analysis": analysis_result
            })
            
            # 检查是否应该提前终止
            if analysis_result.get("terminate", False):
                print("LLM建议提前终止解题")
                return "未找到flag：提前终止"

    def manual_approval_step(self, next_step: Dict) -> Tuple[bool, Optional[Dict]]:
        """手动模式：让用户无限次反馈/重思，直到 ta 主动选 1 或 3"""
        while True:
            arguments = next_step.get("arguments", {})
            purpose = arguments.get("purpose", "未指定目的")
            content = arguments.get("content", "")

            print("\n-----------------------------")
            print(f"目的: {purpose}")
            print(f"命令/代码:\n {content}")
            print("-----------------------------")
            print("1. 批准并执行")
            print("2. 提供反馈并重新思考")
            print("3. 终止解题")
            choice = input("请输入选项编号: ").strip()

            if choice == "1":
                return True, next_step
            elif choice == "2":
                feedback = input("请提供改进建议: ").strip()
                # 仅重思，不立即执行
                next_step = self.regenerate_with_feedback(purpose, feedback)
                if not next_step:
                    print("（思考失败，可继续反馈或选 3 终止）")
                # **此处不再 return**，循环回到菜单让用户再次判断
            elif choice == "3":
                return False, None
            else:
                print("无效选项，请重新选择")
                
    def regenerate_with_feedback(self, purpose: str, feedback: str) -> Dict:
        """
        根据用户反馈重新生成命令
        返回的是“LLM 重新思考后的 next_step”，
        后续仍需回到 manual_approval_step 让用户再次确认。
        """
        history_summary = self.memory.get_summary()

        template = self.env.from_string(self.prompt.get("regenerate_with_feedback", ""))
        prompt = template.render(
            original_purpose=purpose,
            feedback=feedback,
            history_summary=history_summary,
            tools=self.tools.values()
        )

        response = litellm.completion(
            model=self.config["model"],
            api_key=self.config["api_key"],
            api_base=self.config["api_base"],
            messages=[{"role": "user", "content": prompt}],
            tools=self.function_configs,
            tool_choice="auto"
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
            tools=self.tools.values()
        )
        
        # 调用LLM生成下一步动作
        response = litellm.completion(
            model=self.config["model"],
            api_key=self.config["api_key"],
            api_base=self.config["api_base"],
            messages=[{"role": "user", "content": prompt}],
            tools=self.function_configs,
            tool_choice="auto"
        )
        
        # 解析工具调用响应
        return self.parse_tool_call(response)
    
    def parse_tool_call(self, response) -> Dict:
        """解析工具调用响应"""
        message = response.choices[0].message
        
        # 检查是否有工具调用
        if not hasattr(message, 'tool_calls') or not message.tool_calls:
            # 没有工具调用时尝试解析JSON
            return self.parse_next_step(message.content)
        
        # 处理第一个工具调用
        tool_call = message.tool_calls[0]
        func_name = tool_call.function.name
        try:
            args = json.loads(tool_call.function.arguments)
        except json.JSONDecodeError:
            args = {}
        
        # 确保参数中包含purpose和content
        args.setdefault("purpose", "执行操作")
        args.setdefault("content", "")
        
        return {
            "tool_name": func_name,
            "arguments": args
        }

    def parse_next_step(self, llm_output: str, max_retries: int = 3) -> Dict:
        """解析LLM输出的下一步命令，增加重试和修复机制"""
        while True:
            try:
                # 尝试提取JSON部分
                json_str = re.search(r'\{.*\}', llm_output, re.DOTALL)
                if json_str:
                    try:
                        data = json.loads(json_str.group(0))
                        # 检查是否包含tool_calls数组
                        if "tool_calls" in data and isinstance(data["tool_calls"], list) and len(data["tool_calls"]) > 0:
                            tool_call = data["tool_calls"][0]
                            func_name = tool_call.get("name")
                            args = tool_call.get("arguments", {})
                            # 确保参数中包含purpose和content
                            args.setdefault("purpose", "执行操作")
                            args.setdefault("content", "")
                            return {
                                "tool_name": func_name,
                                "arguments": args
                            }
                        else:
                            # 直接包含tool_name和arguments
                            result = data
                            result.setdefault("tool_name", "execute_shell_command")
                            result.setdefault("arguments", {})
                            result["arguments"].setdefault("purpose", "执行操作")
                            result["arguments"].setdefault("content", "")
                            return result
                    except json.JSONDecodeError as e:
                        # JSON解析失败，尝试修复
                        print(f"JSON解析失败: {str(e)}，尝试修复...")
                        fixed_json = self.fix_json_format(json_str.group(0))
                        if fixed_json:
                            llm_output = fixed_json
                            continue
                else:
                    # 没有找到JSON，尝试让LLM修复
                    print("未找到JSON结构，尝试修复...")
                    fixed_output = self.fix_missing_json(llm_output)
                    if fixed_output:
                        llm_output = fixed_output
                        retries += 1
                        continue
            except (json.JSONDecodeError, KeyError) as e:
                print(f"JSON解析异常: {str(e)}")
                retries += 1
                continue

    def fix_json_format(self, json_str: str) -> str:
        """使用LLM修复格式错误的JSON"""
        prompt = (
            "以下是一个格式错误的JSON字符串，请修复它使其成为有效的JSON。"
            "只返回修复后的JSON，不要包含任何其他内容。"
            "确保保留所有原始键值对。\n\n"
            f"错误JSON: {json_str}"
        )
        try:
            response = litellm.completion(
                model=self.config["model"],
                api_key=self.config["api_key"],
                api_base=self.config["api_base"],
                messages=[{"role": "user", "content": prompt}],
                max_tokens=512
            )
            return response.choices[0].message.content.strip()
        except Exception as e:
            print(f"修复JSON失败: {str(e)}")
            return ""

    def analyze_step_output(self, step_num: int, content: str, output: str, solution_plan: str) -> Dict:
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
            output=output[:2048],
            solution_plan=solution_plan,
            history_summary=history_summary
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
            json_str = re.search(r'\{.*\}', response.choices[0].message.content, re.DOTALL)
            if json_str:
                return json.loads(json_str.group(0))
        except (json.JSONDecodeError, KeyError):
            pass
        
        # 解析失败时返回默认结果
        return {
            "analysis": "分析失败",
            "terminate": False,
            "recommendations": "继续执行",
            "flag_found": False,
            "flag": ""
        }