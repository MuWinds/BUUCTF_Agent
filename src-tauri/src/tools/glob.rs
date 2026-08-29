//! Glob 工具：按通配符查找文件。

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use globset::{Glob, GlobMatcher};
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::{json, Value};

use super::path;

/// 最多返回的匹配数。超过就截断 —— 一次给模型几千个路径没有意义，
/// 只会挤占上下文。
const MAX_RESULTS: usize = 300;

pub struct GlobTool;

#[derive(Deserialize)]
struct Args {
    pattern: String,
    /// 搜索起点，相对工作区。省略则从工作区根开始。
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "Glob"
    }

    fn description(&self) -> &'static str {
        "按通配符查找文件，返回路径列表，按最近修改时间排序。\
         支持 `**`（跨目录）、`*`、`?`、`{a,b}` 语法，例如 `**/*.rs`、`src/**/*.{ts,tsx}`。\
         自动跳过 .gitignore 忽略的文件和 .git 目录。\
         需要按文件内容搜索时用 Grep，不要用本工具。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "通配符模式，如 `**/*.rs`。匹配的是相对工作区的路径"
                },
                "path": {
                    "type": "string",
                    "description": "搜索起点目录，相对工作区。省略则搜索整个工作区"
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn preview(&self, args: &Value) -> String {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("?");
        match args.get("path").and_then(Value::as_str) {
            Some(dir) if !dir.is_empty() => format!("Glob({pattern}, 在 {dir})"),
            _ => format!("Glob({pattern})"),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError> {
        let args: Args = serde_json::from_value(args)
            .map_err(|e| ToolError::recoverable(format!("参数不正确：{e}")))?;

        let matcher = Glob::new(&args.pattern)
            .map_err(|e| {
                ToolError::recoverable(format!(
                    "通配符 `{}` 无效：{e}。示例：`**/*.rs`、`src/**/*.{{ts,tsx}}`",
                    args.pattern
                ))
            })?
            .compile_matcher();

        let search_root = match &args.path {
            Some(dir) if !dir.trim().is_empty() => path::resolve(&ctx.workspace_root, dir)?,
            _ => ctx.workspace_root.clone(),
        };

        if !search_root.is_dir() {
            return Err(ToolError::recoverable(format!(
                "`{}` 不是目录。",
                path::display(&ctx.workspace_root, &search_root)
            )));
        }

        let root = ctx.workspace_root.clone();
        let cancel = ctx.cancel.clone();

        // ignore::Walk 是同步阻塞的，扔到阻塞线程池，别占着 async 执行器
        let found =
            tokio::task::spawn_blocking(move || walk(&search_root, &root, &matcher, &cancel))
                .await
                .map_err(|e| ToolError::fatal(format!("遍历任务异常终止：{e}")))?;

        Ok(render(found, &args.pattern))
    }
}

struct Found {
    paths: Vec<String>,
    truncated: bool,
}

fn walk(
    search_root: &std::path::Path,
    workspace_root: &std::path::Path,
    matcher: &GlobMatcher,
    cancel: &tokio_util::sync::CancellationToken,
) -> Found {
    // (修改时间, 相对路径)，先收集再排序 —— 遍历顺序本身没有意义
    let mut hits: Vec<(std::time::SystemTime, String)> = Vec::new();
    let mut truncated = false;

    let walker = WalkBuilder::new(search_root)
        .hidden(true) // 跳过隐藏文件
        .git_ignore(true)
        .git_global(true)
        .parents(true) // 上层目录的 .gitignore 也算数
        // 默认只在 git 仓库里才读 .gitignore。工作区未必是仓库，但只要有
        // .gitignore 就该尊重它，否则会把 node_modules、target 全列出来。
        .require_git(false)
        .build();

    for entry in walker {
        if cancel.is_cancelled() {
            break;
        }

        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }

        let relative = match entry.path().strip_prefix(workspace_root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        // 用相对路径匹配：模式 `src/**/*.rs` 才能按预期工作
        if !matcher.is_match(&relative) {
            continue;
        }

        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);

        hits.push((modified, relative));

        // 多收一些用于判断是否真的超限，但不至于无限增长
        if hits.len() > MAX_RESULTS * 4 {
            truncated = true;
            break;
        }
    }

    // 最近修改的排前面：模型多半在找刚动过的文件
    hits.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));

    if hits.len() > MAX_RESULTS {
        truncated = true;
        hits.truncate(MAX_RESULTS);
    }

    Found {
        paths: hits.into_iter().map(|(_, p)| p).collect(),
        truncated,
    }
}

fn render(found: Found, pattern: &str) -> ToolOutcome {
    if found.paths.is_empty() {
        let message = format!("没有文件匹配 `{pattern}`。");
        return ToolOutcome {
            llm_text: message.clone(),
            ui: ToolResultBody::Text {
                content: message,
                truncated: false,
            },
        };
    }

    let content = found.paths.join("\n");
    let mut llm_text = content.clone();

    if found.truncated {
        llm_text.push_str(&format!(
            "\n\n[结果超过 {MAX_RESULTS} 条已截断，请用更精确的模式缩小范围]"
        ));
    } else {
        llm_text.push_str(&format!("\n\n[共 {} 个文件]", found.paths.len()));
    }

    ToolOutcome {
        llm_text,
        ui: ToolResultBody::Text {
            content,
            truncated: found.truncated,
        },
    }
}
