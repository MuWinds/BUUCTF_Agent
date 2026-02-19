# 题目输入器与Flag提交器 抽象设计

## 1. 现状分析

当前项目中，题目获取和flag提交的逻辑散落在多处且与具体实现强耦合：

- **题目获取**：`main.py` 中硬编码读取 `question.txt` 文件；`webui/app.py` 中通过 POST 请求获取
- **Flag提交**：仅有"用户确认flag是否正确"的流程（`UserInterface.confirm_flag`），没有自动提交到平台的能力
- **问题**：用户无法自定义题目来源（如从BUUCTF/CTFHub等平台自动拉取），也无法自定义flag提交方式（如自动提交到平台验证）

## 2. 设计目标

将"题目输入"和"flag提交"抽象为可插拔的接口，用户可以：

1. 自定义题目来源（手动输入、从文件读取、从平台API拉取等）
2. 自定义flag提交方式（手动确认、自动提交到平台验证等）
3. 组合使用不同的输入器和提交器
4. 通过配置文件选择使用哪种实现

## 3. 接口设计

### 3.1 QuestionInputer — 题目输入器

```python
# ctf_platform/base.py

from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Optional, List


@dataclass
class Question:
    """题目数据结构"""
    title: str                          # 题目标题
    content: str                        # 题目正文
    category: Optional[str] = None      # 题目分类（Web/Pwn/Crypto/Misc等）
    attachments: Optional[List[str]] = None  # 附件文件路径列表
    url: Optional[str] = None           # 靶机地址（如有）
    metadata: Optional[dict] = None     # 额外元数据（平台题目ID等）


class QuestionInputer(ABC):
    """题目输入器抽象基类"""

    @abstractmethod
    def fetch_question(self) -> Question:
        """获取一道题目

        Returns:
            Question: 题目数据
        """
        pass

    def list_questions(self) -> List[Question]:
        """列出可用题目（可选实现，用于平台批量拉取场景）

        Returns:
            List[Question]: 题目列表
        """
        raise NotImplementedError("该输入器不支持列出题目")
```

### 3.2 FlagSubmitter — Flag提交器

```python
# ctf_platform/base.py

@dataclass
class SubmitResult:
    """提交结果数据结构"""
    success: bool           # 是否正确
    message: str = ""       # 平台返回的消息或说明


class FlagSubmitter(ABC):
    """Flag提交器抽象基类"""

    @abstractmethod
    def submit(self, flag: str, question: Question) -> SubmitResult:
        """提交flag

        Args:
            flag: 待提交的flag字符串
            question: 对应的题目（用于定位平台上的题目）

        Returns:
            SubmitResult: 提交结果
        """
        pass
```

## 4. 内置实现

### 4.1 FileQuestionInputer — 文件输入器（兼容现有逻辑）

```python
# ctf_platform/file_inputer.py

class FileQuestionInputer(QuestionInputer):
    """从question.txt文件读取题目（兼容现有行为）"""

    def __init__(self, file_path: str = "./question.txt",
                 attachment_dir: str = "./attachments"):
        self.file_path = file_path
        self.attachment_dir = attachment_dir

    def fetch_question(self) -> Question:
        content = open(self.file_path, "r", encoding="utf-8").read()

        # 扫描附件目录
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
```

### 4.2 ManualFlagSubmitter — 手动确认提交器（兼容现有逻辑）

```python
# ctf_platform/manual_submitter.py

class ManualFlagSubmitter(FlagSubmitter):
    """通过用户手动确认来验证flag（兼容现有行为）"""

    def __init__(self, user_interface: UserInterface):
        self.user_interface = user_interface

    def submit(self, flag: str, question: Question) -> SubmitResult:
        confirmed = self.user_interface.confirm_flag(flag)
        return SubmitResult(
            success=confirmed,
            message="用户确认正确" if confirmed else "用户确认不正确"
        )
```

### 4.3 用户自定义示例 — BUUCTF平台实现

```python
# ctf_platform/buuctf.py（示例，用户自行实现）

import requests

class BUUCTFInputer(QuestionInputer):
    """从BUUCTF平台拉取题目"""

    def __init__(self, cookie: str):
        self.session = requests.Session()
        self.session.headers["Cookie"] = cookie
        self.base_url = "https://buuoj.cn"

    def fetch_question(self) -> Question:
        # 用户根据BUUCTF的实际接口实现
        resp = self.session.get(f"{self.base_url}/api/challenge/...")
        data = resp.json()
        return Question(
            title=data["title"],
            content=data["description"],
            category=data.get("category"),
            url=data.get("url"),
            metadata={"challenge_id": data["id"]}
        )

    def list_questions(self) -> List[Question]:
        # 拉取题目列表
        ...


class BUUCTFSubmitter(FlagSubmitter):
    """自动提交flag到BUUCTF平台"""

    def __init__(self, cookie: str):
        self.session = requests.Session()
        self.session.headers["Cookie"] = cookie
        self.base_url = "https://buuoj.cn"

    def submit(self, flag: str, question: Question) -> SubmitResult:
        challenge_id = question.metadata.get("challenge_id")
        resp = self.session.post(
            f"{self.base_url}/api/flag/submit",
            json={"challenge_id": challenge_id, "flag": flag}
        )
        data = resp.json()
        return SubmitResult(
            success=data.get("correct", False),
            message=data.get("message", "")
        )
```

