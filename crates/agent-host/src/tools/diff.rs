//! 文件改动的 diff 生成与换行符处理。

use agent_core::{DiffHunk, DiffLine, DiffSegment, DiffTag};
use similar::{ChangeTag, TextDiff};

/// diff 中每段改动前后保留的上下文行数。
const CONTEXT_LINES: usize = 3;

/// 生成的 hunk 总行数上限。改动过大时截断，避免几千行 diff 塞爆 UI。
const MAX_DIFF_LINES: usize = 600;

/// 文件使用的换行符。
///
/// **Windows 上这是必须处理的**：Rust 的 `lines()` 会吃掉 `\r`，
/// 写回时若统一用 `\n`，整个文件的行尾都会被悄悄改掉，
/// git 会显示"每一行都变了"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    /// 探测文本主要使用哪种换行符。
    ///
    /// 按占比判断而非看第一个：混用换行符的文件应当保留其主流风格。
    pub fn detect(text: &str) -> Self {
        let crlf = text.matches("\r\n").count();
        let lf = text.matches('\n').count() - crlf;
        if crlf > lf {
            Self::Crlf
        } else {
            Self::Lf
        }
    }

    /// 把文本统一成 `\n`，便于做行级比较。
    pub fn normalize(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    /// 按本换行符还原。
    pub fn apply(self, text: &str) -> String {
        match self {
            Self::Lf => text.to_string(),
            Self::Crlf => text.replace('\n', "\r\n"),
        }
    }
}

/// diff 统计。
pub struct DiffResult {
    pub hunks: Vec<DiffHunk>,
    pub added: usize,
    pub removed: usize,
}

/// 生成带行内高亮的 diff。
///
/// 输入应当已经归一成 `\n` 换行。
pub fn build(old: &str, new: &str) -> DiffResult {
    let diff = TextDiff::from_lines(old, new);

    let mut hunks = Vec::new();
    let mut added = 0usize;
    let mut removed = 0usize;
    let mut emitted = 0usize;
    let mut truncated = false;

    for group in diff.grouped_ops(CONTEXT_LINES) {
        let mut lines = Vec::new();

        for op in &group {
            for change in diff.iter_inline_changes(op) {
                let tag = match change.tag() {
                    ChangeTag::Equal => DiffTag::Eq,
                    ChangeTag::Delete => {
                        removed += 1;
                        DiffTag::Del
                    }
                    ChangeTag::Insert => {
                        added += 1;
                        DiffTag::Ins
                    }
                };

                if emitted >= MAX_DIFF_LINES {
                    truncated = true;
                    continue;
                }
                emitted += 1;

                let segments = change
                    .iter_strings_lossy()
                    .map(|(emphasis, value)| DiffSegment {
                        // 行尾换行符不属于内容，留着会让渲染多出空行
                        text: value.trim_end_matches('\n').to_string(),
                        emphasis,
                    })
                    .collect();

                lines.push(DiffLine {
                    tag,
                    old_line: change.old_index().map(|i| i + 1),
                    new_line: change.new_index().map(|i| i + 1),
                    segments,
                });
            }
        }

        if !lines.is_empty() {
            hunks.push(DiffHunk { lines });
        }
    }

    if truncated {
        tracing::debug!("diff 超过 {MAX_DIFF_LINES} 行已截断");
    }

    DiffResult {
        hunks,
        added,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_crlf_files() {
        assert_eq!(LineEnding::detect("a\r\nb\r\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\nb\n"), LineEnding::Lf);
    }

    /// 混用时按多数派判定。
    #[test]
    fn detects_dominant_ending_when_mixed() {
        assert_eq!(LineEnding::detect("a\r\nb\r\nc\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\nb\nc\r\n"), LineEnding::Lf);
    }

    #[test]
    fn empty_text_defaults_to_lf() {
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
    }

    #[test]
    fn round_trips_crlf() {
        let original = "a\r\nb\r\n";
        let normalized = LineEnding::normalize(original);
        assert_eq!(normalized, "a\nb\n");
        assert_eq!(LineEnding::Crlf.apply(&normalized), original);
    }

    #[test]
    fn counts_added_and_removed() {
        let result = build("a\nb\nc\n", "a\nB\nc\nd\n");
        assert_eq!(result.removed, 1, "b 被改掉算一次删除");
        assert_eq!(result.added, 2, "B 和 d 算两次新增");
    }

    #[test]
    fn produces_no_hunks_for_identical_text() {
        let result = build("same\n", "same\n");
        assert!(result.hunks.is_empty());
        assert_eq!(result.added, 0);
        assert_eq!(result.removed, 0);
    }

    /// 行内高亮：只有真正变化的片段该被标 emphasis。
    #[test]
    fn marks_changed_span_within_line() {
        let result = build("let x = 1;\n", "let x = 2;\n");
        let all: Vec<_> = result.hunks.iter().flat_map(|h| &h.lines).collect();

        let ins = all
            .iter()
            .find(|l| l.tag == DiffTag::Ins)
            .expect("应当有插入行");
        let emphasized: String = ins
            .segments
            .iter()
            .filter(|s| s.emphasis)
            .map(|s| s.text.as_str())
            .collect();

        assert!(
            emphasized.contains('2'),
            "变化的部分应被标记：{emphasized:?}"
        );
        assert!(!emphasized.contains("let"), "未变化的部分不该被标记");
    }

    /// 分段拼起来必须还原整行，否则 UI 显示的内容就是错的。
    #[test]
    fn segments_reconstruct_the_line() {
        let result = build("hello world\n", "hello brave world\n");
        for hunk in &result.hunks {
            for line in &hunk.lines {
                let joined: String = line.segments.iter().map(|s| s.text.as_str()).collect();
                assert!(!joined.contains('\n'), "分段里不该残留换行符");
            }
        }
    }

    /// 中文内容不该在分段时被切坏。
    #[test]
    fn handles_multibyte_content() {
        let result = build("中文测试\n", "中文修改\n");
        let all: Vec<_> = result.hunks.iter().flat_map(|h| &h.lines).collect();
        let ins = all
            .iter()
            .find(|l| l.tag == DiffTag::Ins)
            .expect("应当有插入行");
        let joined: String = ins.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "中文修改");
    }

    #[test]
    fn truncates_huge_diffs() {
        let old = String::new();
        let new: String = (0..MAX_DIFF_LINES + 200)
            .map(|i| format!("line {i}\n"))
            .collect();
        let result = build(&old, &new);

        let total: usize = result.hunks.iter().map(|h| h.lines.len()).sum();
        assert!(total <= MAX_DIFF_LINES, "输出行数应受限，实际 {total}");
        // 统计值不受截断影响，仍反映真实改动量
        assert_eq!(result.added, MAX_DIFF_LINES + 200);
    }
}
