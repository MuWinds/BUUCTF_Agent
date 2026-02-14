import json
import json_repair
import logging
from utils.llm_request import LLMRequest
from config import Config
from typing import List, Dict
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class Memory:
    def __init__(
        self, max_steps: int = 15, compression_threshold: int = 7
    ):
        """
        记忆管理类
        :param config: 配置字典
        :param max_steps: 最大保存步骤数
        :param compression_threshold: 触发压缩的步骤阈值
        """
        self.config = Config.load_config()
        self.solve_llm = LLMRequest("solve_agent")
        self.max_steps = max_steps
        self.compression_threshold = compression_threshold
        self.history: List[Dict] = []  # 详细历史记录
        self.compressed_memory: List[Dict] = []  # 压缩后的记忆块
        self.key_facts: Dict[str, str] = {}  # 关键事实存储（结构化）
        self.failed_attempts: Dict[str, int] = {}  # 记录失败尝试

    def add_step(self, step: Dict) -> None:
        """添加新的步骤到历史记录，并提取关键信息"""
        self.history.append(step)

        # 提取关键事实（命令、输出、分析）
        self._extract_key_facts(step)

        # 记录失败尝试
        if "analysis" in step and "success" in step["analysis"]:
            if not step["analysis"]["success"]:
                if "tool_calls" in step and step["tool_calls"]:
                    # 将工具调用列表转为字符串作为键
                    command = str([(t.get('tool_name'), t.get('arguments')) for t in step["tool_calls"]])
                else:
                    command = str(step.get("tool_args", ""))
                self.failed_attempts[command] = self.failed_attempts.get(command, 0) + 1

        # 检查是否需要压缩记忆
        if len(self.history) >= self.compression_threshold:
            self.compress_memory()

    def add_planned_step(self, step_num: int, think: str, tool_calls: List[Dict]) -> None:
        self.history.append({
            "step": step_num,
            "think": think,
            "tool_calls": tool_calls,
            "status": "planned"
        })

    def update_step(self, step_num: int, fields: Dict) -> None:
        for i in range(len(self.history) - 1, -1, -1):
            s = self.history[i]
            if s.get("step") == step_num:
                s.update(fields)
                break

    def _extract_key_facts(self, step: Dict) -> None:
        """从步骤中提取关键事实并存储，支持多个工具调用"""
        # 提取关键命令和结果
        if "tool_calls" in step and step["tool_calls"]:
            # 处理多个工具调用
            tool_call_summary = []
            for i, tool_call in enumerate(step["tool_calls"], 1):
                tool_name = tool_call.get("tool_name", "未知工具")
                args = tool_call.get("arguments", {})
                tool_call_summary.append(f"{i}. {tool_name}({args})")
            
            self.key_facts[f"tool_calls"] = f"工具调用: {', '.join(tool_call_summary)}"
        
        # 处理单个工具调用（向后兼容）
        elif "tool_args" in step and step["tool_args"]:
            command = str(step["tool_args"])
            self.key_facts[f"command"] = f"命令: {command}"

        # 提取输出摘要
        if "output" in step:
            output = step["output"]
            output_summary = output[:256] + ("..." if len(output) > 256 else "")
            self.key_facts[f"output_summary"] = f"输出摘要: {output_summary}"

        # 提取分析结论
        if "analysis" in step and "analysis" in step["analysis"]:
            analysis = step["analysis"]["analysis"]
            if "关键发现" in analysis:
                self.key_facts[f"finding:{hash(analysis)}"] = analysis

    def compress_memory(self) -> None:
        """压缩历史记录，生成结构化记忆块"""
        logger.info("优化记忆压缩中...")
        if not self.history:
            return

        # 构建更精细的压缩提示
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

        # 添加关键事实作为上下文
        prompt += "关键事实摘要:\n"
        for _, value in list(self.key_facts.items())[-5:]:  # 只取最近5个关键事实
            prompt += f"- {value}\n"

        # 添加历史步骤
        for i, step in enumerate(self.history[-self.compression_threshold :]):
            prompt += f"\n步骤 {i+1}:\n"
            prompt += f"- 目的: {step.get('think', '未指定')}\n"
            if step.get('tool_calls'):
                # 优先使用多工具调用的摘要
                tc = []
                for t in step.get('tool_calls', []):
                    name = t.get('tool_name', '未知工具')
                    args = t.get('arguments', {})
                    tc.append(f"{name}({args})")
                prompt += f"- 命令: {', '.join(tc)}\n"
            else:
                prompt += f"- 命令: {step.get('tool_args', {})}\n"

            # 添加分析结果（如果有）
            if "analysis" in step:
                analysis = step["analysis"].get("analysis", "无分析")
                prompt += f"- 分析: {analysis}\n"

        try:
            # 调用LLM生成结构化记忆
            response = self.solve_llm.text_completion(
                prompt=optimize_text(prompt),
                json_check=True,
                max_tokens=1024
            )
            # 解析并存储压缩记忆
            json_str = response.choices[0].message.content.strip()
            compressed_data = json_repair.loads(json_str)

            # 更新失败尝试记录
            for attempt in compressed_data.get("failed_attempts", []):
                self.failed_attempts[attempt] = self.failed_attempts.get(attempt, 0) + 1

            # 添加时间戳和来源信息
            compressed_data["source_steps"] = len(self.history)

            self.compressed_memory.append(compressed_data)
            print(
                f"记忆压缩成功: 添加了{len(compressed_data['key_findings'])}个关键发现"
            )

        except (json.JSONDecodeError, KeyError) as e:
            # 回退到文本摘要
            fallback = (
                response.choices[0].message.content.strip()
                if "response" in locals()
                else "压缩失败"
            )
            self.compressed_memory.append(
                {"fallback_summary": fallback, "source_steps": len(self.history)}
            )
        except Exception as e:
            print(f"记忆压缩失败: {str(e)}")
            self.compressed_memory.append(
                {"error": f"压缩失败: {str(e)}", "source_steps": len(self.history)}
            )

        # 清空历史记录，但保留最后几步上下文
        keep_last = min(4, len(self.history))
        self.history = self.history[-keep_last:]

    def to_dict(self) -> dict:
        """导出记忆状态为字典，用于存档"""
        return {
            "history": self.history,
            "compressed_memory": self.compressed_memory,
            "key_facts": self.key_facts,
            "failed_attempts": self.failed_attempts,
            "compression_threshold": self.compression_threshold,
        }

    def restore_from_dict(self, data: dict) -> None:
        """从字典恢复记忆状态"""
        self.history = data.get("history", [])
        self.compressed_memory = data.get("compressed_memory", [])
        self.key_facts = data.get("key_facts", {})
        self.failed_attempts = data.get("failed_attempts", {})
        self.compression_threshold = data.get("compression_threshold", self.compression_threshold)

    def get_summary(self, include_key_facts: bool = True) -> str:
        """获取综合记忆摘要"""
        summary = ""

        # 1. 关键事实摘要
        if include_key_facts and self.key_facts:
            summary += "关键事实:\n"
            for _, value in list(self.key_facts.items())[-10:]:  # 显示最近10个关键事实
                summary += f"- {value}\n"
            summary += "\n"

        # 2. 压缩记忆摘要
        if self.compressed_memory:
            summary += "压缩记忆块:\n"
            for i, mem in enumerate(self.compressed_memory[-3:]):  # 显示最近3个压缩块
                summary += f"记忆块 #{len(self.compressed_memory)-i}:\n"

                if "key_findings" in mem:
                    summary += f"- 状态: {mem.get('current_status', '未知')}\n"
                    summary += f"- 关键发现: {', '.join(mem['key_findings'][:3])}"
                    if len(mem["key_findings"]) > 3:
                        summary += f" 等{len(mem['key_findings'])}项"
                    summary += "\n"

                if "failed_attempts" in mem:
                    summary += f"- 失败尝试: {', '.join(mem['failed_attempts'][:3])}"
                    if len(mem["failed_attempts"]) > 3:
                        summary += f" 等{len(mem['failed_attempts'])}项"
                    summary += "\n"

                if "next_steps" in mem:
                    summary += f"- 建议步骤: {mem['next_steps'][0]}\n"

                summary += f"- 来源: 基于{mem['source_steps']}个历史步骤\n\n"

        # 3. 最近详细步骤
        if self.history:
            summary += "最近详细步骤:\n"
            for i, step in enumerate(self.history):
                step_num = len(self.history) - i
                summary += f"步骤 {step_num}:\n"
                summary += f"- 目的: {step.get('think', '未指定')}\n"
                if step.get('tool_calls'):
                    tc = []
                    for t in step.get('tool_calls', []):
                        name = t.get('tool_name', '未知工具')
                        args = t.get('arguments', {})
                        tc.append(f"{name}({args})")
                    summary += f"- 命令: {', '.join(tc)}\n"
                else:
                    summary += f"- 命令: {step.get('tool_args', {})}\n"

                # 显示输出摘要和分析
                if "output" in step:
                    output = step["output"]
                    summary += (
                        f"- 输出: {output[:512]}{'...' if len(output) > 512 else ''}\n"
                    )

                if "analysis" in step:
                    analysis = step["analysis"].get("analysis", "无分析")
                    summary += f"- 分析: {analysis}\n"

                # 显示失败次数
                if "content" in step and step["content"] in self.failed_attempts:
                    summary += (
                        f"- 历史失败次数: {self.failed_attempts[step['content']]}\n"
                    )

                summary += "\n"
        return summary if summary else "无历史记录"
