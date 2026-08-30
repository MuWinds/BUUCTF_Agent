//! Grep 工具：按内容搜索文件。
//!
//! 用 ripgrep 拆出的 `grep-searcher` 而非自己拼 `walkdir` + `regex`：
//! 前者自带二进制检测、行号追踪、内存映射优化，性能与 `rg` 同量级。

use std::path::Path;

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use super::path;

/// 最多返回的匹配行数。
const MAX_MATCHES: usize = 200;

/// 单文件最多返回的匹配数，防止一个巨型文件吃掉整个配额。
const MAX_PER_FILE: usize = 30;

/// 单行最多保留的字符数。
const MAX_LINE_CHARS: usize = 400;

pub struct GrepTool;

#[derive(Deserialize)]
struct Args {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    /// 文件名过滤，如 `*.rs`。
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    case_insensitive: bool,
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "Grep"
    }

    fn description(&self) -> &'static str {
        "按正则表达式搜索文件内容，返回 `路径:行号: 内容` 形式的匹配列表。\
         语法是 Rust regex（与 ripgrep 一致），不支持反向引用和环视。\
         用 glob 参数限定文件类型可大幅缩小范围。\
         自动跳过 .gitignore 忽略的文件和二进制文件。\
         按文件名找文件用 Glob，不要用本工具。"
    }

    fn prompt_contribution(&self) -> agent_core::PromptContribution {
        agent_core::PromptContribution {
            snippet: "按内容正则搜索文件，返回 `路径:行号: 内容` 匹配列表。",
            guidelines: &[
                "按内容搜索用 Grep，不要用 Bash 的 grep/findstr。",
                "用 glob 参数限定文件类型可大幅缩小范围，加快搜索。",
            ],
        }
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "正则表达式，如 `fn\\s+main`。特殊字符需转义"
                },
                "path": {
                    "type": "string",
                    "description": "搜索起点目录，相对工作区。省略则搜索整个工作区"
                },
                "glob": {
                    "type": "string",
                    "description": "只搜索匹配此通配符的文件，如 `*.rs`、`**/*.{ts,tsx}`"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "是否忽略大小写，默认 false"
                }
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }

    fn preview(&self, args: &Value) -> String {
        let pattern = args.get("pattern").and_then(Value::as_str).unwrap_or("?");
        match args.get("glob").and_then(Value::as_str) {
            Some(glob) if !glob.is_empty() => format!("Grep({pattern}, {glob})"),
            _ => format!("Grep({pattern})"),
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError> {
        let args: Args = serde_json::from_value(args)
            .map_err(|e| ToolError::recoverable(format!("参数不正确：{e}")))?;

        // line_terminator 让 searcher 能走按行扫描的快路径
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(args.case_insensitive)
            .line_terminator(Some(b'\n'))
            .build(&args.pattern)
            .map_err(|e| {
                ToolError::recoverable(format!(
                    "正则 `{}` 无效：{e}。注意这是 Rust regex 语法，不支持反向引用和环视。",
                    args.pattern
                ))
            })?;

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

        let file_filter = match &args.glob {
            Some(g) if !g.trim().is_empty() => Some(
                globset::Glob::new(g)
                    .map_err(|e| ToolError::recoverable(format!("通配符 `{g}` 无效：{e}")))?
                    .compile_matcher(),
            ),
            _ => None,
        };

        let root = ctx.workspace_root.clone();
        let cancel = ctx.cancel.clone();

        // grep-searcher 是同步阻塞的，扔到阻塞线程池
        let found = tokio::task::spawn_blocking(move || {
            search(&search_root, &root, &matcher, file_filter.as_ref(), &cancel)
        })
        .await
        .map_err(|e| ToolError::fatal(format!("搜索任务异常终止：{e}")))?;

        Ok(render(found, &args.pattern))
    }
}

struct Found {
    lines: Vec<String>,
    files: usize,
    truncated: bool,
}

fn search(
    search_root: &Path,
    workspace_root: &Path,
    matcher: &RegexMatcher,
    file_filter: Option<&globset::GlobMatcher>,
    cancel: &CancellationToken,
) -> Found {
    let mut lines: Vec<String> = Vec::new();
    let mut files = 0usize;
    let mut truncated = false;

    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        // 前 8KB 里出现 NUL 就判定为二进制并跳过，避免把 .exe 的内容吐出来
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let walker = WalkBuilder::new(search_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        // 默认只在 git 仓库里才读 .gitignore。工作区未必是仓库，但只要有
        // .gitignore 就该尊重它，否则会把 node_modules、target 全搜一遍。
        .require_git(false)
        .build();

    for entry in walker {
        if cancel.is_cancelled() || lines.len() >= MAX_MATCHES {
            truncated = truncated || lines.len() >= MAX_MATCHES;
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

        if let Some(filter) = file_filter {
            if !filter.is_match(&relative) {
                continue;
            }
        }

        let mut in_file = 0usize;
        let before = lines.len();

        let result = searcher.search_path(
            matcher,
            entry.path(),
            UTF8(|number, line| {
                if in_file >= MAX_PER_FILE || lines.len() >= MAX_MATCHES {
                    // 返回 false 让 searcher 停掉这个文件
                    return Ok(false);
                }
                in_file += 1;
                lines.push(format!("{relative}:{number}: {}", clip(line.trim_end())));
                Ok(true)
            }),
        );

        if let Err(e) = result {
            tracing::debug!("搜索 {relative} 失败: {e}");
            continue;
        }

        if lines.len() > before {
            files += 1;
        }
        if in_file >= MAX_PER_FILE {
            truncated = true;
        }
    }

    Found {
        lines,
        files,
        truncated,
    }
}

fn clip(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let mut s: String = line.chars().take(MAX_LINE_CHARS).collect();
    s.push_str(" …");
    s
}

fn render(found: Found, pattern: &str) -> ToolOutcome {
    if found.lines.is_empty() {
        let message = format!("没有内容匹配 `{pattern}`。");
        return ToolOutcome {
            llm_text: message.clone(),
            ui: ToolResultBody::Text {
                content: message,
                truncated: false,
            },
        };
    }

    let content = found.lines.join("\n");
    let mut llm_text = content.clone();

    llm_text.push_str(&format!(
        "\n\n[{} 处匹配，分布在 {} 个文件{}]",
        found.lines.len(),
        found.files,
        if found.truncated {
            "；结果已截断，请缩小搜索范围"
        } else {
            ""
        }
    ));

    ToolOutcome {
        llm_text,
        ui: ToolResultBody::Text {
            content,
            truncated: found.truncated,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn clips_overlong_lines() {
        let long = "x".repeat(MAX_LINE_CHARS + 100);
        let clipped = clip(&long);
        assert!(clipped.ends_with('…'));
        assert_eq!(clipped.chars().count(), MAX_LINE_CHARS + 2);
    }

    #[test]
    fn keeps_short_lines_intact() {
        assert_eq!(clip("fn main() {}"), "fn main() {}");
    }

    #[test]
    fn reports_no_matches() {
        let outcome = render(
            Found {
                lines: vec![],
                files: 0,
                truncated: false,
            },
            "zzz",
        );
        assert!(outcome.llm_text.contains("没有内容匹配"));
    }

    #[test]
    fn summarises_matches() {
        let found = Found {
            lines: vec!["a.rs:1: fn main".into(), "b.rs:2: fn helper".into()],
            files: 2,
            truncated: false,
        };
        let outcome = render(found, "fn");
        assert!(outcome.llm_text.contains("2 处匹配，分布在 2 个文件"));
        assert!(!outcome.llm_text.contains("已截断"));
    }

    #[test]
    fn warns_when_truncated() {
        let found = Found {
            lines: vec!["a.rs:1: x".into()],
            files: 1,
            truncated: true,
        };
        assert!(render(found, "x").llm_text.contains("已截断"));
    }

    // ---------- 真实文件系统上的行为 ----------

    /// 搭一个小仓库：普通源文件、被 gitignore 的文件、二进制文件各一。
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("创建临时目录");
        let root = dir.path();

        fs::write(root.join(".gitignore"), "ignored.rs\nbuild/\n").unwrap();
        fs::write(root.join("main.rs"), "fn main() {\n    needle();\n}\n").unwrap();
        fs::write(root.join("lib.rs"), "pub fn needle() {}\n").unwrap();
        fs::write(root.join("notes.txt"), "needle in text\n").unwrap();
        fs::write(root.join("ignored.rs"), "fn needle_ignored() {}\n").unwrap();

        fs::create_dir(root.join("build")).unwrap();
        fs::write(root.join("build").join("gen.rs"), "needle generated\n").unwrap();

        // 含 NUL 字节，应被判定为二进制
        fs::write(root.join("blob.bin"), b"needle\x00\x01\x02binary").unwrap();

        dir
    }

    fn run(root: &std::path::Path, pattern: &str, glob: Option<&str>) -> Found {
        let matcher = RegexMatcherBuilder::new()
            .line_terminator(Some(b'\n'))
            .build(pattern)
            .expect("正则应当有效");
        let filter = glob.map(|g| globset::Glob::new(g).unwrap().compile_matcher());

        search(
            root,
            root,
            &matcher,
            filter.as_ref(),
            &CancellationToken::new(),
        )
    }

    #[test]
    fn finds_matches_across_files() {
        let dir = fixture();
        let found = run(dir.path(), "needle", None);

        let joined = found.lines.join("\n");
        assert!(
            joined.contains("main.rs:2"),
            "应当匹配到 main.rs 第 2 行：{joined}"
        );
        assert!(joined.contains("lib.rs:1"), "应当匹配到 lib.rs：{joined}");
        assert!(
            joined.contains("notes.txt:1"),
            "应当匹配到 notes.txt：{joined}"
        );
    }

    /// .gitignore 里的文件和目录都不该被搜到。
    #[test]
    fn respects_gitignore() {
        let dir = fixture();
        let joined = run(dir.path(), "needle", None).lines.join("\n");

        assert!(
            !joined.contains("ignored.rs"),
            "gitignore 的文件被搜到了：{joined}"
        );
        assert!(
            !joined.contains("gen.rs"),
            "gitignore 的目录被搜到了：{joined}"
        );
    }

    /// 二进制文件里的匹配不该被吐出来。
    #[test]
    fn skips_binary_files() {
        let dir = fixture();
        let joined = run(dir.path(), "needle", None).lines.join("\n");

        assert!(
            !joined.contains("blob.bin"),
            "二进制文件被当成文本搜索了：{joined}"
        );
    }

    #[test]
    fn glob_filter_narrows_search() {
        let dir = fixture();
        let joined = run(dir.path(), "needle", Some("*.rs")).lines.join("\n");

        assert!(joined.contains("main.rs"));
        assert!(!joined.contains("notes.txt"), "glob 过滤没生效：{joined}");
    }

    #[test]
    fn case_insensitive_matching() {
        let dir = fixture();
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(true)
            .line_terminator(Some(b'\n'))
            .build("NEEDLE")
            .unwrap();

        let found = search(
            dir.path(),
            dir.path(),
            &matcher,
            None,
            &CancellationToken::new(),
        );
        assert!(!found.lines.is_empty(), "忽略大小写时应当能匹配到");
    }

    /// 取消令牌一旦触发就应立刻停止遍历。
    #[test]
    fn honours_cancellation() {
        let dir = fixture();
        let matcher = RegexMatcherBuilder::new()
            .line_terminator(Some(b'\n'))
            .build("needle")
            .unwrap();

        let cancel = CancellationToken::new();
        cancel.cancel();

        let found = search(dir.path(), dir.path(), &matcher, None, &cancel);
        assert!(found.lines.is_empty(), "已取消时不该继续搜索");
    }

    /// 路径以工作区为基准显示，且用正斜杠（Windows 上也一样）。
    #[test]
    fn reports_workspace_relative_paths() {
        let dir = fixture();
        let sub = dir.path().join("src");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("deep.rs"), "needle\n").unwrap();

        let joined = run(dir.path(), "needle", None).lines.join("\n");
        assert!(joined.contains("src/deep.rs:1"), "路径格式不对：{joined}");
    }

    /// 让 PathBuf 导入有实际用途，同时确认目录不存在时不会 panic。
    #[test]
    fn missing_directory_yields_nothing() {
        let missing = PathBuf::from("definitely-not-a-real-directory-xyz");
        let found = run(&missing, "needle", None);
        assert!(found.lines.is_empty());
    }
}
