"""ReAct 编排器 - CTF Agent 主循环"""
import json
from datetime import datetime

from langchain_core.messages import AIMessage, HumanMessage, SystemMessage, ToolMessage
from langchain_openai import ChatOpenAI

from agent.state import EvidenceItem, State
from storage import EvidenceStore, SessionManager
from utils.tools import http_request


# ---------- System Prompt ----------

SYSTEM_PROMPT = """你是一个专业的 CTF（Capture The Flag）解题 Agent。
你的任务是通过 ReAct（Reason-Act-Observe）循环来分析和解决 CTF 题目。

## 工作原则
1. 首先仔细分析题面，从中提取目标地址、提示信息、可能的漏洞类型
2. 每轮只做一件可验证的小事
3. 优先选择低成本、高信息增益的动作（先侦察，再深入）
4. 所有观测必须结构化记录
5. 如果连续多轮无进展，切换策略

## 可用工具
- http_request: 发送 HTTP 请求，用于 Web 侦察、获取页面、提交表单等

## 输出要求
每轮请先简要说明：
1. 当前目标（success_criteria）
2. 选择该动作的理由
3. 然后调用工具执行

当你认为已经找到 flag 或无法继续时，直接输出结论，不要调用工具。
flag 格式通常为 flag{{...}} 或 ctf{{...}}。

## 题目信息
{prompt}
"""


