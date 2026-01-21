# BUUCTF_Agent
![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/1.png)

[![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/tch_logo.png)](https://zc.tencent.com/competition/competitionHackathon?code=cha004)
## 背景

起源于[@MuWinds](https://github.com/MuWinds)闲来无事，所以打算写个Ai Agent练手

项目并不打算局限于[BUUCTF](https://buuoj.cn)，所以现在是手动输入题面的（更主要是我懒）。

愿景：成为各路CTF大手子的好伙伴，当然如果Agent能独当一面的话那最好不过~

## 功能

1. 支持全自动解题，包括题目分析，靶机探索，代码执行，flag分析全流程
2. 支持命令行交互式解题
3. 目前项目内置支持Python工具和SSH到装好环境的Linux机器进行解题
4. 可扩展的CTF工具框架
5. 可自定义的Prompt和模型文件
6. 提供实时可视化的Web控制台，支持配置编辑与任务终止

## 部署与运行

先决条件
- Python 3.8+（推荐 3.10+）
- 建议使用虚拟环境（venv）或容器
- 若使用 Docker，请预先安装 Docker

### 1) 克隆仓库

```bash
git clone https://github.com/MuWinds/BUUCTF_Agent.git
cd BUUCTF_Agent
```

### 2) 配置工具环境

让 Agent 解题和让人解题类似，都需要提供解题的工具供 Agent 调用才能够解题，目前 Agent 自带一个 SSH 连接到指定服务器的工具和 Python 执行的工具，你可以直接调用他们，也可以根据下面的抽象方法来开发自己的工具，同时还可以根据下面的配置文件来配置多个 MCP 增加 Agent 解题的能力。

```python
# -*- coding: utf-8 -*-
# 工具基类
from abc import ABC, abstractmethod
from typing import Dict
import logging

logger = logging.getLogger(__name__)


class BaseTool(ABC):
    @abstractmethod
    def execute(self, *args, **kwargs) -> str:
        """执行工具操作"""
        pass

    @property
    @abstractmethod
    def function_config(self) -> Dict:
        """返回工具的函数调用配置"""
        pass
```

### 3) 配置文件说明

- `config.json` 的关键字段：
    - `llm`：为不同任务（`analyzer` / `solve_agent` / `pre_processor`）配置模型与 API
    - `tool_config`：各工具的运行参数（例如 `ssh_shell` 的 `host` / `port` / `username` / `password`）
    - `mcp_server`：可选的 MCP 服务配置

- 示例（请替换为你自己的 API 与凭证）：

```json
{
    "llm":{
        "analyzer":{
            "model": "deepseek-ai/DeepSeek-V3.2",
            "api_key": "your-api-key",
            "api_base": "https://api.siliconflow.cn/"
        },
        "solve_agent":{
            "model": "deepseek-ai/DeepSeek-V3.2",
            "api_key": "your-api-key",
            "api_base": "https://api.siliconflow.cn/"
        },
        "embedding":{
            "model": "BAAI/bge-m3",
            "api_key": "your-api-key",
            "api_base": "https://api.siliconflow.cn/v1"
        },
        "pre_processor":{
            "model": "THUDM/glm-4-9b-chat",
            "api_key": "your-api-key",
            "api_base": "https://api.siliconflow.cn/"
        }
    },
    "max_history_steps": 10,
    "compression_threshold": 5,
    "persist_directory": "./rag_db",
    "tool_config":{
        "ssh_shell": 
        {
            "host": "172.25.231.142",
            "port": 2201,
            "username": "root",
            "password": "password"
        },
        "python":
        {
            "remote": true
        }
    },
    "mcp_server": {

    }
}
```

注意：本项目目前仅兼容 OpenAI API 类型（或与 OpenAI 兼容的 API）的大模型接口。


### 4) 运行

目前Agent跑起来有两种方式，一种是在项目根目录下直接运行 main.py 文件来进行解题：
```bash
python main.py
```
还可以通过Web来进行解题，Web目前做的相对简陋
```bash
python webui/app.py
```
默认运行在本地5000端口，在浏览器打开 http://127.0.0.1:5000 即可


## 目前计划
- ~~允许用户本地环境运行Python代码~~（已完成）
- 支持更多工具，比如二进制分析等，不局限于Web题和Web相关的密码学之类的
- ~~提供更美观的界面，比如Web前端或者Qt界面~~（已完成）
- ~~RAG知识库~~(已完成)
- ~~将不同工具的LLM进行区分，或者按照思考推理与代码指令编写两种任务分派到不同的LLM~~（已完成）
- ~~更好的MCP支持~~（已完成✅）
- 实现不同OJ平台的自动化，提供手动输入题面之外更便捷的选择
- ~~支持附件输入~~已实现，需要在项目根目录的attachments目录下放入附件


QQ群：

![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/qq_group.jpg)
