#!/usr/bin/env bash
set -euo pipefail

# PostToolUse hook: 在 Edit/Write 后对修改的 .py 文件运行 ruff check
# stdin 传入 JSON，包含 file_path 字段

input=$(cat)
file_path=$(echo "$input" | python -c "import sys,json; print(json.load(sys.stdin).get('file_path',''))" 2>/dev/null || true)

# 只处理 .py 文件
if [[ ! "$file_path" == *.py ]]; then
    exit 0
fi

# 规范化路径：反斜杠转正斜杠，去掉仓库根目录前缀
file_path="${file_path//\\//}"
repo_root=$(git rev-parse --show-toplevel 2>/dev/null || echo ".")
file_path="${file_path#"$repo_root"/}"

# 对单个文件运行 ruff check
if ! ruff check "$file_path" 2>&1; then
    echo "ruff check 失败: $file_path" >&2
    exit 2
fi
