import os
from ctf_platform.base import QuestionInputer, Question
from ctf_platform.registry import register_inputer


@register_inputer("file")
class FileQuestionInputer(QuestionInputer):
    """从question.txt文件读取题目（兼容现有行为）"""

    def __init__(self, file_path: str = "./question.txt",
                 attachment_dir: str = "./attachments"):
        self.file_path = file_path
        self.attachment_dir = attachment_dir

    def fetch_question(self) -> Question:
        content = open(self.file_path, "r", encoding="utf-8").read()

        attachments = []
        if os.path.isdir(self.attachment_dir):
            attachments = [
                os.path.join(self.attachment_dir, f)
                for f in os.listdir(self.attachment_dir)
                if os.path.isfile(os.path.join(self.attachment_dir, f))
            ]

        return Question(
            title="",
            content=content,
            attachments=attachments
        )
