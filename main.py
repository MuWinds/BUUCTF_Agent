from config import Config
from agent.workflow import Workflow
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
    print("如题目中含有附件，请放附件文件到项目根目录的attachments文件夹下")
    input("将题目文本放在Agent根目录下的question.txt回车以结束")
    question = open("question.txt", "r", encoding="utf-8").read()
    logger.debug(f"题目内容：{question}")
    result = Workflow(config=config).solve(question)
    logger.info(f"最终结果:{result}")
