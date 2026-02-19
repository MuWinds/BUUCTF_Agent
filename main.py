from config import Config
from agent.workflow import Workflow
from agent.checkpoint import CheckpointManager
from utils.user_interface import CommandLineInterface
from ctf_platform import create_inputer, create_submitter
from datetime import datetime
import sys
import os
import logging


def setup_logging():
    """配置日志系统"""
    # 创建日志目录
    log_dir = "logs"
    os.makedirs(log_dir, exist_ok=True)
    
    # 生成带时间戳的日志文件名
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    log_file = os.path.join(log_dir, f"log_{timestamp}.log")
    
    # 配置日志
    logging.basicConfig(
        level=logging.DEBUG,
        format='%(asctime)s - %(levelname)s - %(message)s',
        handlers=[
            logging.FileHandler(log_file),
            logging.StreamHandler(sys.stdout)
        ]
    )
    
    # 设置文件日志级别为DEBUG，控制台日志级别为INFO
    file_handler = logging.FileHandler(log_file,encoding='utf-8')
    file_handler.setLevel(logging.DEBUG)
    file_handler.setFormatter(logging.Formatter('%(asctime)s - %(name)s - %(levelname)s - %(message)s'))
    
    console_handler = logging.StreamHandler()
    console_handler.setLevel(logging.INFO)
    console_handler.setFormatter(logging.Formatter('%(asctime)s - %(levelname)s - %(message)s'))
    
    # 清除所有基本配置的handler，添加自定义handler
    root_logger = logging.getLogger()
    for handler in root_logger.handlers[:]:
        root_logger.removeHandler(handler)
    
    root_logger.addHandler(file_handler)
    root_logger.addHandler(console_handler)
    
    # 设置第三方库的日志级别
    logging.getLogger("LiteLLM").setLevel(logging.WARNING)
    logging.getLogger("paramiko").setLevel(logging.WARNING)
    logging.getLogger("httpcore").setLevel(logging.WARNING)


if __name__ == "__main__":
    setup_logging()
    logger = logging.getLogger(__name__)
    config: dict = Config.load_config()

    # 创建命令行交互接口
    cli = CommandLineInterface()

    # 从配置创建输入器和提交器
    platform_config = config.get("platform", {})
    inputer = create_inputer(platform_config.get("inputer", {"type": "file"}))
    submitter = create_submitter(
        platform_config.get("submitter", {"type": "manual"}),
        user_interface=cli
    )

    # 检查是否有存档可以恢复
    checkpoint_mgr = CheckpointManager(checkpoint_dir=config.get("checkpoint_dir", "./checkpoints"))
    checkpoint_data = checkpoint_mgr.load_any()

    resume_data = None
    question_data = None
    if checkpoint_data and cli.confirm_resume():
        # 从存档恢复，直接使用存档中的题目
        question = checkpoint_data["problem"]
        resume_data = checkpoint_data
        logger.info("用户选择从存档恢复")
    else:
        # 不恢复存档，走正常流程：通过输入器获取题目
        if checkpoint_data:
            # 用户选择不恢复，删除旧存档
            checkpoint_mgr.delete(checkpoint_data["problem"])

        cli.display_message("如题目中含有附件，请放附件文件到项目根目录的attachments文件夹下")
        cli.input_question_ready("将题目文本放在Agent根目录下的question.txt回车以结束")
        question_data = inputer.fetch_question()
        question = question_data.content

    logger.debug(f"题目内容：{question}")

    # 创建Workflow实例并传入用户接口和平台组件
    workflow = Workflow(
        config=config,
        user_interface=cli,
        inputer=inputer,
        submitter=submitter
    )
    result = workflow.solve(question, resume_data=resume_data, question=question_data)

    logger.info(f"最终结果:{result}")
