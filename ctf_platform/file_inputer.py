"""@brief 提供基于本地文件的题目输入器实现。"""

import os

from ctf_platform.base import Question, QuestionInputer
from ctf_platform.registry import register_inputer


@register_inputer("file")
class FileQuestionInputer(QuestionInputer):
    """@brief 从本地 question 文件读取题目内容。"""

    def __init__(
        self,
        file_path: str = "./question.txt",
        attachment_dir: str = "./attachments",
    ):
        """@brief 初始化文件输入器。

        @param file_path 题目文本文件路径。
        @param attachment_dir 附件目录路径。
        """
        self.file_path = file_path
        self.attachment_dir = attachment_dir

    def fetch_question(self) -> Question:
        """@brief 读取题目文本与附件列表。

        @return Question 题目对象。
        """
        with open(self.file_path, "r", encoding="utf-8") as file:
            content = file.read()

        attachments = []
        try:
            if os.path.isdir(self.attachment_dir):
                attachments = [
                    os.path.join(self.attachment_dir, file_name)
                    for file_name in os.listdir(self.attachment_dir)
                    if os.path.isfile(os.path.join(self.attachment_dir, file_name))
                ]
        except Exception as error:
            print(f"Error occurred while fetching attachments: {error}")

        return Question(
            title="",
            content=content,
            attachments=attachments,
        )
