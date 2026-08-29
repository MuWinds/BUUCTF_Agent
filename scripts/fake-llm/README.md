# fake-llm

OpenAI 兼容协议的假服务端，给功能测试提供确定性的真实数据。

同样的输入永远得到同样的事件序列 —— 既被 `src-tauri/tests/e2e.rs` 断言，
也供人对着 GUI 肉眼核对（见 `docs/manual-gui-checklist.md`）。

```
server.mjs             # 服务端，零依赖，只用 node:http
fixtures/*.json        # 场景数据：每个文件一个场景
sandbox/               # 工具操作的工作区数据
config.template.json   # 录制模式的配置模板（提交）
config.json            # 你的真实密钥（已 gitignore，不会被提交）
```

服务端有两种模式：**回放**（默认，不需要任何密钥）和**录制**（连真实 LLM，
把它的回答抓成 fixture）。

## 回放

```bash
pnpm fake-llm                              # 监听 127.0.0.1:8787
node scripts/fake-llm/server.mjs --port 0  # 随机端口，就绪后打印到 stdout
curl http://127.0.0.1:8787/scenarios       # 列出全部场景
```

应用侧把 API 地址填 `http://127.0.0.1:8787/v1`，Key 留空，工作区指向 `sandbox/`。

### 场景选择

按优先级：

1. 请求里的 `model` 恰好等于某个场景 id —— 自动化测试用，绝对确定
2. 最后一条 user 消息命中某场景的 `match` 关键词 —— 手动测试用，说人话即可
3. 都不中则回落到 `basic-chat`

### 轮次推进

一个场景可以有多轮（模型调工具 → 拿到结果 → 继续）。**该回放第几轮由请求历史
推算**，服务端不存计数器：数最后一条 user 消息之后有几条带 `tool_calls` 的
assistant 消息，就是第几轮。

服务端一旦有状态，测试就得关心执行顺序，重试也会错位。

## 录制真实数据

手写 fixture 猜不准真实模型的口气、分片节奏和工具调用习惯。录制模式解决这个：
它把请求转发给真实 LLM，**逐字透传**响应给应用，同时旁路抓成 fixture。

透传而非缓冲是关键 —— 应用照常流式渲染、照常执行真工具、照常发起下一轮，
于是**多轮工具流程和真实的分片节奏都被原样记录下来**，不需要你手工编排。

**一次性准备**：

```bash
cp scripts/fake-llm/config.template.json scripts/fake-llm/config.json
# 编辑 config.json，填真实的 base_url / api_key / model
```

`config.json` 已在 `.gitignore` 里。模板里 `temperature` 默认 0 —— 录制希望
同样的输入尽量得到同样的输出。

**录一个场景**：

```bash
pnpm fake-llm:record my-case --title "测什么" --match "触发词1,触发词2"
```

然后正常起应用（`pnpm tauri dev`），API 地址仍指向 `http://127.0.0.1:8787/v1`，
像平时那样把这个场景走一遍。每轮结束就落盘到 `fixtures/my-case.json`，
中途 Ctrl-C 也能留下已录到的部分。

几点注意：

- 应用设置页里填的模型名会被 `config.json` 的 `model` **覆盖**
- 重录同一个 id 时，第 0 轮会清空旧的 `rounds`，不会留下上一次的尾巴
- 录完记得检查一遍：真实回答里可能带有本机路径、时间戳这类不该进仓库的内容
- 录制期间点「停止」不会落盘 —— 半截的轮次当 fixture 只会误导人

## 写新场景（手工）

在 `fixtures/` 加一个 JSON：

```jsonc
{
  "id": "my-case",              // 文件名与之一致；也是选场景用的 model 名
  "title": "一句话说清测什么",
  "match": ["关键词"],           // 命中最后一条 user 消息则选中
  "rounds": [
    {
      "reasoning": ["思维链分段"],       // 可选，走 reasoning_content
      "content": ["正文", "分段发"],
      "toolCalls": [
        { "id": "call_1", "name": "Read", "arguments": "{\"path\":\"README.md\"}" }
      ],
      "finishReason": "tool_calls",     // 有工具调用时填 tool_calls，否则 stop
      "usage": { "prompt_tokens": 820, "completion_tokens": 28, "total_tokens": 848 }
    }
  ],
  "delayMs": 0,                  // 每帧间隔，做慢速/可取消场景用
  "toolCallStyle": "standard",   // standard | no-index-micro（压 accumulator）
  "injectBadFrame": false,       // 插一帧非法 JSON，验证客户端跳过而非中断
  "http": { "status": 401, "body": { "error": { "message": "..." } } }  // 直接返错，不进 SSE
}
```

`arguments` 按协议是 **JSON 字符串**而非对象。

工具参数引用的文件必须在 `sandbox/` 里真实存在，否则测的是失败路径。
改了 `sandbox/` 的内容，对应的断言也要一起改。
