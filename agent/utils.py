import litellm
import re
import json


def fix_json_with_llm(json_str: str, err_content: str, config: dict) -> dict:
    """
    使用LLM修复格式错误的JSON
    :param json_str: 格式错误的JSON字符串
    :param config: LLM配置
    :return: 修复后的字典
    """
    litellm.enable_json_schema_validation = True
    prompt = (
        "以下是一个格式错误的JSON字符串，请修复它使其成为有效的JSON。"
        "只返回修复后的JSON，不要包含任何其他内容。"
        "确保保留所有原始键值对，不要改动里面的内容\n\n"
        f"错误JSON: {json_str}"
        f"错误信息: {err_content}"
    )

    while True:
        response = litellm.completion(
            model=config["model"],
            api_key=config["api_key"],
            api_base=config["api_base"],
            messages=[{"role": "user", "content": prompt}],
        )
        fixed_json = response.choices[0].message.content
        try:
            json.loads(fixed_json)
            return fixed_json
        except:
            continue


def optimize_text(text: str) -> str:
    # 把连续 2 个及以上空格 → 先统一替换成一个特殊标记
    text = re.sub(r" {2,}", "\x00", text)  # \x00 几乎不会出现在正文里
    # 把特殊标记还原成唯一一个空格
    text = text.replace("\x00", " ")
    text = re.sub(r"\n+", "\n", text)
    lines = [line for line in text.splitlines() if line.strip()]
    return "\n".join(lines)
