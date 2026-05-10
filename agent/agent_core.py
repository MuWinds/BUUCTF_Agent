"""
@brief 自主 Agent 核心循环。
"""

import logging
import re
from typing import Any, Callable, Dict, List, Optional

logger = logging.getLogger(__name__)


def run(
    messages: List[Dict[str, Any]],
    tools: list,
    tool_defs: list,
    llm: Any,
    on_message: Optional[Callable] = None,
    checkpoint_mgr: Any = None,
    problem: str = "",
) -> str:
    """
    @brief 自主 agent 循环：LLM 自主决定工具调用和停止时机。
    @param messages 初始消息列表（含 system + user）。
    @param tools 工具实例字典。
    @param tool_defs OpenAI 格式的工具定义列表。
    @param llm LLMRequest 实例。
    @param on_message 消息回调（用于 UI 显示）。
    @param checkpoint_mgr 可选的存档管理器。
    @param problem 题目文本（用于存档元数据）。
    @return 解题结果字符串。
    """
    from utils.tools import ToolUtils

    while True:
        # 调用 LLM
        try:
            response = llm.chat_completion(
                messages=messages,
                tools=tool_defs if tool_defs else None,
                tool_choice="auto",
            )
        except Exception as error:
            logger.error("LLM 调用失败: %s", error)
            return f"LLM 调用失败: {error}"

        msg = response.choices[0].message

        # 将 assistant 消息加入历史
        messages.append(_msg_to_dict(msg))

        # 回调：显示 assistant 消息
        if on_message and msg.content:
            on_message(msg.content)

        # LLM 未调用工具 → 停止
        if not msg.tool_calls:
            break

        # 回调：显示工具调用信息
        if on_message:
            for tc in msg.tool_calls:
                on_message(f"  调用工具: {tc.function.name}")

        # 并行执行工具
        tool_results = ToolUtils.execute_tools(
            tools=tools,
            tool_calls=msg.tool_calls,
            display_message=on_message,
        )

        # 工具结果加入历史
        messages.extend(tool_results)

        # 回调：显示工具结果
        if on_message:
            for result in tool_results:
                preview = result["content"][:200]
                if len(result["content"]) > 200:
                    preview += "..."
                on_message(f"  工具结果: {preview}")

        # 保存存档
        if checkpoint_mgr:
            checkpoint_mgr.save(messages, problem=problem)

        # 上下文截断
        messages = _trim_context(messages, max_tokens=100000)

    # 从最后一条 assistant 消息提取结果
    return _extract_result(messages)


def _msg_to_dict(msg: Any) -> Dict[str, Any]:
    """将 OpenAI message 对象转为可序列化的 dict。"""
    if hasattr(msg, "model_dump"):
        return msg.model_dump()
    # 兼容旧版 SDK
    result = {"role": msg.role or "assistant"}
    if msg.content:
        result["content"] = msg.content
    if msg.tool_calls:
        result["tool_calls"] = [
            {
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.function.name,
                    "arguments": tc.function.arguments,
                },
            }
            for tc in msg.tool_calls
        ]
    return result


def _trim_context(
    messages: List[Dict[str, Any]],
    max_tokens: int = 100000,
) -> List[Dict[str, Any]]:
    """
    @brief 当消息列表过长时，截断最早的消息。
    @details 保留系统提示（第一条）+ 最近的消息。使用粗略的字符数估算 token。
    """
    # 粗略估算：1 token ≈ 2 个中文字符 或 4 个英文字符，取平均 3
    total_chars = sum(len(str(m.get("content", ""))) for m in messages)
    estimated_tokens = total_chars // 3

    if estimated_tokens <= max_tokens:
        return messages

    # 保留系统提示 + 截断早期消息
    system_msg = messages[0] if messages and messages[0].get("role") == "system" else None
    remaining = messages[1:] if system_msg else messages[:]

    # 删除最早的 20% 消息
    cut_count = max(1, len(remaining) // 5)
    remaining = remaining[cut_count:]
    logger.info("上下文截断：移除了 %d 条消息", cut_count)

    if system_msg:
        return [system_msg] + remaining
    return remaining


def _extract_result(messages: List[Dict[str, Any]]) -> str:
    """
    @brief 从消息历史中提取最终结果。
    @details 扫描最后一条 assistant 消息，查找 FLAG_FOUND 或 UNABLE_TO_SOLVE 标记。
    """
    # 从后往前找最后一条 assistant 消息
    for msg in reversed(messages):
        if msg.get("role") != "assistant":
            continue
        content = msg.get("content", "")
        if not content:
            continue

        # 检查 flag
        flag_match = re.search(r"FLAG_FOUND:\s*(.+)", content)
        if flag_match:
            return flag_match.group(1).strip()

        # 检查无法解决
        unsolvable_match = re.search(r"UNABLE_TO_SOLVE:\s*(.+)", content)
        if unsolvable_match:
            return f"未找到flag：{unsolvable_match.group(1).strip()}"

        # 有内容但没有标记，返回整个内容
        return content

    return "未找到flag：无输出"
