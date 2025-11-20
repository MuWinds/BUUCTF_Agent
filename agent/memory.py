import litellm
import json
import logging
import json_repair
import time
from config import Config
from typing import List, Dict, Optional
from rag.memory_base import MemorySystem
from utils.text import optimize_text

logger = logging.getLogger(__name__)


class Memory:
    def __init__(self, max_steps: int = 15, compression_threshold: int = 7):
        self.config = Config.load_config()
        self.llm_config = self.config["llm"]["solve_agent"]
        self.max_steps = max_steps
        self.compression_threshold = compression_threshold

        # 中短期记忆结构
        self.history: List[Dict] = []  # 近期细节
        self.compressed_memory: List[Dict] = []  # 压缩的记忆块
        self.key_facts: Dict[str, str] = {}  # 重要事实摘要
        self.failed_attempts: Dict[str, int] = {}  # 失败尝试记录

        # 长期记忆系统
        self.memory_system = MemorySystem()

        # 遗忘机制相关属性
        self.access_counts: Dict[str, int] = {}  # 访问次数统计
        self.importance_scores: Dict[str, float] = {}  # 重要性评分
        self.timestamps: Dict[str, float] = {}  # 时间戳记录
        
        # 新增：工具使用统计
        self.tool_usage_stats: Dict[str, Dict] = {}  # 工具使用统计

    def add_step(self, step: Dict, current_problem: Optional[str] = None) -> None:
        """
        添加一个新的步骤到记忆系统中，并根据需要进行压缩。
        """
        # 记录时间戳和唯一标识
        step_id = str(len(self.history))
        step["step_id"] = step_id
        step["timestamp"] = time.time()
        self.timestamps[step_id] = step["timestamp"]

        # 更新工具使用统计
        self._update_tool_stats(step)

        self.history.append(step)
        self._extract_key_facts(step)

        # 记录失败尝试
        if "analysis" in step and "success" in step["analysis"]:
            if not step["analysis"]["success"]:
                tool_key = self._get_tool_attempt_key(step)
                self.failed_attempts[tool_key] = self.failed_attempts.get(tool_key, 0) + 1

        # 评估重要性
        importance = self._assess_importance(step)
        self.importance_scores[step_id] = importance

        if len(self.history) >= self.compression_threshold and current_problem:
            self.compress_memory(current_problem)
            self.forget_memory()

    def _get_tool_attempt_key(self, step: Dict) -> str:
        """生成工具尝试的唯一键"""
        tool_name = step.get("tool_name", "unknown")
        tool_category = step.get("tool_category", "unknown")
        tool_args = step.get("tool_args", {})
        
        # 创建包含工具信息的键
        args_str = json.dumps(tool_args, sort_keys=True) if isinstance(tool_args, dict) else str(tool_args)
        return f"{tool_category}:{tool_name}:{args_str}"

    def _update_tool_stats(self, step: Dict) -> None:
        """更新工具使用统计"""
        tool_name = step.get("tool_name")
        tool_category = step.get("tool_category")
        
        if tool_name:
            if tool_name not in self.tool_usage_stats:
                self.tool_usage_stats[tool_name] = {
                    "category": tool_category,
                    "usage_count": 0,
                    "last_used": step.get("timestamp", time.time()),
                    "success_count": 0
                }
            
            stats = self.tool_usage_stats[tool_name]
            stats["usage_count"] += 1
            stats["last_used"] = step.get("timestamp", time.time())

    def _extract_key_facts(self, step: Dict) -> None:
        """从步骤中提取关键事实并存储，集成工具信息"""
        tool_name = step.get("tool_name", "未知工具")
        tool_category = step.get("tool_category", "未知类别")
        
        # 构建包含工具信息的键
        tool_info = f"{tool_category}:{tool_name}"
        
        if "tool_args" in step and "output" in step:
            command = step["tool_args"]
            output_summary = step["output"][:4096]  # 缩短输出摘要长度
            
            # 更结构化的关键事实存储
            fact_key = f"{tool_info}:{hash(str(command))}"
            self.key_facts[fact_key] = (
                f"工具: {tool_name} (类别: {tool_category})\n"
                f"参数: {command}\n"
                f"结果摘要: {output_summary}"
            )

        if "analysis" in step and "analysis" in step["analysis"]:
            analysis = step["analysis"]["analysis"]
            if any(keyword in analysis for keyword in ["关键发现", "重要", "flag"]):
                fact_key = f"analysis:{tool_info}:{hash(analysis)}"
                self.key_facts[fact_key] = (
                    f"分析发现 (工具: {tool_name}): {analysis}"
                )

    def compress_memory(self, current_problem: str) -> None:
        """
        压缩当前的记忆历史，提取关键信息并存储到长期记忆系统中。
        集成工具类别和名称信息。
        """
        logger.info("正在压缩记忆以提取关键信息...")
        if not self.history:
            return

        # 增强的提示词，包含工具信息
        prompt = """
                请分析以下CTF解题历史记录，提取关键信息并压缩记忆。请特别注意工具的使用情况和效果。

                请返回以下结构的JSON数据：
                {
                "key_findings": ["关键发现1", "关键发现2"],
                "failed_attempts": ["失败尝试描述1", "失败尝试描述2"],
                "effective_tools": ["有效工具1", "有效工具2"],
                "ineffective_tools": ["无效工具1", "无效工具2"],
                "current_status": "当前解题状态描述",
                "next_steps": ["建议下一步1", "建议下一步2"],
                "tool_insights": "关于工具使用效果的见解"
                }

                历史记录:
                """

        # 添加工具使用统计摘要
        if self.tool_usage_stats:
            prompt += "\n工具使用统计:\n"
            for tool, stats in list(self.tool_usage_stats.items())[-5:]:
                prompt += f"- {tool} (类别: {stats['category']}): 使用{stats['usage_count']}次\n"

        prompt += "\n关键事实摘要:\n"
        for _, value in list(self.key_facts.items())[-5:]:
            prompt += f"- {value}\n"

        # 详细步骤记录，包含工具信息
        for i, step in enumerate(self.history[-self.compression_threshold:]):
            prompt += f"\n步骤 {i+1}:\n"
            prompt += f"- 思考: {step.get('think', 'N/A')}\n"
            prompt += f"- 工具: {step.get('tool_name', 'N/A')} (类别: {step.get('tool_category', 'N/A')})\n"
            prompt += f"- 参数: {step.get('tool_args', 'N/A')}\n"
            if "analysis" in step:
                analysis = step['analysis'].get('analysis', '无分析')
                prompt += f"- 分析: {analysis} \n"

        try:
            response = litellm.completion(
                model=self.llm_config["model"],
                api_key=self.llm_config["api_key"],
                api_base=self.llm_config["api_base"],
                messages=[{"role": "user", "content": optimize_text(prompt)}],
                max_tokens=1024
            )

            json_str = response.choices[0].message.content.strip()
            compressed_data = json_repair.loads(json_str)

            # 处理失败尝试
            for attempt in compressed_data.get("failed_attempts", []):
                self.failed_attempts[attempt] = self.failed_attempts.get(attempt, 0) + 1

            compressed_data["source_steps"] = len(self.history)
            compressed_data["compression_timestamp"] = time.time()
            self.compressed_memory.append(compressed_data)

            # 保存到长期记忆
            solution_summary = (
                f"状态: {compressed_data.get('current_status', 'N/A')}. "
                f"关键发现: {', '.join(compressed_data.get('key_findings', []))}. "
                f"有效工具: {', '.join(compressed_data.get('effective_tools', []))}. "
                f"工具洞察: {compressed_data.get('tool_insights', '无')}"
            )
            
            self.memory_system.add_problem_solution_memory(
                problem=current_problem,
                solution=solution_summary,
                problem_type="CTF-intermediate-summary",
            )
            
            logger.info(f"记忆压缩完成，存储了 {len(self.history)} 条历史步骤的关键信息。")

        except (json.JSONDecodeError, KeyError) as e:
            logger.error(f"记忆压缩失败：{e}")
            fallback = response.choices[0].message.content.strip() if 'response' in locals() else "Compression failed"
            self.compressed_memory.append({"总结": fallback, "来自步骤": len(self.history)})
        except Exception as e:
            logger.error(f"记忆压缩失败：{e}")

        # 保留最近几步的详细历史
        keep_last = min(4, len(self.history))
        self.history = self.history[-keep_last:]

    def get_summary(self, current_problem: str, include_key_facts: bool = True) -> str:
        """
        获取当前记忆的总结，用于提示构建。集成工具信息。
        """
        # 更新访问计数
        for step in self.history:
            step_id = step.get("step_id")
            if step_id:
                self.access_counts[step_id] = self.access_counts.get(step_id, 0) + 1

        # 获取相关记忆
        retrieved_memories = self.memory_system.get_relevant_memories_for_prompt(current_problem)

        summary_parts = []

        if retrieved_memories:
            summary_parts.append(f"{retrieved_memories}")

        summary_parts.append("当前记忆状态:")

        # 工具使用统计摘要
        if self.tool_usage_stats:
            summary_parts.append("\n工具使用统计:")
            for tool, stats in list(self.tool_usage_stats.items())[-5:]:
                summary_parts.append(
                    f"- {tool} ({stats['category']}): 使用{stats['usage_count']}次)"
                )

        if include_key_facts and self.key_facts:
            summary_parts.append("\n关键事实:")
            for _, value in list(self.key_facts.items())[-8:]:
                summary_parts.append(f"- {value}")

        if self.compressed_memory:
            summary_parts.append("\n压缩记忆块:")
            for i, mem in enumerate(self.compressed_memory[-3:]):
                block_num = len(self.compressed_memory) - 3 + i + 1
                summary_parts.append(f"记忆块 #{block_num}:")
                if "key_findings" in mem:
                    summary_parts.append(f"- 状态: {mem.get('current_status', 'Unknown')}")
                    summary_parts.append(f"- 关键发现: {', '.join(mem['key_findings'][:3])}")
                    if "effective_tools" in mem and mem["effective_tools"]:
                        summary_parts.append(f"- 有效工具: {', '.join(mem['effective_tools'])}")
                summary_parts.append(f"(来自 {mem.get('source_steps', 'N/A')} 个步骤)\n")

        if self.history:
            summary_parts.append("\n最近详细步骤:")
            for step in self.history:
                step_id = step.get("step_id", "")
                summary_parts.append(f"步骤 {step_id}:")
                summary_parts.append(f"- 思路: {step.get('think', 'N/A')}")
                
                tool_name = step.get("tool_name", "未知")
                tool_category = step.get("tool_category", "未知")
                summary_parts.append(f"- 工具: {tool_name} (类别: {tool_category})")
                
                tool_args = step.get("tool_args", {})
                command_key = json.dumps(tool_args) if isinstance(tool_args, dict) else str(tool_args)
                summary_parts.append(f"- 参数: {command_key}")
                
                if "output" in step:
                    output_preview = step["output"][:4096] + ("..." if len(step["output"]) > 512 else "")
                    summary_parts.append(f"- 输出预览: {output_preview}")
                
                # 失败次数统计
                tool_key = self._get_tool_attempt_key(step)
                if tool_key in self.failed_attempts:
                    summary_parts.append(f"- 类似尝试失败次数: {self.failed_attempts[tool_key]}")
                
                summary_parts.append("")

        return "\n".join(summary_parts) if summary_parts else "无历史记录"

    def _assess_importance(self, step: Dict) -> float:
        """
        根据分析内容和工具信息评估步骤的重要性，返回评分 (0~1)
        """
        importance = 0.1  # 默认基础值
        
        # 分析结果的重要性
        if "analysis" in step:
            analysis = step["analysis"]
            if any(keyword in analysis.get("analysis", "") for keyword in ["关键发现", "重要", "flag"]):
                importance += 0.3
            if analysis.get("flag_found", False):
                importance += 0.5  # 发现flag的步骤非常重要
        
        # 输出内容的重要性
        if "output" in step:
            output = step["output"]
            if len(output) > 10:  # 有实质性输出
                importance += 0.1
            if "flag" in output.lower():  # 输出中包含flag关键词
                importance += 0.2
        
        return min(importance, 1.0)

    def _evaluate_forgetting_weight(self, step_id: str) -> float:
        """
        计算遗忘权重（三因素加权）：时间、访问频率、重要性
        """
        current_time = time.time()
        timestamp = self.timestamps.get(step_id, current_time)
        age = (current_time - timestamp) / 60  # 分钟为单位

        access_count = self.access_counts.get(step_id, 0)
        importance = self.importance_scores.get(step_id, 0.1)

        # 动态权重计算
        time_weight = min(age / 2, 1)  # 2分钟后达到最大时间权重
        access_weight = 1 / (access_count + 1)
        importance_weight = 1 - importance

        # 综合权重（可调节参数）
        weight = 0.4 * time_weight + 0.4 * access_weight + 0.2 * importance_weight
        return weight

    def forget_memory(self, threshold: float = 0.6) -> None:
        """
        执行遗忘机制，移除低权重的记忆项
        """
        to_remove = []
        for step in self.history:
            step_id = step.get("step_id")
            if not step_id:
                continue
            weight = self._evaluate_forgetting_weight(step_id)
            if weight > threshold:
                to_remove.append(step)

        # 移除低权重记忆
        for step in to_remove:
            self.history.remove(step)
            step_id = step.get("step_id")
            if step_id:
                self.timestamps.pop(step_id, None)
                self.access_counts.pop(step_id, None)
                self.importance_scores.pop(step_id, None)
        
        if to_remove:
            logger.info(f"遗忘机制执行，移除了 {len(to_remove)} 条低权重记忆。")
            
            # 同时清理相关的关键事实
            self._cleanup_orphaned_facts()

    def _cleanup_orphaned_facts(self) -> None:
        """清理孤立的关键事实"""
        current_step_ids = {step.get("step_id") for step in self.history}
        
        # 标记需要清理的事实键
        to_remove = []
        for key in self.key_facts.keys():
            # 检查事实键是否关联到已删除的步骤
            if any(step_id in key for step_id in current_step_ids if step_id):
                continue
            to_remove.append(key)
        
        # 清理孤立事实
        for key in to_remove:
            self.key_facts.pop(key, None)
        
        if to_remove:
            logger.debug(f"清理了 {len(to_remove)} 个孤立的关键事实")