## 5. 集成方式

### 5.1 配置文件扩展

在 `config.json` 中新增 `platform` 配置段：

```json
{
    "platform": {
        "inputer": {
            "type": "file",
            "file_path": "./question.txt",
            "attachment_dir": "./attachments"
        },
        "submitter": {
            "type": "manual"
        }
    }
}
```

用户自定义平台时的配置示例：

```json
{
    "platform": {
        "inputer": {
            "type": "buuctf",
            "cookie": "your_cookie_here"
        },
        "submitter": {
            "type": "buuctf",
            "cookie": "your_cookie_here"
        }
    }
}
```

### 5.2 工厂注册机制

```python
# ctf_platform/registry.py

# 内置实现注册表
_inputer_registry: Dict[str, Type[QuestionInputer]] = {}
_submitter_registry: Dict[str, Type[FlagSubmitter]] = {}


def register_inputer(name: str):
    """装饰器：注册题目输入器"""
    def decorator(cls):
        _inputer_registry[name] = cls
        return cls
    return decorator


def register_submitter(name: str):
    """装饰器：注册flag提交器"""
    def decorator(cls):
        _submitter_registry[name] = cls
        return cls
    return decorator


def create_inputer(config: dict) -> QuestionInputer:
    """根据配置创建输入器实例"""
    type_name = config.get("type", "file")
    cls = _inputer_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的输入器类型: {type_name}")
    # 将config中除type外的字段作为构造参数
    params = {k: v for k, v in config.items() if k != "type"}
    return cls(**params)


def create_submitter(config: dict, user_interface=None) -> FlagSubmitter:
    """根据配置创建提交器实例"""
    type_name = config.get("type", "manual")
    cls = _submitter_registry.get(type_name)
    if cls is None:
        raise ValueError(f"未知的提交器类型: {type_name}")
    params = {k: v for k, v in config.items() if k != "type"}
    if type_name == "manual":
        params["user_interface"] = user_interface
    return cls(**params)
```

### 5.3 Workflow 改造

```python
# agent/workflow.py 改造要点

class Workflow:
    def __init__(self, config: dict, user_interface=None,
                 inputer: QuestionInputer = None,
                 submitter: FlagSubmitter = None):
        self.config = config
        self.user_interface = user_interface or CommandLineInterface()

        # 通过参数传入或从配置创建
        platform_config = config.get("platform", {})
        self.inputer = inputer or create_inputer(
            platform_config.get("inputer", {"type": "file"})
        )
        self.submitter = submitter or create_submitter(
            platform_config.get("submitter", {"type": "manual"}),
            user_interface=self.user_interface
        )

    def confirm_flag(self, flag_candidate: str) -> bool:
        """使用submitter提交flag并返回结果"""
        result = self.submitter.submit(flag_candidate, self.current_question)
        return result.success
```

### 5.4 main.py 改造

```python
# main.py 改造要点

if __name__ == "__main__":
    config = Config.load_config()
    cli = CommandLineInterface()

    # 创建输入器和提交器
    inputer = create_inputer(config.get("platform", {}).get("inputer", {"type": "file"}))
    submitter = create_submitter(
        config.get("platform", {}).get("submitter", {"type": "manual"}),
        user_interface=cli
    )

    # 获取题目
    question_data = inputer.fetch_question()

    # 创建Workflow
    workflow = Workflow(
        config=config,
        user_interface=cli,
        inputer=inputer,
        submitter=submitter
    )
    result = workflow.solve(question_data.content)
```

## 6. 文件结构

新增的文件组织：

```
ctf_platform/
├── __init__.py
├── base.py              # Question, SubmitResult, QuestionInputer, FlagSubmitter
├── registry.py          # 注册表和工厂函数
├── file_inputer.py      # FileQuestionInputer（内置）
└── manual_submitter.py  # ManualFlagSubmitter（内置）
```

用户自定义实现只需：
1. 创建一个 Python 文件，继承 `QuestionInputer` 或 `FlagSubmitter`
2. 使用 `@register_inputer("名称")` / `@register_submitter("名称")` 注册
3. 在 `config.json` 的 `platform` 段指定 `type` 为注册名称

## 7. 向后兼容

- 不配置 `platform` 段时，默认使用 `FileQuestionInputer` + `ManualFlagSubmitter`，行为与现有完全一致
- `UserInterface.confirm_flag` 方法保留，`ManualFlagSubmitter` 内部调用它
- WebUI 的 `WebInterface` 同样可以通过 `ManualFlagSubmitter` 无缝工作
