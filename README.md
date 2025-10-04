# BUUCTF_Agent
![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/1.png)
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

1. 克隆仓库
```
git clone https://github.com/MuWinds/BUUCTF_Agent.git
```
2. 安装依赖
```
pip install -r .\requirements.txt
```
3. Docker容器配置（可选）：这一步是配置Agent的执行环境，可以自己配虚拟机也可以用仓库里现成的Dockerfile，如果用docker的方式配置请提前安装好Docker   
   (1)先制作镜像:
   ```bash
   docker build -t ctf_agent .
   ```
   (2)再运行镜像，将镜像内ssh所用的22端口映射到宿主机的2201端口：
   ```bash
   docker run -itd -p 2201:22 ctf_agent
   ```
   如果用仓库里Dockerfile去创建Docker容器，SSH用户为root，密码为ctfagent。
4. 修改配置文件：config.json，修改工具的配置文件
   下面是是硅基流动API（OpenAI兼容模式的配置示例：）
   ```json
   {
    "model": "openai/deepseek-ai/DeepSeek-V3.1-Terminus",
    "api_key": "",
    "api_base": "https://api.siliconflow.cn/v1",
    "tool_config":{
        "ssh_shell": 
        {
            "host": "127.0.0.1",
            "port": 2201,
            "username": "root",
            "password": ""
        },
        "python":
        {
        }
    }
   }
   ```
   本项目采用litellm与大模型进行对接，因此，如果要使用openai兼容的api模型，需要在model值前加openai/，即原来是`deepseek-ai/DeepSeek-V3.1-Terminus`，需要改成`openai/deepseek-ai/DeepSeek-V3.1-Terminus`
5. 运行：
```
python .\main.py
```


## 目前计划
- 允许用户本地环境运行Python代码
- 支持更多工具，比如二进制分析等，不局限于Web题和Web相关的密码学之类的
- 提供更美观的界面，比如Web前端或者Qt界面
- RAG知识库
- 将不同工具的LLM进行区分，或者按照思考推理与代码指令编写两种任务分派到不同的LLM
- 实现不同OJ平台的自动化，提供手动输入题面之外更便捷的选择

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
```

## Attention
既然有shell代码的执行，**请不要作死拿自己存着重要数据的机器让Agent执行代码**，我不确保大语言模型一定不会输出诸如`rm -rf /*`这种奇怪东西，因为这种操作出现的各种问题请自行认命，项目仓库给了对应的Dockerfile方便各位拿来就用。

QQ群：
![image](https://github.com/MuWinds/BUUCTF_Agent/blob/main/qq_group.png)
