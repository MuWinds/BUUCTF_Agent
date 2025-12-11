# BUUCTF_Agent
![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/1.png)

![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/tch_logo.png)
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

本节提供在类 Unix（Linux / WSL / 容器）环境下的推荐部署方式，并补充 WebUI 的启动方式说明。该项目在原生 Windows 环境下运行会遇到不便，强烈建议使用 Linux/WSL 或 Docker。

先决条件
- Python 3.8+（推荐 3.10+）
- 建议使用虚拟环境（venv）或容器
- 若使用 Docker，请预先安装 Docker

### 1) 克隆仓库

```bash
git clone https://github.com/MuWinds/BUUCTF_Agent.git
cd BUUCTF_Agent
```

### 2) 在 Linux / WSL 下本地运行（用于开发与调试）

#### 2.1 创建并激活虚拟环境，安装依赖

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
```

#### 2.2 初始化并编辑配置文件 `config.json`

```bash
cp config_template.json config.json
# 编辑 config.json，填写 llm、tool_config（例如 ssh_shell 的 host/port/username/password）
```

> 注意：程序运行时会通过 `config.Config.load_config("./config.json")` 读取配置，并自动在 `llm` 下的所有 `model` 字段前加上 `openai/` 前缀（如果你未手动带前缀），因此 `model` 值可以直接写成 `deepseek-ai/DeepSeek-R1` 等兼容 OpenAI 接口的模型名。

#### 2.3 命令行模式运行（手动输入题面）

命令行入口为 `main.py`，逻辑为：
- 从项目根目录的 `config.json` 读取配置；
- 提示你将题面写入 `question.txt`；
- 读取 `question.txt` 的内容并调用 Agent 进行全流程解题；
- 如题目有附件，请提前放入项目根目录下的 `attachments/` 目录。

在项目根目录执行：

```bash
python3 main.py
```

#### 2.4 启动 Web UI

项目提供了 FastAPI + 前端静态页面的 Web 控制台，入口在 `webui/server.py` 中的 `app` 变量：

```bash
uvicorn webui.server:app --reload --host 0.0.0.0 --port 8000
```

- 访问路径：`http://<服务器IP>:8000/`
- 静态资源路径：`/static`（由 `webui/static` 目录提供）
- WebUI 中可以：
    - 在线查看与编辑当前生效的 `config.json`（通过 WebUI 内的配置管理接口）；
    - 上传/删除附件（保存到项目根目录的 `attachments/` 目录，与命令行模式复用）；
    - 启动/终止 Agent 会话，并实时查看日志事件流；
    - 对模型推理出的 flag 进行人工确认后再提交。

> 如果在 WSL 中运行，请使用 `--host 0.0.0.0`，以便宿主 Windows 访问 Web UI。

### 3) 使用 Docker（推荐用于隔离运行）

#### 3.1 构建镜像

```bash
docker build -t ctf_agent .
```

#### 3.2 运行容器并映射 Web UI 与 SSH（根据需要调整端口）

```bash
docker run -itd -p 8000:8000 -p 2201:22 --name ctf_agent ctf_agent
```

- Web UI 将暴露在宿主机 `http://localhost:8000/`
- 若镜像中启用 SSH 服务，则示例中 SSH 端口映射到宿主机的 `2201`；
- 若使用仓库内的 Dockerfile，默认 SSH 用户可能为 `root`，密码 `ctfagent`（以实际镜像配置为准）。

#### 3.3 挂载配置与附件（可选但推荐）

实际使用中通常希望持久化 `config.json` 与 `attachments/`，可以通过挂载宿主目录实现，例如：

```bash
docker run -itd \
    -p 8000:8000 -p 2201:22 \
    -v $(pwd)/config.json:/app/config.json \
    -v $(pwd)/attachments:/app/attachments \
    --name ctf_agent ctf_agent
```

> 挂载路径请根据 Dockerfile 中的工作目录调整（如果你更改了默认工作目录 `/app`）。

### 4) 核心配置说明

- `config.json` 的关键字段：
    - `llm`：为不同任务（`analyzer` / `solve_agent` / `pre_processor`）配置模型与 API
    - `tool_config`：各工具的运行参数（例如 `ssh_shell` 的 `host` / `port` / `username` / `password`）
    - `mcp_server`：可选的 MCP 服务配置

- 示例（请替换为你自己的 API 与凭证）：

