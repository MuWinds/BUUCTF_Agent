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

## 部署与运行

先决条件
- Python 3.10+
- Conda

### 1) 克隆仓库

```bash
git clone https://github.com/MuWinds/BUUCTF_Agent.git
cd BUUCTF_Agent
```

### 2) 创建并激活 conda 环境（ctf-agent）

```bash
conda create -n ctf-agent python=3.10 -y
conda activate ctf-agent
```

### 3) 安装依赖

```bash
pip install -r requirements.txt
```

### 4) 配置 OpenAI API

1. 复制配置模板：

```bash
cp config_template.json config.json
```

2. 编辑 `config.json`，填入你的 API Key。

示例（请替换为你自己的凭证）：

```json
{
    "llm": {
        "model": "gpt-4o-mini",
        "api_key": "your-api-key",
        "api_base": "https://api.openai.com/v1"
    }
}
```

注意：本项目仅对接 OpenAI API（或 OpenAI 兼容接口）。

### 5) 运行

命令行模式：

```bash
python main.py
```

## 目前计划
- ~~允许用户本地环境运行Python代码~~（已完成）
- 支持更多工具，比如二进制分析等，不局限于Web题和Web相关的密码学之类的
- ~~提供更美观的界面，比如Web前端或者Qt界面~~（已完成）
- ~~将不同工具的LLM进行区分，或者按照思考推理与代码指令编写两种任务分派到不同的LLM~~（已完成）
- ~~更好的MCP支持~~（已完成✅）
- 实现不同OJ平台的自动化，提供手动输入题面之外更便捷的选择
- ~~支持附件输入~~已实现，需要在项目根目录的attachments目录下放入附件


QQ群：

![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/qq_group.jpg)
