//! Edit 工具：按字符串精确替换修改文件。
//!
//! 用「唯一匹配的旧字符串 → 新字符串」而非行号定位：行号在模型上一次
//! Read 之后可能已经失效，而带足够上下文的字符串是自校验的。

use std::sync::Arc;

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use super::diff::{self, LineEnding};
use super::path;
use super::read_registry::ReadRegistry;

pub struct EditTool {
    pub registry: Arc<ReadRegistry>,
}

#[derive(Deserialize)]
struct Args {
    path: String,
    old_string: String,
    new_string: String,
    /// 替换全部匹配而非要求唯一匹配。
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "Edit"
    }

    fn description(&self) -> &'static str {
        "在文件中把 old_string 替换成 new_string。\
         编辑前必须先用 Read 读过该文件。\
         old_string 必须与文件内容**逐字符**一致（含缩进），且在文件中唯一 —— \
         不唯一时请补充前后文使其唯一，或用 replace_all 替换全部。\
         注意：Read 返回的行号前缀（`   1→`）不是文件内容，不要写进 old_string。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "文件路径，相对于工作区根目录"
                },
                "old_string": {
                    "type": "string",
                    "description": "要被替换的原文，必须逐字符一致且唯一"
                },
                "new_string": {
                    "type": "string",
                    "description": "替换后的新内容。留空表示删除 old_string"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "替换所有匹配处，默认 false（要求唯一匹配）"
                }
            },
            "required": ["path", "old_string", "new_string"],
            "additionalProperties": false
        })
    }

    fn preview(&self, args: &Value) -> String {
        let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
        format!("Edit({path})")
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError> {
        let args: Args = serde_json::from_value(args)
            .map_err(|e| ToolError::recoverable(format!("参数不正确：{e}")))?;

        if args.old_string == args.new_string {
            return Err(ToolError::recoverable(
                "old_string 和 new_string 完全相同，这次编辑没有任何效果。",
            ));
        }

        let full = path::resolve(&ctx.workspace_root, &args.path)?;
        let shown = path::display(&ctx.workspace_root, &full);

        let meta = tokio::fs::metadata(&full).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::recoverable(format!(
                    "文件 `{shown}` 不存在。要新建文件请用 Write 工具。"
                ))
            } else {
                ToolError::recoverable(format!("无法访问 `{shown}`：{e}"))
            }
        })?;

        let modified = meta
            .modified()
            .map_err(|e| ToolError::fatal(format!("无法读取 `{shown}` 的修改时间：{e}")))?;

        self.registry.check(&full, modified, &shown)?;

        let raw = tokio::fs::read_to_string(&full)
            .await
            .map_err(|e| ToolError::recoverable(format!("读取 `{shown}` 失败：{e}")))?;

        // 换行符先记下来，写回时原样还原
        let ending = LineEnding::detect(&raw);
        let original = LineEnding::normalize(&raw);
        let old_norm = LineEnding::normalize(&args.old_string);
        let new_norm = LineEnding::normalize(&args.new_string);

        let updated = replace(&original, &old_norm, &new_norm, args.replace_all, &shown)?;

        tokio::fs::write(&full, ending.apply(&updated))
            .await
            .map_err(|e| ToolError::recoverable(format!("写入 `{shown}` 失败：{e}")))?;

        // 刷新记录，模型可以接着改同一个文件而不必重新 Read
        if let Ok(m) = tokio::fs::metadata(&full).await.and_then(|m| m.modified()) {
            self.registry.refresh(&full, m);
        }

        Ok(render(&original, &updated, &shown))
    }
}

/// 执行替换，匹配不唯一或找不到时给出可操作的错误。
fn replace(
    haystack: &str,
    old: &str,
    new: &str,
    replace_all: bool,
    shown: &str,
) -> Result<String, ToolError> {
    let count = haystack.matches(old).count();

    if count == 0 {
        return Err(ToolError::recoverable(format!(
            "在 `{shown}` 中找不到 old_string。{}",
            mismatch_hint(haystack, old)
        )));
    }

    if count > 1 && !replace_all {
        return Err(ToolError::recoverable(format!(
            "old_string 在 `{shown}` 中出现了 {count} 次，无法确定改哪一处。\
             请在 old_string 前后补充更多上下文使其唯一；\
             若确实要全部替换，请传 replace_all=true。"
        )));
    }

    Ok(if replace_all {
        haystack.replace(old, new)
    } else {
        haystack.replacen(old, new, 1)
    })
}

