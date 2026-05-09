import re


def optimize_text(text: str) -> str:
    """
    @brief 缩减 Prompt 中的重复空白字符。
    @param text 待优化文本。
    @return 优化后的文本。
    """
    text = re.sub(r"(\s)\1+", r"\1", text)
    return text.strip()
