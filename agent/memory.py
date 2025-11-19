import litellm
import json
import logging
from typing import List, Dict
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class Memory:
    def __init__(
        self, config: dict, max_steps: int = 15, compression_threshold: int = 7
    ):
        """
        记忆管理类
        :param config: 配置字典
        :param max_steps: 最大保存步骤数
        :param compression_threshold: 触发压缩的步骤阈值
        """
        self.config = config
        self.llm_config = self.config["llm"]["solve_agent"]
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
                command = step.get("content", "")
                self.failed_attempts[command] = self.failed_attempts.get(command, 0) + 1

        # 检查是否需要压缩记忆
        if len(self.history) >= self.compression_threshold:
            self.compress_memory()

    def _extract_key_facts(self, step: Dict) -> None:
        """从步骤中提取关键事实并存储"""
        # 提取关键命令和结果
        if "content" in step and "output" in step:
            command = step["content"]
            output_summary = step["output"][:256] + (
                "..." if len(step["output"]) > 256 else ""
            )
            self.key_facts[f"command"] = f"命令：{command},结果: {output_summary}"

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
            prompt += f"- 目的: {step.get('purpose', '未指定')}\n"
            prompt += f"- 命令: {step['content']}\n"

            # 添加分析结果（如果有）
            if "analysis" in step:
                analysis = step["analysis"].get("analysis", "无分析")
                prompt += f"- 分析: {analysis}\n"

        try:
            # 调用LLM生成结构化记忆
            litellm.enable_json_schema_validation = True
            response = litellm.completion(
                model=self.llm_config["model"],
                api_key=self.llm_config["api_key"],
                api_base=self.llm_config["api_base"],
                messages=[{"role": "user", "content": optimize_text(prompt)}],
                max_tokens=1024,
            )

            # 解析并存储压缩记忆
            json_str = response.choices[0].message.content.strip()
            compressed_data = json.loads(json_str)

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
            print(f"记忆压缩解析失败: {str(e)}")
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
                summary += f"步骤 #{step_num}:\n- 思考: {step.get('think', 'N/A')}\n"
                summary += f"- 工具: {step.get('tool_name', 'N/A')} (类别: {step.get('tool_category', 'N/A')})\n"
                summary += f"- 参数: {step.get('tool_args', 'N/A')}\n"

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
