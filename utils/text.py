import json
import logging
import re

from json_repair import repair_json

logger = logging.getLogger(__name__)


def fix_json_with_llm(json_str: str, err_content: str) -> str:
    """
    @brief 使用 LLM 修复格式错误的 JSON 字符串。
    @param json_str 格式错误的 JSON 字符串。
    @param err_content 解析时的错误信息。
    @return 修复后的有效 JSON 字符串。
    """
    from utils.llm_request import LLMRequest

    prompt = (
        "以下是一个格式错误的JSON字符串，请修复它使其成为有效的JSON。"
        "只返回修复后的JSON，不要包含任何其他内容。"
        "确保保留所有原始键值对，不要改动里面的内容\n\n"
        f"错误JSON: {json_str}"
        f"错误信息: {err_content}"
    )
    pre_processor = LLMRequest("solve_agent")

    while True:
        try:
            repaired_json = repair_json(json_str)
            if json.loads(repaired_json):
                return repaired_json
        except Exception:
            response = pre_processor.text_completion(prompt, True)
            json_str = response.choices[0].message.content
            continue


def optimize_text(text: str) -> str:
    """
    @brief 缩减 Prompt 中的重复空白字符。

    @details
    将连续重复的同类空白字符（空格、换行、制表符等）压缩为单个字符，
    并移除首尾空白。

    @param text 待优化文本。
    @return 优化后的文本。
    """
    text = re.sub(r"(\s)\1+", r"\1", text)
    return text.strip()