class ReActOrchestrator:
    """ReAct 编排器"""

    def __init__(
        self,
        model_name: str,
        api_key: str,
        base_url: str,
        prompt: str,
        max_steps: int = 20,
        mode: str = "confirm",
    ):
        self.prompt = prompt
        self.max_steps = max_steps
        self.mode = mode

        # 初始化 State
        self.state = State(
            prompt=prompt,
            max_steps=max_steps,
        )

        # 初始化 Session
        self.session = SessionManager()
        session_dir = self.session.create_session()
        self.state.session_dir = session_dir
        self.session.save_prompt(prompt)

        # 初始化证据存储
        self.evidence_store = EvidenceStore(self.session.get_log_path(""))

        # 工具注册表：name -> callable
        self.tools = [http_request]
        self.tool_map = {t.name: t for t in self.tools}

        # 初始化 LLM（bind_tools 让 LLM 知道可用工具的 schema）
        self.llm = ChatOpenAI(
            model=model_name,
            api_key=api_key,
            base_url=base_url,
            temperature=0.1,
        ).bind_tools(self.tools)

    def _execute_tool_calls(self, ai_message: AIMessage) -> list[ToolMessage]:
        """手动执行 AI 消息中的所有 tool_calls，返回 ToolMessage 列表"""
        tool_messages = []
        for tc in ai_message.tool_calls:
            tool_name = tc["name"]
            tool_args = tc["args"]
            tool_id = tc["id"]

            tool_fn = self.tool_map.get(tool_name)
            if tool_fn is None:
                result = json.dumps({"error": f"Unknown tool: {tool_name}"})
            else:
                try:
                    raw_result = tool_fn.invoke(tool_args)
                    result = json.dumps(raw_result, ensure_ascii=False) if isinstance(raw_result, dict) else str(raw_result)
                except Exception as e:
                    result = json.dumps({"error": str(e)})

            tool_messages.append(
                ToolMessage(content=result, tool_call_id=tool_id, name=tool_name)
            )
        return tool_messages

    def _record_evidence(self, step_id: int, goal: str, action: dict,
                         observation: dict, conclusion: str,
                         judge_result: str = None) -> None:
        """记录证据"""
        evidence = EvidenceItem(
            ts=datetime.now().isoformat(),
            step_id=step_id,
            goal=goal,
            action=action,
            observation=observation,
            conclusion=conclusion,
            judge_result=judge_result,
        )
        self.state.add_evidence(evidence)
        self.evidence_store.append_evidence(evidence)
        self.evidence_store.append_summary(
            step_id=step_id,
            goal=goal,
            action=json.dumps(action, ensure_ascii=False),
            observation=json.dumps(observation, ensure_ascii=False)[:500],
            conclusion=conclusion,
        )

    def run(self) -> None:
        """运行 Agent"""
        system_msg = SYSTEM_PROMPT.format(prompt=self.prompt)

        # 初始证据（step 0）
        self._record_evidence(
            step_id=0,
            goal="初始化会话",
            action={"tool_name": "init", "arguments": {"prompt": self.prompt[:200]}},
            observation={"summary": f"会话已创建: {self.state.session_dir}"},
            conclusion="开始解题",
        )

        initial_messages = [
            SystemMessage(content=system_msg),
            HumanMessage(content=f"请分析以下题面并开始解题:\n{self.prompt}"),
        ]

        print(f"\n{'='*60}")
        print(f"  CTF Agent 启动")
        print(f"  会话目录: {self.state.session_dir}")
        print(f"  模式: {'自动' if self.mode == 'auto' else '半自动(确认)'}")
        print(f"  最大步数: {self.max_steps}")
        print(f"{'='*60}\n")

        if self.mode == "auto":
            self._run_auto(initial_messages)
        else:
            self._run_confirm(initial_messages)

    def _run_auto(self, messages: list) -> None:
        """自动模式：直接运行到结束"""
        step = 0
        current_messages = list(messages)

        while step < self.max_steps:
            # --- 调用 LLM ---
            ai_response = self.llm.invoke(current_messages)
            current_messages.append(ai_response)
            step += 1

            # 打印 AI 思考
            if ai_response.content:
                print(f"\n--- Step {step} ---")
                print(ai_response.content)

            # 没有工具调用 → 结论
            if not ai_response.tool_calls:
                self._record_evidence(
                    step_id=step,
                    goal=ai_response.content[:200] if ai_response.content else "结论",
                    action={"tool_name": "none", "arguments": {}},
                    observation={"summary": "Agent 给出结论，无工具调用"},
                    conclusion=ai_response.content[:500] if ai_response.content else "",
                )
                break

            # 打印工具调用
            for tc in ai_response.tool_calls:
                print(f"\n[Tool] {tc['name']}({json.dumps(tc['args'], ensure_ascii=False)[:200]})")

            # --- 执行工具 ---
            tool_messages = self._execute_tool_calls(ai_response)

            for tm in tool_messages:
                current_messages.append(tm)
                preview = tm.content[:500]
                print(f"[Result] {preview}")

                self.evidence_store.append_tool_call(
                    tool_name=tm.name,
                    arguments={},
                    raw_output={"content": tm.content[:2000]},
                )

            # --- 记录完整证据 ---
            tc = ai_response.tool_calls[0]
            obs_content = tool_messages[0].content if tool_messages else ""

            self._record_evidence(
                step_id=step,
                goal=ai_response.content[:200] if ai_response.content else "工具调用",
                action={"tool_name": tc["name"], "arguments": tc["args"]},
                observation={"summary": obs_content[:500]},
                conclusion=f"工具 {tc['name']} 执行完成",
            )

        print(f"\n{'='*60}")
        print(f"  Agent 运行结束")
        print(f"  共执行 {step} 步")
        print(f"  证据记录: {self.session.get_log_path('evidence.jsonl')}")
        print(f"{'='*60}")

    def _run_confirm(self, messages: list) -> None:
        """半自动模式：每步确认"""
        step = 0
        current_messages = list(messages)

        while step < self.max_steps:
            # --- 阶段 1: 调用 LLM 决策 ---
            ai_response = self.llm.invoke(current_messages)
            current_messages.append(ai_response)
            step += 1

            # 打印 AI 思考
            if ai_response.content:
                print(f"\n--- Step {step} ---")
                print(ai_response.content)

            # --- 阶段 2: 判断是否有工具调用 ---
            if not ai_response.tool_calls:
                self._record_evidence(
                    step_id=step,
                    goal=ai_response.content[:200] if ai_response.content else "结论",
                    action={"tool_name": "none", "arguments": {}},
                    observation={"summary": "Agent 给出结论，无工具调用"},
                    conclusion=ai_response.content[:500] if ai_response.content else "",
                )
                print("\n[Agent 结论] 无更多工具调用，Agent 已给出结论。")
                break

            # --- 阶段 3: 展示工具调用，等待用户确认 ---
            for tc in ai_response.tool_calls:
                print(f"\n[将调用] {tc['name']}({json.dumps(tc['args'], ensure_ascii=False)[:200]})")

            user_input = input("\n执行? (y=执行 / n=跳过 / q=退出 / 自定义指令): ").strip().lower()

            if user_input == "q":
                print("用户退出")
                break
            elif user_input == "n":
                current_messages.pop()
                current_messages.append(
                    HumanMessage(content="用户跳过了该工具调用，请换一个思路。")
                )
                step -= 1
                continue
            elif user_input not in ("y", ""):
                current_messages.pop()
                current_messages.append(
                    HumanMessage(content=f"用户指示: {user_input}")
                )
                step -= 1
                continue

            # --- 阶段 4: 执行工具 ---
            tool_messages = self._execute_tool_calls(ai_response)

            for tm in tool_messages:
                current_messages.append(tm)
                preview = tm.content[:500]
                print(f"[Result] {preview}")

                self.evidence_store.append_tool_call(
                    tool_name=tm.name,
                    arguments={},
                    raw_output={"content": tm.content[:2000]},
                )

            # --- 阶段 5: 记录完整证据 ---
            tc = ai_response.tool_calls[0]
            obs_content = tool_messages[0].content if tool_messages else ""

            self._record_evidence(
                step_id=step,
                goal=ai_response.content[:200] if ai_response.content else "工具调用",
                action={"tool_name": tc["name"], "arguments": tc["args"]},
                observation={"summary": obs_content[:500]},
                conclusion=f"工具 {tc['name']} 执行完成",
            )

        print(f"\n{'='*60}")
        print(f"  Agent 运行结束")
        print(f"  共执行 {step} 步")
        print(f"  证据记录: {self.session.get_log_path('evidence.jsonl')}")
        print(f"{'='*60}")
