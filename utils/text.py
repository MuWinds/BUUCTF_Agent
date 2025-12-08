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
    """
    缩减 Prompt：
    将连续重复的空白字符（空格、换行、制表符等）压缩为单个字符。
    例如：
    - 连续的 "  " -> " "
    - 连续的 "\n\n\n" -> "\n"
    - 连续的 "\t\t" -> "\t"
    """
    # 正则表达式解释：
    # (\s) : 捕获组1，匹配任意空白字符（包括空格、\n, \t, \r 等）
    # \1+  : 引用捕获组1，匹配前面捕获到的那个字符连续出现 1 次或多次
    # r"\1": 将匹配到的整个序列替换为捕获组1的内容（即保留一个原字符）
    text = re.sub(r"(\s)\1+", r"\1", text)
    
    # 通常为了 Prompt 干净，建议最后去掉首尾的空白
    return text.strip()


