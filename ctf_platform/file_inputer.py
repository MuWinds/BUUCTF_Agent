"""提供基于本地文件的题目输入器实现。"""

from ctf_platform.base import Question, QuestionInputer
from ctf_platform.registry import register_inputer


@register_inputer("file")
class FileQuestionInputer(QuestionInputer):
    """从本地 question 文件读取题目内容。"""

    def __init__(
        self,
        file_path: str = "./question.txt",
    ):
        """初始化文件输入器。

        Args:
            file_path: 题目文本文件路径。
        """
        self.file_path = file_path

    def fetch_question(self) -> Question:
        """读取题目文本。

        Returns:
            题目对象。
        """
        with open(self.file_path, "r", encoding="utf-8") as file:
            content = file.read()

        return Question(
            title="",
            content=content,
        )
