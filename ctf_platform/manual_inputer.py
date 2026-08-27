"""提供手动输入的题目输入器实现。"""

import os
from typing import List

from ctf_platform.base import Question, QuestionInputer
from ctf_platform.registry import register_inputer
from utils.user_interface import UserInterface


@register_inputer("manual")
class ManualQuestionInputer(QuestionInputer):
    """通过用户手动输入获取题目内容。"""

    def __init__(
        self,
        user_interface: UserInterface,
        attachment_dir: str = "./attachments",
    ):
        """初始化手动输入器。

        Args:
            user_interface: 用户交互接口实例。
            attachment_dir: 附件目录路径。
        """
        self.user_interface = user_interface
        self.attachment_dir = attachment_dir

    def fetch_question(self) -> Question:
        """通过用户多行输入获取题目。

        Returns:
            题目对象。
        """
        content = self.user_interface.input_question()

        attachments: List[str] = []
        try:
            if os.path.isdir(self.attachment_dir):
                attachments = [
                    os.path.join(self.attachment_dir, file_name)
                    for file_name in os.listdir(self.attachment_dir)
                    if os.path.isfile(
                        os.path.join(self.attachment_dir, file_name)
                    )
                ]
        except Exception as error:
            print(f"获取附件出错: {error}")

        return Question(
            title="",
            content=content,
            attachments=attachments,
        )
