"""兼容入口：转发至 Typer CLI。"""

from __future__ import annotations

import sys

from cli.app import app
from config import Config


def main() -> None:
    """运行 Typer CLI，无参数时根据 config 自动选择子命令。"""
    if len(sys.argv) == 1:
        config = Config.load_config()
        platform_type = config.get("platform", {}).get("inputer", {}).get("type", "")
        if platform_type:
            from ctf_platform import get_platform_cli

            cli_cmd = get_platform_cli(platform_type)
            sys.argv.append(cli_cmd or "solve")
        else:
            sys.argv.append("solve")
    app()


if __name__ == "__main__":
    main()