/// 猜测匹配失败的原因，给模型一个具体的修正方向。
fn mismatch_hint(haystack: &str, old: &str) -> String {
    // 两边都按行去掉首尾空白再比：能对上就说明只是缩进/行尾空格不一致。
    // 单行的情况不会走到这里 —— 不带缩进的单行本来就是带缩进那行的子串。
    let strip = |s: &str| s.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let old_stripped = strip(old);

    if !old_stripped.is_empty() && strip(haystack).contains(&old_stripped) {
        return "文件里有内容相同但缩进或行尾空格不同的段落 —— \
                old_string 必须与原文逐字符一致，请重新 Read 复制确切的原文。"
            .into();
    }

    // 首行能对上说明大方向没错，多半是后面某行有出入
    if let Some(first) = old.lines().next().filter(|l| !l.trim().is_empty()) {
        if haystack.contains(first) {
            return format!(
                "文件里能找到首行 `{}`，但整段对不上。\
                 请重新 Read 确认这一段的确切内容（尤其是空行和行尾空格）。",
                first.trim()
            );
        }
    }

    "请重新 Read 确认文件的当前内容。".into()
}

fn render(old: &str, new: &str, shown: &str) -> ToolOutcome {
    let result = diff::build(old, new);

    // 给模型的只有一句摘要：它不需要 diff 的渲染细节，UI 才需要
    let llm_text = format!(
        "已修改 `{shown}`（+{} -{}）。",
        result.added, result.removed
    );

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
    fn replaces_unique_match() {
        let got = replace("a\nb\nc\n", "b", "B", false, "f.rs").expect("唯一匹配应当成功");
        assert_eq!(got, "a\nB\nc\n");
    }

    #[test]
    fn rejects_ambiguous_match() {
        let err = replace("x\nx\n", "x", "y", false, "f.rs").expect_err("多处匹配必须报错");
        let message = err.to_string();
        assert!(message.contains("出现了 2 次"), "{message}");
        assert!(
            message.contains("replace_all"),
            "错误消息该给出解法：{message}"
        );
    }

    #[test]
    fn replace_all_handles_multiple() {
        let got = replace("x\nx\n", "x", "y", true, "f.rs").expect("replace_all 应当成功");
        assert_eq!(got, "y\ny\n");
    }

    #[test]
    fn reports_missing_match() {
        let err = replace("a\n", "zzz", "b", false, "f.rs").expect_err("找不到必须报错");
        assert!(err.to_string().contains("找不到 old_string"));
    }

    /// 缩进不一致是最常见的失败原因，提示要点出这一点。
    ///
    /// 注意必须用多行样例：单行的 `let x = 1;` 本来就是 `    let x = 1;`
    /// 的子串，替换会直接成功（缩进被保留，行为是对的）。
    #[test]
    fn hints_about_indentation() {
        let haystack = "    let x = 1;\n    let y = 2;\n";
        let old = "let x = 1;\nlet y = 2;";
        let err = replace(haystack, old, "z", false, "f.rs").expect_err("缩进不同应当匹配失败");
        assert!(err.to_string().contains("缩进"), "{err}");
    }

    /// 不带缩进的单行仍应正常替换，且保留原有缩进。
    #[test]
    fn single_line_match_keeps_indentation() {
        let got = replace(
            "    let x = 1;\n",
            "let x = 1;",
            "let x = 2;",
            false,
            "f.rs",
        )
        .expect("单行子串匹配应当成功");
        assert_eq!(got, "    let x = 2;\n");
    }

    /// 首行能对上但整段对不上时，提示应指向具体位置。
    #[test]
    fn hints_when_only_first_line_matches() {
        let haystack = "fn main() {\n    do_something();\n}\n";
        let old = "fn main() {\n    other();\n}";
        let err = replace(haystack, old, "x", false, "f.rs").expect_err("应当匹配失败");
        assert!(err.to_string().contains("首行"), "{err}");
    }

    #[test]
    fn deletes_when_new_string_empty() {
        let got = replace("a\nb\nc\n", "b\n", "", false, "f.rs").expect("删除应当成功");
        assert_eq!(got, "a\nc\n");
    }

    #[test]
    fn summary_reports_counts() {
        let outcome = render("a\nb\n", "a\nB\n", "f.rs");
        assert!(outcome.llm_text.contains("+1 -1"), "{}", outcome.llm_text);

        match outcome.ui {
            ToolResultBody::Diff {
                added,
                removed,
                ref hunks,
                ..
            } => {
                assert_eq!((added, removed), (1, 1));
                assert!(!hunks.is_empty());
            }
            _ => panic!("Edit 的 UI 结果应当是 Diff"),
        }
    }

    // ---------- 端到端：真实文件 + ReadRegistry ----------

    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        tool: EditTool,
        registry: Arc<ReadRegistry>,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("创建临时目录");
            let root = dir.path().to_path_buf();
            let registry = Arc::new(ReadRegistry::new());
            Self {
                _dir: dir,
                root,
                tool: EditTool {
                    registry: registry.clone(),
                },
                registry,
            }
        }

        fn write_file(&self, name: &str, content: &str) -> PathBuf {
            let path = self.root.join(name);
            std::fs::write(&path, content).expect("写入测试文件");
            path
        }

        /// 模拟 Read 工具登记过这个文件。
        fn mark_read(&self, path: &PathBuf) {
            let modified = std::fs::metadata(path).unwrap().modified().unwrap();
            self.registry.record(path, modified);
        }

        fn ctx(&self) -> ToolCtx {
            ToolCtx {
                workspace_root: self.root.clone(),
                cancel: CancellationToken::new(),
                progress: agent_core::ProgressReporter::null(),
            }
        }

        async fn edit(&self, name: &str, old: &str, new: &str) -> Result<ToolOutcome, ToolError> {
            self.tool
                .execute(
                    json!({ "path": name, "old_string": old, "new_string": new }),
                    &self.ctx(),
                )
                .await
        }
    }

    /// CRLF 文件编辑后必须还是 CRLF。
    ///
    /// 搞错的话 git 会显示整个文件每一行都变了，而肉眼完全看不出来。
    #[tokio::test]
    async fn preserves_crlf_line_endings() {
        let f = Fixture::new();
        let path = f.write_file("crlf.txt", "one\r\ntwo\r\nthree\r\n");
        f.mark_read(&path);

        f.edit("crlf.txt", "two", "TWO")
            .await
            .expect("编辑应当成功");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, "one\r\nTWO\r\nthree\r\n", "CRLF 被改成 LF 了");
    }

    /// LF 文件不该被 Windows 环境带成 CRLF。
    #[tokio::test]
    async fn preserves_lf_line_endings() {
        let f = Fixture::new();
        let path = f.write_file("lf.txt", "one\ntwo\nthree\n");
        f.mark_read(&path);

        f.edit("lf.txt", "two", "TWO").await.expect("编辑应当成功");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, "one\nTWO\nthree\n");
        assert!(!after.contains('\r'), "LF 文件被写成了 CRLF");
    }

    /// old_string 里带 \n 而文件是 CRLF 时也应当匹配得上。
    #[tokio::test]
    async fn matches_across_line_endings() {
        let f = Fixture::new();
        let path = f.write_file("crlf.txt", "a\r\nb\r\nc\r\n");
        f.mark_read(&path);

        f.edit("crlf.txt", "a\nb", "x\ny")
            .await
            .expect("跨换行符风格也该匹配");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x\r\ny\r\nc\r\n");
    }

    /// 没 Read 过就编辑必须被拒。
    #[tokio::test]
    async fn refuses_edit_without_prior_read() {
        let f = Fixture::new();
        f.write_file("a.txt", "hello\n");

        let err = f
            .edit("a.txt", "hello", "bye")
            .await
            .expect_err("应当被拒绝");
        assert!(err.to_string().contains("必须先用 Read"), "{err}");

        assert_eq!(
            std::fs::read_to_string(f.root.join("a.txt")).unwrap(),
            "hello\n",
            "被拒绝时不该动文件"
        );
    }

    /// Read 之后文件被外部改动，编辑必须被拒，避免冲掉别人的改动。
    #[tokio::test]
    async fn refuses_edit_when_file_changed_since_read() {
        let f = Fixture::new();
        let path = f.write_file("a.txt", "hello\n");
        f.mark_read(&path);

        // 直接篡改登记的时间戳，等价于文件在读取后被外部修改
        f.registry.record(&path, std::time::SystemTime::UNIX_EPOCH);

        let err = f
            .edit("a.txt", "hello", "bye")
            .await
            .expect_err("应当被拒绝");
        assert!(err.to_string().contains("被外部修改过"), "{err}");
    }

    /// 一次编辑之后应当能接着改同一个文件，不必重新 Read。
    #[tokio::test]
    async fn allows_consecutive_edits() {
        let f = Fixture::new();
        let path = f.write_file("a.txt", "one two\n");
        f.mark_read(&path);

        f.edit("a.txt", "one", "1").await.expect("第一次编辑");
        f.edit("a.txt", "two", "2")
            .await
            .expect("第二次编辑不该要求重新 Read");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "1 2\n");
    }

    /// 工作区之外的路径必须被沙箱挡住。
    #[tokio::test]
    async fn rejects_paths_outside_workspace() {
        let f = Fixture::new();
        let err = f
            .edit("../escape.txt", "a", "b")
            .await
            .expect_err("逃逸路径应当被拒");
        assert!(err.to_string().contains("工作区之外"), "{err}");
    }

    /// 文件不存在时该引导模型改用 Write。
    #[tokio::test]
    async fn missing_file_suggests_write() {
        let f = Fixture::new();
        let err = f.edit("nope.txt", "a", "b").await.expect_err("应当报错");
        assert!(err.to_string().contains("Write"), "{err}");
    }
}
