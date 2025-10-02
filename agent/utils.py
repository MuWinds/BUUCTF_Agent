import litellm

def fix_json_with_llm(json_str: str, config: dict) -> dict:
    """
    使用LLM修复格式错误的JSON
    :param json_str: 格式错误的JSON字符串
    :param config: LLM配置
    :return: 修复后的字典
    """
    prompt = (
        "以下是一个格式错误的JSON字符串，请修复它使其成为有效的JSON。"
        "只返回修复后的JSON，不要包含任何其他内容。"
        "确保保留所有原始键值对，不要改动里面的内容\n\n"
        f"错误JSON: {json_str}"
    )
    
    try:
        response = litellm.completion(
            model=config["model"],
            api_key=config["api_key"],
            api_base=config["api_base"],
            messages=[{"role": "user", "content": prompt}],
        )
        fixed_json = response.choices[0].message.content.strip()
        return fixed_json
    except Exception:
        return {}