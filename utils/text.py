import re
import json
import logging
from json_repair import repair_json


logger = logging.getLogger(__name__)


def fix_json_with_llm(json_str: str, err_content: str) -> str:
    """
    使用LLM修复格式错误的JSON
    :param json_str: 格式错误的JSON字符串
    :param config: LLM配置
    :return: 修复后的字典
    """
    from utils.llm_request import LLMRequest
    prompt = (
        "以下是一个格式错误的JSON字符串，请修复它使其成为有效的JSON。"
        "只返回修复后的JSON，不要包含任何其他内容。"
        "确保保留所有原始键值对，不要改动里面的内容\n\n"
        f"错误JSON: {json_str}"
        f"错误信息: {err_content}"
    )
    pre_processor = LLMRequest("pre_processor")
    while True:
        try:
            if json.loads(repair_json(json_str)):
                return repair_json(json_str)
        except:
            response = pre_processor.text_completion(prompt, True)
            json_str = response.choices[0].message.content
            continue


def optimize_text(text: str) -> str:
    # 把连续 2 个及以上空格 → 先统一替换成一个特殊标记
    text = re.sub(r" {2,}", "\x00", text)  # \x00 几乎不会出现在正文里
    # 把特殊标记还原成唯一一个空格
    text = text.replace("\x00", " ")
    text = re.sub(r"\n+", "\n", text)
    lines = [line for line in text.splitlines() if line.strip()]
    return "\n".join(lines)


