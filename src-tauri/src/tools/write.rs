//! Write 工具：创建或整体覆盖文件。

use std::sync::Arc;

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use super::diff::{self, LineEnding};
use super::path;
use super::read_registry::ReadRegistry;

pub struct WriteTool {
    pub registry: Arc<ReadRegistry>,
}

#[derive(Deserialize)]
struct Args {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "Write"
    }

    fn description(&self) -> &'static str {
        "把内容写入文件，会**整体覆盖**已有内容。\
         覆盖已存在的文件前必须先用 Read 读过它。\
         只改文件的一部分时应当用 Edit，不要用本工具重写整个文件。\
         父目录不存在时会自动创建。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "文件路径，相对于工作区根目录"
                },
                "content": {
                    "type": "string",
                    "description": "文件的完整内容"
                }
            },
            "required": ["path", "content"],
            "additionalProperties": false
        })
    }

    fn preview(&self, args: &Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
        format!("Write({path})")
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError> {
        let args: Args = serde_json::from_value(args)
            .map_err(|e| ToolError::recoverable(format!("参数不正确：{e}")))?;

        let full = path::resolve(&ctx.workspace_root, &args.path)?;
        let shown = path::display(&ctx.workspace_root, &full);

        // 已存在的文件走覆盖保护；新建文件不需要先 Read
        let existing = match tokio::fs::metadata(&full).await {
            Ok(meta) if meta.is_dir() => {
                return Err(ToolError::recoverable(format!(
                    "`{shown}` 是目录，无法写入。"
                )));
            }
            Ok(meta) => {
                let modified = meta
                    .modified()
                    .map_err(|e| ToolError::fatal(format!("无法读取修改时间：{e}")))?;
                self.registry.check(&full, modified, &shown)?;

                Some(
                    tokio::fs::read_to_string(&full)
                        .await
                        .map_err(|e| ToolError::recoverable(format!("读取 `{shown}` 失败：{e}")))?,
                )
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(ToolError::recoverable(format!("无法访问 `{shown}`：{e}")));
            }
        };

        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::recoverable(format!("创建目录失败：{e}")))?;
        }

        // 覆盖已有文件时沿用它原来的换行符；新建文件用 LF
        let ending = existing
            .as_deref()
            .map_or(LineEnding::Lf, LineEnding::detect);
        let new_content = LineEnding::normalize(&args.content);

        tokio::fs::write(&full, ending.apply(&new_content))
            .await
            .map_err(|e| ToolError::recoverable(format!("写入 `{shown}` 失败：{e}")))?;

        if let Ok(m) = tokio::fs::metadata(&full).await.and_then(|m| m.modified()) {
            self.registry.refresh(&full, m);
        }

        let old_content = existing
            .as_deref()
            .map(LineEnding::normalize)
            .unwrap_or_default();
        Ok(render(
            &old_content,
            &new_content,
            &shown,
            existing.is_none(),
        ))
    }
}

fn render(old: &str, new: &str, shown: &str, created: bool) -> ToolOutcome {
    let result = diff::build(old, new);

    let llm_text = if created {
        format!("已创建 `{shown}`（{} 行）。", new.lines().count())
    } else {
        format!(
            "已覆盖 `{shown}`（+{} -{}）。",
            result.added, result.removed
        )
    };

    ToolOutcome {
        llm_text,
        ui: ToolResultBody::Diff {
            path: shown.to_string(),
            hunks: result.hunks,
            added: result.added,
            removed: result.removed,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_creation() {
        let outcome = render("", "a\nb\n", "new.rs", true);
        assert!(outcome.llm_text.contains("已创建"), "{}", outcome.llm_text);
        assert!(outcome.llm_text.contains("2 行"), "{}", outcome.llm_text);
    }

    #[test]
    fn reports_overwrite_counts() {
        let outcome = render("a\n", "b\n", "old.rs", false);
        assert!(outcome.llm_text.contains("已覆盖"), "{}", outcome.llm_text);
        assert!(outcome.llm_text.contains("+1 -1"), "{}", outcome.llm_text);
    }

    #[test]
    fn produces_diff_body() {
        match render("a\n", "b\n", "f.rs", false).ui {
            ToolResultBody::Diff {
                path,
                added,
                removed,
                ..
            } => {
                assert_eq!(path, "f.rs");
                assert_eq!((added, removed), (1, 1));
            }
            _ => panic!("Write 的 UI 结果应当是 Diff"),
        }
    }
}
