//! Read 工具：读取工作区内的文本文件。

use std::path::Path;
use std::sync::Arc;

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use super::path;
use super::read_registry::ReadRegistry;

/// 单次最多返回的行数。超过就截断并告诉模型怎么读后续部分。
const DEFAULT_LIMIT: usize = 2000;

/// 单行最多保留的字符数。压缩过的 JS、data URI 之类的超长行会瞬间吃光上下文。
const MAX_LINE_CHARS: usize = 2000;

/// 文件大小上限。超过直接拒绝，避免把几百 MB 读进内存。
const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;

pub struct ReadTool {
    pub registry: Arc<ReadRegistry>,
}

#[derive(Deserialize)]
struct Args {
    path: String,
    /// 起始行号，从 1 开始。
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "Read"
    }

    fn description(&self) -> &'static str {
        "读取工作区内文本文件的内容。返回结果带行号（形如 `   1→内容`），\
         行号仅用于定位，不是文件内容的一部分。\
         文件很大时用 offset/limit 分段读取。无法读取二进制文件。"
    }

    fn prompt_contribution(&self) -> agent_core::PromptContribution {
        agent_core::PromptContribution {
            snippet: "读取文件内容，返回带行号的原文；大文件可分段读取。",
            guidelines: &[
                "需要了解代码时先用工具查看，不要凭猜测回答。",
                "文件很大时用 offset/limit 分段读取，不要一次读整份。",
            ],
        }
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "文件路径，相对于工作区根目录，如 `src/main.rs`"
                },
                "offset": {
                    "type": "integer",
                    "description": "起始行号（从 1 开始）。省略则从头读",
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "最多读取的行数，默认 2000",
                    "minimum": 1
                }
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }

    fn preview(&self, args: &Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
        match (
            args.get("offset").and_then(Value::as_u64),
            args.get("limit").and_then(Value::as_u64),
        ) {
            (Some(offset), _) => format!("Read({path}, 第 {offset} 行起)"),
            _ => format!("Read({path})"),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError> {
        let args: Args = serde_json::from_value(args)
            .map_err(|e| ToolError::recoverable(format!("参数不正确：{e}")))?;

        let full = path::resolve(&ctx.workspace_root, &args.path)?;
        let shown = path::display(&ctx.workspace_root, &full);

        let meta = tokio::fs::metadata(&full)
            .await
            .map_err(|e| describe_io_error(&full, &shown, &e))?;

        if meta.is_dir() {
            return Err(ToolError::recoverable(format!(
                "`{shown}` 是目录，不是文件。用 Glob 工具列出目录内容。"
            )));
        }
        if meta.len() > MAX_FILE_BYTES {
            return Err(ToolError::recoverable(format!(
                "`{shown}` 有 {:.1} MB，超过 {} MB 的读取上限。",
                meta.len() as f64 / 1024.0 / 1024.0,
                MAX_FILE_BYTES / 1024 / 1024
            )));
        }

        let bytes = tokio::fs::read(&full)
            .await
            .map_err(|e| describe_io_error(&full, &shown, &e))?;

        // NUL 字节是判定二进制最可靠的启发式：合法的 UTF-8 文本不会包含它
        if bytes.contains(&0) {
            return Err(ToolError::recoverable(format!(
                "`{shown}` 是二进制文件，无法作为文本读取。"
            )));
        }

        let text = String::from_utf8_lossy(&bytes);
        let render = render(
            &text,
            args.offset.unwrap_or(1),
            args.limit.unwrap_or(DEFAULT_LIMIT),
        );

        // 记下修改时间：Write / Edit 靠它判断文件在读取之后有没有被外部改动
        if let Ok(modified) = meta.modified() {
            self.registry.record(&full, modified);
        }

        let mut llm_text = render.body.clone();
        if let Some(note) = render.note() {
            llm_text.push_str(&note);
        }

        Ok(ToolOutcome {
            llm_text,
            ui: ToolResultBody::Text {
                content: render.body,
                truncated: render.truncated,
            },
        })
    }
}

struct Render {
    body: String,
    truncated: bool,
    /// 实际渲染的最后一行行号。
    last_line: usize,
    total_lines: usize,
}

impl Render {
    /// 给模型的补充说明。UI 不需要这段 —— 它有自己的截断提示。
    fn note(&self) -> Option<String> {
        if !self.truncated {
            return None;
        }
        Some(format!(
            "\n\n[已显示到第 {} 行，文件共 {} 行。继续读取请用 offset={}]",
            self.last_line,
            self.total_lines,
            self.last_line + 1
        ))
    }
}

/// 渲染成带行号的形式。
///
/// 行号用 `→` 与内容分隔而非制表符：制表符会被模型当成内容的一部分，
/// 而箭头是明确的视觉分隔，实测能显著减少模型把行号写进 Edit 参数的情况。
fn render(text: &str, offset: usize, limit: usize) -> Render {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    let start = offset.saturating_sub(1).min(total);
    let end = start.saturating_add(limit).min(total);

    let mut body = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let number = start + i + 1;
        let mut content: String = line.chars().take(MAX_LINE_CHARS).collect();
        if line.chars().count() > MAX_LINE_CHARS {
            content.push_str(" …[本行过长已截断]");
        }
        body.push_str(&format!("{number:>6}→{content}\n"));
    }

    if body.is_empty() {
        body.push_str("[文件为空]");
    }

    Render {
        body,
        truncated: end < total,
        last_line: end,
        total_lines: total,
    }
}

/// 把 IO 错误翻译成模型能据以纠正的说明。
fn describe_io_error(full: &Path, shown: &str, error: &std::io::Error) -> ToolError {
    use std::io::ErrorKind;
    match error.kind() {
        ErrorKind::NotFound => ToolError::recoverable(format!(
            "文件 `{shown}` 不存在。先用 Glob 确认路径是否正确。"
        )),
        ErrorKind::PermissionDenied => {
            ToolError::recoverable(format!("没有读取 `{shown}` 的权限。"))
        }
        _ => ToolError::recoverable(format!("读取 `{}` 失败：{error}", full.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_line_numbers() {
        let r = render("a\nb\nc\n", 1, 100);
        assert_eq!(r.body, "     1→a\n     2→b\n     3→c\n");
        assert!(!r.truncated);
    }

    #[test]
    fn honours_offset_and_limit() {
        let r = render("a\nb\nc\nd\n", 2, 2);
        assert_eq!(r.body, "     2→b\n     3→c\n");
        assert!(r.truncated, "后面还有内容，应当标记为截断");
        assert_eq!(r.last_line, 3);
        assert_eq!(r.total_lines, 4);
    }

    /// offset 超出文件范围不该 panic。
    #[test]
    fn handles_offset_past_end() {
        let r = render("a\n", 99, 10);
        assert_eq!(r.body, "[文件为空]");
        assert!(!r.truncated);
    }

    #[test]
    fn truncates_overlong_lines() {
        let long = "x".repeat(MAX_LINE_CHARS + 50);
        let r = render(&long, 1, 10);
        assert!(r.body.contains("本行过长已截断"));
        assert!(r.body.chars().count() < long.len() + 100);
    }

    #[test]
    fn marks_empty_file() {
        assert_eq!(render("", 1, 10).body, "[文件为空]");
    }
}
