"""CTF Agent CLI 入口"""
import argparse
import json
import sys
from pathlib import Path

from agent.orchestrator import ReActOrchestrator


def load_config(config_path: str = "config.json") -> dict:
    """加载配置文件"""
    p = Path(config_path)
    if not p.exists():
        print(f"[错误] 配置文件 {config_path} 不存在")
        print("请复制 config_template.json 为 config.json 并填写 API 配置")
        sys.exit(1)
    with open(p, "r", encoding="utf-8") as f:
        return json.load(f)


def read_prompt_from_stdin() -> str:
    """从 stdin 交互式读取题面，以 EOF 行结束"""
    print("请输入题面描述（输入单独一行 EOF 结束）:")
    lines = []
    while True:
        try:
            line = input()
        except EOFError:
            break
        if line.strip() == "EOF":
            break
        lines.append(line)
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description="CTF Agent 解题助手")
    parser.add_argument("--mode", choices=["auto", "confirm"], default="confirm",
                        help="运行模式: auto(自动) / confirm(半自动，默认)")
    parser.add_argument("--max-steps", type=int, default=20, help="最大循环步数")
    parser.add_argument("--config", default="config.json", help="配置文件路径")
    parser.add_argument("--prompt", default=None, help="题面描述（不提供则从 stdin 读取）")

    args = parser.parse_args()

    # 加载配置
    config = load_config(args.config)
    llm_config = config.get("llm", {})

    # 读取题面
    if args.prompt:
        prompt = args.prompt
    else:
        prompt = read_prompt_from_stdin()

    if not prompt.strip():
        print("[错误] 题面不能为空")
        sys.exit(1)

    # 启动 Agent
    orchestrator = ReActOrchestrator(
        model_name=llm_config.get("model", "gpt-4o-mini"),
        api_key=llm_config.get("api_key", ""),
        base_url=llm_config.get("base_url", ""),
        prompt=prompt,
        max_steps=args.max_steps,
        mode=args.mode,
    )

    orchestrator.run()


if __name__ == "__main__":
    main()
