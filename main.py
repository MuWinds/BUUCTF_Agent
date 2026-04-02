"""兼容入口：转发至 Typer CLI。"""

from __future__ import annotations

import sys

from cli.app import app


def main() -> None:
    """运行 Typer CLI，并兼容无参数时默认 `solve`。"""
    if len(sys.argv) == 1:
        sys.argv.append("solve")
    app()


if __name__ == "__main__":
    main()
