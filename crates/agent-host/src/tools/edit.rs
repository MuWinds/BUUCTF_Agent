//! Edit 工具：按字符串精确替换修改文件。
//!
//! 用「唯一匹配的旧字符串 → 新字符串」而非行号定位：行号在模型上一次
//! Read 之后可能已经失效，而带足够上下文的字符串是自校验的。
//!
//! 一次调用可含多笔替换（`edits[]`）：每笔的 `old_string` 都匹配**原始**
//! 文件内容而非前一笔的结果，区间不得重叠 —— 与 pi 的 edit 工具一致，
//! 减少模型「改一处就重新 Read 一次」的往返。精确匹配失败时回退到模糊
//! 匹配：全角字符、弯引号、破折号、NBSP 等视觉上一样但字节不同的差异
//! 会被归一化后比对，命中后仍以原文区间替换，不破坏文件其余部分。

use std::sync::Arc;

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::diff::{self, LineEnding};
use super::path;
use super::read_registry::ReadRegistry;

pub struct EditTool {
    pub registry: Arc<ReadRegistry>,
}

/// 单笔替换。
#[derive(Deserialize, Serialize)]
struct EditItem {
    old_string: String,
    new_string: String,
}

#[derive(Deserialize)]
struct Args {
    path: String,
    /// 一次调用多笔替换。每笔都匹配原始文件内容，区间不得重叠。
    #[serde(default)]
    edits: Vec<EditItem>,
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "Edit"
    }

    fn description(&self) -> &'static str {
        "在文件中做精确的字符串替换。\
         编辑前必须先用 Read 读过该文件。\
         传 edits[] 数组，每笔含 old_string 与 new_string，均匹配原始文件内容，\
         区间不得重叠；不相邻的多处修改请在一次调用里完成，不要逐笔 Edit。\
         old_string 必须与文件内容**逐字符**一致（含缩进），且在文件中唯一 —— \
         不唯一时请补充前后文使其唯一；全角/弯引号等差异会被自动归一化匹配，\
         但请尽量复制 Read 返回的原文。\
         注意：Read 返回的行号前缀（`   1→`）不是文件内容，不要写进 old_string。"
    }

    fn prompt_contribution(&self) -> agent_core::PromptContribution {
        agent_core::PromptContribution {
            snippet: "在文件中做精确的字符串替换，一次调用可含多笔不相邻的修改（edits[]）。",
            guidelines: &[
                "修改文件前必须先 Read 它，old_string 要以 Read 返回的原文为准。",
                "多笔不相邻的修改用一次 edits[] 调用，不要逐笔 Edit；每笔都匹配原始内容，区间不得重叠。",
                "old_string 不唯一时补充上下文使其唯一；不要只给一行让模型猜位置。",
            ],
        }
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "文件路径，相对于工作区根目录"
                },
                "edits": {
                    "type": "array",
                    "description": "一次调用多笔替换。每笔都匹配原始文件内容而非前一笔的结果，区间不得重叠",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": {
                                "type": "string",
                                "description": "要被替换的原文，必须逐字符一致且在文件中唯一"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "替换后的新内容。留空表示删除 old_string"
                            }
                        },
                        "required": ["old_string", "new_string"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "edits"],
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

        if args.edits.is_empty() {
            return Err(ToolError::recoverable(
                "edits[] 不能为空：至少提供一笔 old_string + new_string。",
            ));
        }

        let edits = args
            .edits
            .iter()
            .map(|i| EditSpec {
                old: i.old_string.clone(),
                new: i.new_string.clone(),
            })
            .collect::<Vec<_>>();

        if edits.iter().any(|e| e.old == e.new) {
            return Err(ToolError::recoverable(
                "某笔替换的 old_string 和 new_string 完全相同，这次编辑没有任何效果。",
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

        let updated = apply_edits(&original, &edits, &shown)?;

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

/// 一笔待应用的替换。
struct EditSpec {
    old: String,
    new: String,
}

/// 把所有替换应用到原始内容上。
///
/// 每笔都在同一份 `original` 上定位（互不干扰），区间两两不得重叠；
/// 全部定位成功后从后往前替换，避免偏移互相影响。
fn apply_edits(original: &str, edits: &[EditSpec], shown: &str) -> Result<String, ToolError> {
    // (start, end, new, fuzzy) 列表；fuzzy 表示该区间经归一化匹配命中
    let mut located: Vec<(usize, usize, &str, bool)> = Vec::new();

    for (index, edit) in edits.iter().enumerate() {
        for (start, end, fuzzy) in locate_all(original, &edit.old, shown, index)? {
            located.push((start, end, &edit.new, fuzzy));
        }
    }

    check_no_overlap(&located, shown)?;

    // 从后往前应用，区间互不重叠因此偏移安全
    let mut updated = original.to_string();
    for (start, end, new, _) in located.iter().rev() {
        updated.replace_range(*start..*end, new);
    }

    let fuzzy_count = located.iter().filter(|(_, _, _, fuzzy)| *fuzzy).count();
    if fuzzy_count > 0 {
        tracing::debug!(
            "`{shown}` 有 {fuzzy_count} 笔替换经模糊匹配，模型给的内容与原文有字符差异"
        );
    }

    Ok(updated)
}

/// 定位一笔替换的全部区间，返回 `(start, end, 是否模糊命中)` 列表。
///
/// - 精确匹配唯一 → 那一个区间
/// - 精确匹配多处 → 报错（edits 模式每笔必须唯一，不提供 replace_all）
/// - 精确匹配不到 → 回退到归一化模糊匹配（至多一个区间）
fn locate_all(
    haystack: &str,
    old: &str,
    shown: &str,
    index: usize,
) -> Result<Vec<(usize, usize, bool)>, ToolError> {
    let label = if index == 0 {
        "old_string".to_string()
    } else {
        format!("edits[{index}].old_string")
    };

    let exact_count = haystack.matches(old).count();

    if exact_count == 0 {
        // 精确匹配不到：回退到逐字符归一化的模糊匹配
        if let Some((start, end, _)) = locate_fuzzy(haystack, old) {
            return Ok(vec![(start, end, true)]);
        }
        return Err(ToolError::recoverable(format!(
            "在 `{shown}` 中找不到 {label}。{}",
            mismatch_hint(haystack, old)
        )));
    }

    if exact_count > 1 {
        return Err(ToolError::recoverable(format!(
            "{label} 在 `{shown}` 中出现了 {exact_count} 次，无法确定改哪一处。\
             请在前后补充更多上下文使其唯一，或把多处替换拆成互不重叠的几笔。"
        )));
    }

    // 恰好一个匹配
    let start = haystack.find(old).expect("计数为 1 则必然可找到");
    Ok(vec![(start, start + old.len(), false)])
}

/// 归一化后的模糊定位。逐字符映射不改变字符数量，因此可以把
/// 归一化串里的位置映射回原文的字节区间。
fn locate_fuzzy(haystack: &str, old: &str) -> Option<(usize, usize, bool)> {
    if old.is_empty() {
        return None;
    }
    let hay_norm: Vec<char> = haystack.chars().map(normalize_char).collect();
    let old_norm: Vec<char> = old.chars().map(normalize_char).collect();
    if old_norm.len() > hay_norm.len() {
        return None;
    }
    let char_start = hay_norm
        .windows(old_norm.len())
        .position(|w| w == old_norm)?;

    let hay_chars: Vec<char> = haystack.chars().collect();
    let byte_start = hay_chars[..char_start]
        .iter()
        .map(|c| c.len_utf8())
        .sum::<usize>();
    let byte_end = byte_start
        + hay_chars[char_start..char_start + old_norm.len()]
            .iter()
            .map(|c| c.len_utf8())
            .sum::<usize>();

    Some((byte_start, byte_end, true))
}

/// 把容易打错的全角字符、弯引号、破折号、特殊空格归一到 ASCII。
///
/// 全部是单字符到单字符的映射 —— 模糊匹配要靠「字符数不变」把位置
/// 映射回原文，任何增删字符的变换（如 NFKC 拆字）都会破坏这一点。
fn normalize_char(c: char) -> char {
    match c {
        // 弯引号 → 直引号
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        // 各种连字符/破折号 → ASCII 连字符
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        // NBSP / 各种宽空格 / 全角空格 → 普通空格
        '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
        // 全角 ASCII 区（！～）→ 半角
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

/// 检查多笔替换的区间是否重叠。
fn check_no_overlap(located: &[(usize, usize, &str, bool)], shown: &str) -> Result<(), ToolError> {
    for i in 0..located.len() {
        for j in (i + 1)..located.len() {
            let a = located[i];
            let b = located[j];
            let overlap = a.0 < b.1 && b.0 < a.1;
            if overlap {
                return Err(ToolError::recoverable(format!(
                    "`{shown}` 中的两笔替换区间重叠，无法确定先后。\
                     请把互相靠近的修改合并成同一笔替换。"
                )));
            }
        }
    }
    Ok(())
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

    fn spec(old: &str, new: &str) -> EditSpec {
        EditSpec {
            old: old.to_string(),
            new: new.to_string(),
        }
    }

    #[test]
    fn replaces_unique_match() {
        let got = apply_edits("a\nb\nc\n", &[spec("b", "B")], "f.rs").expect("唯一匹配应当成功");
        assert_eq!(got, "a\nB\nc\n");
    }

    #[test]
    fn rejects_ambiguous_match() {
        let err = apply_edits("x\nx\n", &[spec("x", "y")], "f.rs").expect_err("多处匹配必须报错");
        let message = err.to_string();
        assert!(message.contains("出现了 2 次"), "{message}");
        assert!(
            message.contains("唯一") || message.contains("上下文"),
            "错误消息该给出解法：{message}"
        );
    }

    #[test]
    fn reports_missing_match() {
        let err = apply_edits("a\n", &[spec("zzz", "b")], "f.rs").expect_err("找不到必须报错");
        assert!(err.to_string().contains("找不到"));
    }

    /// 缩进不一致是最常见的失败原因，提示要点出这一点。
    ///
    /// 注意必须用多行样例：单行的 `let x = 1;` 本来就是 `    let x = 1;`
    /// 的子串，替换会直接成功（缩进被保留，行为是对的）。
    #[test]
    fn hints_about_indentation() {
        let haystack = "    let x = 1;\n    let y = 2;\n";
        let old = "let x = 1;\nlet y = 2;";
        let err =
            apply_edits(haystack, &[spec(old, "z")], "f.rs").expect_err("缩进不同应当匹配失败");
        assert!(err.to_string().contains("缩进"), "{err}");
    }

    /// 不带缩进的单行仍应正常替换，且保留原有缩进。
    #[test]
    fn single_line_match_keeps_indentation() {
        let got = apply_edits(
            "    let x = 1;\n",
            &[spec("let x = 1;", "let x = 2;")],
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
        let err = apply_edits(haystack, &[spec(old, "x")], "f.rs").expect_err("应当匹配失败");
        assert!(err.to_string().contains("首行"), "{err}");
    }

    #[test]
    fn deletes_when_new_string_empty() {
        let got = apply_edits("a\nb\nc\n", &[spec("b\n", "")], "f.rs").expect("删除应当成功");
        assert_eq!(got, "a\nc\n");
    }

    // ---------- 多笔替换 ----------

    /// 一次调用多笔替换：每笔都在原始内容上定位，结果互不干扰。
    #[test]
    fn applies_multiple_disjoint_edits() {
        let got = apply_edits("a\nb\nc\nd\n", &[spec("a", "A"), spec("c", "C")], "f.rs")
            .expect("不相邻的多笔替换应当成功");
        assert_eq!(got, "A\nb\nC\nd\n");
    }

    /// 多笔替换都匹配原始内容：即使某笔的 old 与另一笔的 new 相同，也不影响定位。
    #[test]
    fn edits_match_original_not_incrementally() {
        let got = apply_edits(
            "foo bar\n",
            &[spec("bar", "baz"), spec("foo", "bar")],
            "f.rs",
        )
        .expect("每笔都匹配原始内容");
        assert_eq!(
            got, "bar baz\n",
            "第二笔的 old=bar 应命中原始 bar，而非第一笔刚写的"
        );
    }

    /// 区间重叠必须报错 —— 模型该合并相邻修改而不是给出互相打架的区间。
    #[test]
    fn rejects_overlapping_edits() {
        let err = apply_edits(
            "hello world\n",
            &[spec("hello", "hi"), spec("hello world", "bye")],
            "f.rs",
        )
        .expect_err("重叠区间必须报错");
        assert!(err.to_string().contains("重叠"), "{err}");
    }

    // ---------- 模糊匹配 ----------

    /// 弯引号被模型打成直引号时，归一化后仍能命中，且替换生效。
    #[test]
    fn fuzzy_matches_smart_quotes() {
        let haystack = "她说：“你好”。\n";
        let got = apply_edits(haystack, &[spec("\"你好\"", "再见")], "f.rs")
            .expect("弯引号差异应当被归一化命中");
        // 被替换的是整个目标区间，弯引号随原文区间一起被换掉
        assert_eq!(got, "她说：再见。\n");
    }

    /// 全角 ASCII 与半角的差异应当被归一化命中。
    #[test]
    fn fuzzy_matches_fullwidth_ascii() {
        let haystack = "版本：１．０\n";
        let got = apply_edits(haystack, &[spec("1.0", "2.0")], "f.rs")
            .expect("全角数字差异应当被归一化命中");
        assert_eq!(got, "版本：2.0\n");
    }

    /// NBSP 与普通空格的差异应当被归一化命中。
    #[test]
    fn fuzzy_matches_nbsp() {
        let haystack = "a\u{00A0}b\n";
        let got = apply_edits(haystack, &[spec("a b", "a c")], "f.rs")
            .expect("NBSP 差异应当被归一化命中");
        assert_eq!(got, "a c\n");
    }

    /// 归一化后仍找不到时保持报错。
    #[test]
    fn fuzzy_falls_back_to_error_when_still_missing() {
        let err = apply_edits("abc\n", &[spec("xyz", "q")], "f.rs")
            .expect_err("归一化后仍找不到必须报错");
        assert!(err.to_string().contains("找不到"), "{err}");
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
                    json!({
                        "path": name,
                        "edits": [{ "old_string": old, "new_string": new }]
                    }),
                    &self.ctx(),
                )
                .await
        }

        async fn edit_many(
            &self,
            name: &str,
            edits: Vec<EditItem>,
        ) -> Result<ToolOutcome, ToolError> {
            self.tool
                .execute(json!({ "path": name, "edits": edits }), &self.ctx())
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

    /// 一次多笔替换真实落到磁盘上。
    #[tokio::test]
    async fn applies_multiple_edits_to_disk() {
        let f = Fixture::new();
        let path = f.write_file("multi.txt", "one two\nthree four\n");
        f.mark_read(&path);

        let edits = vec![
            EditItem {
                old_string: "one".into(),
                new_string: "1".into(),
            },
            EditItem {
                old_string: "four".into(),
                new_string: "4".into(),
            },
        ];
        f.edit_many("multi.txt", edits)
            .await
            .expect("多笔替换应当成功");

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "1 two\nthree 4\n",
            "两笔替换应各自生效"
        );
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

    /// edits 数组为空时直接报错 —— 没有要改的东西，不该碰文件。
    #[tokio::test]
    async fn rejects_empty_edits() {
        let f = Fixture::new();
        let path = f.write_file("a.txt", "hello\n");
        f.mark_read(&path);

        let err = f
            .tool
            .execute(json!({ "path": "a.txt", "edits": [] }), &f.ctx())
            .await
            .expect_err("空 edits 必须报错");
        assert!(err.to_string().contains("不能为空"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "hello\n",
            "报错时不该动文件"
        );
    }
}