```json
 {
     "llm":{
         "analyzer":{
             "model": "deepseek-ai/DeepSeek-R1",
             "api_key": "",
             "api_base": "https://api.siliconflow.cn/"
         },
         "solve_agent":{
             "model": "deepseek-ai/DeepSeek-V3",
             "api_key": "",
             "api_base": "https://api.siliconflow.cn/"
         },
         "pre_processor":{
             "model": "Qwen/Qwen3-8B",
             "api_key": "",
             "api_base": "https://api.siliconflow.cn/"
         }
     },
     "max_history_steps": 15,
     "compression_threshold": 7,
     "tool_config":{
         "ssh_shell": 
         {
             "host": "127.0.0.1",
             "port": 22,
             "username": "",
             "password": ""
         },
         "python":
         {
         }
     },
     "mcp_server": {
         "hexstrike": {
             "type": "stdio",
             "command": "python3",
             "args": [
                 "/root/hexstrike-ai/hexstrike_mcp.py",
                 "--server",
                 "http://localhost:8888"
             ]
         }
     }
 }
```

注意：本项目目前仅兼容 OpenAI API 类型（或与 OpenAI 兼容的 API）的大模型接口。

5) 运行与故障排查要点

- 若模块缺失或安装失败，确认虚拟环境已激活并重新运行 `pip install -r requirements.txt`。
- 若 Web UI 无法访问，检查防火墙与端口映射（容器运行时用 `docker ps` 和 `docker logs <name>` 查看）。
- 容器日志查看：

```bash
docker logs ctf_agent
```

- 当使用远程 SSH 工具时，确保 `tool_config.ssh_shell` 中的 host/port/credential 正确且目标允许连接。

6) 安全提示

- 该项目能在被配置的工具上执行任意 Shell/代码，请勿在含有重要数据或生产环境的机器上无充分隔离地运行 Agent。
- 对外提供 Web UI 或开放端口时请做好访问控制与凭证管理。



## 目前计划
- ~~允许用户本地环境运行Python代码~~（已完成）
- 支持更多工具，比如二进制分析等，不局限于Web题和Web相关的密码学之类的
- ~~提供更美观的界面，比如Web前端或者Qt界面~~（已完成）
- RAG知识库
- ~~将不同工具的LLM进行区分，或者按照思考推理与代码指令编写两种任务分派到不同的LLM~~（已完成）
- ~~更好的MCP支持~~（已完成✅）
- 实现不同OJ平台的自动化，提供手动输入题面之外更便捷的选择
- ~~支持附件输入~~已实现，需要在项目根目录的attachments目录下放入附件

## 工具开发
**目前项目已经内置支持Python工具和SSH到装好环境的Linux机器进行解题**，如果还需要开发自己顺手的工具可以看这里

在项目的ctf_tool文件夹下，有base_tool.py:
```python
class BaseTool(ABC):
    @abstractmethod
    def execute(self, *args, **kwargs) -> Tuple[str, str]:
        """执行工具操作"""
        pass
    
    @property
    @abstractmethod
    def function_config(self) -> Dict:
        """返回工具的函数调用配置"""
        pass
```
提供了抽象方法实例，其中必须包含`execute`和`function_config`这两个方法。

* `execute`方法是提供直接执行的操作，返回的是执行的结果，为元组类型，元组的两个元素一个是正常输出，一个是报错输出，对顺序没有要求。

* `function_config`方法是提供function call的配置通过Agent传给大语言模型，方便大语言模型决定什么情况下调用这个工具，该方法必须添加`@property`注解，且返回的格式相对固定，下面是一个远程执行shell的示例：
```python
@property
def function_config(self) -> Dict:
    return {
        "type": "function",
        "function": {
            "name": "execute_shell_command",
            "description": "在远程服务器上执行Shell命令，服务器内提供了curl,sqlmap,nmap,openssl等常用工具",
            "parameters": {
                "type": "object",
                "properties": {
                    "purpose": {
                        "type": "string",
                        "description": "执行此步骤的目的"
                    },
                    "content": {
                        "type": "string",
                        "description": "要执行的shell命令"
                    },
                "required": ["purpose", "content"]
                }
            }
        }
    }
```

## Attention
既然有shell代码的执行，**请不要作死拿自己存着重要数据的机器让Agent执行代码**，我不确保大语言模型一定不会输出诸如`rm -rf /*`这种奇怪东西，因为这种操作出现的各种问题请自行认命，项目仓库给了对应的Dockerfile方便各位拿来就用。

QQ群：

![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/qq_group.jpg)
