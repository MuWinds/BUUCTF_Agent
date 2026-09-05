//! Markdown → ratatui `Line` 的渲染。
//!
//! 参考 codex 的约束（AGENTS.md 架构约束 2 的 TUI 侧对应物）：
//! **流式期间不做完整 Markdown 解析**。完整解析只在「新行提交」时
//! 对定稿前缀做一次并缓存，未闭合的最后一行保持纯文本 ——
//! 这正是 codex `StreamingRender` 的 stable/tail 两区域模型的简化版。

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// 流式文本的增量渲染器。
///
/// 提交策略：只有**新行终止**的文本才参与完整 Markdown 渲染，
/// 未闭合的尾部行保持纯文本。这样每次 delta 到来时，稳定前缀
/// 的渲染结果直接复用，只有最后一行需要重算 —— 与 codex 的
/// stable region / tail region 同构，只是粒度从「块」降到「行」。
#[derive(Debug, Default, Clone)]
pub struct StreamingMarkdown {
    /// 稳定前缀：按新行边界提交、已渲染的行。
    lines: Vec<Line<'static>>,
    /// 未闭合的尾部行原文（无 `\n`）。
    tail: String,
}

impl StreamingMarkdown {
    /// 追加一段文本增量。
    pub fn push(&mut self, delta: &str) {
        self.tail.push_str(delta);

        // 循环提交新行之前的所有完整行；未闭合的最后一行留在 tail 里。
        while let Some(newline_idx) = self.tail.find('\n') {
            let committed = self.tail[..=newline_idx].to_string();
            self.tail = self.tail[newline_idx + 1..].to_string();

            // 定稿行参与完整 Markdown 渲染；空行保留为分隔。
            if committed.trim().is_empty() {
                self.lines.push(Line::from(""));
            } else {
                let rendered = render(&committed, None);
                self.lines.extend(rendered.lines);
            }
        }
    }

    /// 提交结束：把残留的尾部行也渲染进去（轮次结束、消息定稿时调用）。
    pub fn finalize(&mut self) {
        if self.tail.is_empty() {
            return;
        }
        let tail = std::mem::take(&mut self.tail);
        if tail.trim().is_empty() {
            self.lines.push(Line::from(""));
        } else {
            let rendered = render(&tail, None);
            self.lines.extend(rendered.lines);
        }
    }

    /// 当前全部已提交行。
    pub fn lines(&self) -> &[Line<'static>] {
        &self.lines
    }

    /// 未提交的尾部行原文。
    pub fn tail(&self) -> &str {
        &self.tail
    }

    /// 是否还有未提交内容。
    pub fn has_pending(&self) -> bool {
        !self.tail.is_empty()
    }
}

/// 一次性渲染整段 Markdown（历史回放、定稿消息用）。
///
/// `width` 为 None 时不做折行。
pub fn render(markdown: &str, width: Option<usize>) -> Rendered {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);

    let mut lines = Vec::new();
    let mut paragraph: Vec<Span<'static>> = Vec::new();
    let mut in_code = false;
    let mut code_lines: Vec<String> = Vec::new();
    let mut inline_bold = false;
    let mut link_depth = 0usize;

    let mut current_lang: Option<String> = None;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_paragraph(&mut paragraph, &mut lines, width);
                in_code = true;
                code_lines.clear();
                current_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        let l = lang.trim().to_string();
                        if l.is_empty() {
                            None
                        } else {
                            Some(l)
                        }
                    }
                    pulldown_cmark::CodeBlockKind::Indented => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                if in_code {
                    let lang_tag = current_lang.as_deref().unwrap_or("code");
                    lines.push(Line::from(vec![
                        Span::styled("╭── ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            lang_tag.to_string(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            " ──────────────────────────────",
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                    for raw in &code_lines {
                        lines.push(highlight_code_line(raw, current_lang.as_deref()));
                    }
                    lines.push(Line::from(vec![Span::styled(
                        "╰─────────────────────────────────",
                        Style::default().fg(Color::DarkGray),
                    )]));
                    in_code = false;
                    current_lang = None;
                }
            }
            Event::Text(t) if in_code => code_lines.push(t.to_string()),
            Event::Text(t) => {
                let style = if inline_bold {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                paragraph.push(Span::styled(t.to_string(), style));
            }
            Event::Code(t) => {
                paragraph.push(Span::styled(
                    t.to_string(),
                    Style::default().fg(Color::Cyan),
                ));
            }
            Event::Start(Tag::Strong) => inline_bold = true,
            Event::End(TagEnd::Strong) => inline_bold = false,
            Event::Start(Tag::Link { dest_url, .. }) => {
                paragraph.push(Span::styled(
                    "[",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                link_depth += 1;
                let _ = dest_url;
            }
            Event::End(TagEnd::Link) => {
                paragraph.push(Span::styled(
                    "]",
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                link_depth = link_depth.saturating_sub(1);
            }
            Event::SoftBreak => paragraph.push(Span::raw(" ")),
            Event::HardBreak => flush_paragraph(&mut paragraph, &mut lines, width),
            Event::End(TagEnd::Paragraph) => flush_paragraph(&mut paragraph, &mut lines, width),
            Event::Start(Tag::Heading { level, .. }) => {
                flush_paragraph(&mut paragraph, &mut lines, width);
                let marker = "#".repeat(level as usize);
                paragraph.push(Span::styled(marker, heading_style(level as u8)));
            }
            Event::End(TagEnd::Heading(_)) => flush_paragraph(&mut paragraph, &mut lines, width),
            Event::Start(Tag::Item) => paragraph.push(Span::raw("• ")),
            Event::Start(Tag::BlockQuote(_)) => {
                paragraph.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
            }
            _ => {}
        }
    }
    flush_paragraph(&mut paragraph, &mut lines, width);

    Rendered { lines }
}

/// 渲染产物。
#[derive(Debug, Clone)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
}

fn heading_style(level: u8) -> Style {
    match level {
        1 => Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
    }
}

/// 把攒下的行内 span 落成一行（按宽度折行），然后清空。
fn flush_paragraph(
    paragraph: &mut Vec<Span<'static>>,
    lines: &mut Vec<Line<'static>>,
    width: Option<usize>,
) {
    if paragraph.is_empty() {
        return;
    }
    let text: String = paragraph.iter().map(|s| s.content.as_ref()).collect();
    if let Some(width) = width {
        for chunk in textwrap::wrap(&text, width) {
            lines.push(Line::from(chunk.to_string()));
        }
    } else {
        lines.push(Line::from(std::mem::take(paragraph)));
    }
    paragraph.clear();
}

/// 为单行代码做轻量词法着色，无需引入重型语法高亮库。
fn highlight_code_line(line: &str, lang: Option<&str>) -> Line<'static> {
    let lang_lower = lang.map(|s| s.to_ascii_lowercase());
    // 针对 diff 特殊处理
    if lang_lower.as_deref() == Some("diff") {
        if line.starts_with('+') {
            return Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::Green)),
            ]);
        } else if line.starts_with('-') {
            return Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::Red)),
            ]);
        } else if line.starts_with('@') {
            return Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::DarkGray)),
                Span::styled(line.to_string(), Style::default().fg(Color::Cyan)),
            ]);
        }
    }

    let mut spans = vec![Span::styled("│ ", Style::default().fg(Color::DarkGray))];
    let mut chars = line.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        // 注释 // 或 #
        if (c == '/' && line[i..].starts_with("//"))
            || (c == '#' && lang_lower.as_deref() != Some("rust"))
        {
            spans.push(Span::styled(
                line[i..].to_string(),
                Style::default().fg(Color::DarkGray),
            ));
            break;
        }

        // 字符串 "..." 或 '...'
        if c == '"' || c == '\'' {
            let quote = c;
            let start = i;
            let mut escaped = false;
            let mut end = line.len();
            for (j, next_c) in chars.by_ref() {
                if escaped {
                    escaped = false;
                } else if next_c == '\\' {
                    escaped = true;
                } else if next_c == quote {
                    end = j + next_c.len_utf8();
                    break;
                }
            }
            spans.push(Span::styled(
                line[start..end].to_string(),
                Style::default().fg(Color::LightGreen),
            ));
            continue;
        }

        // 标识符或关键字
        if c.is_alphabetic() || c == '_' {
            let start = i;
            let mut end = start + c.len_utf8();
            while let Some(&(next_i, next_c)) = chars.peek() {
                if next_c.is_alphanumeric() || next_c == '_' {
                    chars.next();
                    end = next_i + next_c.len_utf8();
                } else {
                    break;
                }
            }
            let word = &line[start..end];
            let is_keyword = matches!(
                word,
                "fn" | "let"
                    | "mut"
                    | "pub"
                    | "struct"
                    | "enum"
                    | "impl"
                    | "trait"
                    | "async"
                    | "await"
                    | "if"
                    | "else"
                    | "match"
                    | "return"
                    | "for"
                    | "while"
                    | "loop"
                    | "in"
                    | "use"
                    | "mod"
                    | "crate"
                    | "const"
                    | "static"
                    | "type"
                    | "where"
                    | "self"
                    | "super"
                    | "import"
                    | "export"
                    | "from"
                    | "function"
                    | "class"
                    | "def"
                    | "val"
                    | "var"
                    | "case"
                    | "default"
                    | "switch"
                    | "try"
                    | "catch"
                    | "finally"
                    | "throw"
                    | "new"
                    | "null"
                    | "true"
                    | "false"
                    | "None"
                    | "True"
                    | "False"
            );
            if is_keyword {
                spans.push(Span::styled(
                    word.to_string(),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw(word.to_string()));
            }
            continue;
        }

        // 数字
        if c.is_ascii_digit() {
            let start = i;
            let mut end = start + c.len_utf8();
            while let Some(&(next_i, next_c)) = chars.peek() {
                if next_c.is_ascii_digit() || next_c == '.' || next_c == 'x' || next_c == '_' {
                    chars.next();
                    end = next_i + next_c.len_utf8();
                } else {
                    break;
                }
            }
            spans.push(Span::styled(
                line[start..end].to_string(),
                Style::default().fg(Color::Cyan),
            ));
            continue;
        }

        // 其他字符
        spans.push(Span::raw(c.to_string()));
    }

    Line::from(spans)
}

/// 计算文本的显示宽度（CJK 双宽正确计数）。
pub fn width_of(s: &str) -> usize {
    s.width()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_lines() {
        let out = render(&"a".repeat(100), Some(20));
        assert_eq!(out.lines.len(), 5);
    }

    #[test]
    fn code_block_is_styled() {
        let out = render("```rust\nfn main() {}\n```", Some(80));
        let joined: Vec<String> = out.lines.iter().map(|l| l.to_string()).collect();
        assert!(joined.iter().any(|l| l.contains("fn main()")));
    }

    #[test]
    fn streaming_commits_only_on_newline() {
        let mut sm = StreamingMarkdown::default();
        sm.push("hello ");
        assert_eq!(sm.tail(), "hello ");
        sm.push("world\n");
        assert!(sm.tail().is_empty());
        assert_eq!(sm.lines().len(), 1);
        assert!(sm.lines()[0].to_string().contains("hello world"));
    }

    #[test]
    fn streaming_finalize_flushes_tail() {
        let mut sm = StreamingMarkdown::default();
        sm.push("no newline yet");
        assert!(sm.has_pending());
        sm.finalize();
        assert!(!sm.has_pending());
        assert_eq!(sm.lines().len(), 1);
    }

    #[test]
    fn streaming_renders_markdown_in_committed_lines() {
        let mut sm = StreamingMarkdown::default();
        sm.push("**bold**\nplain\n");
        assert!(!sm.lines().is_empty(), "定稿行应参与 markdown 渲染");
    }

    #[test]
    fn width_counts_cjk_as_double() {
        assert_eq!(width_of("中文"), 4);
        assert_eq!(width_of("ab"), 2);
    }

    /// 代码块应当渲染语言标头并保留代码内容。
    #[test]
    fn code_block_renders_header_and_highlight() {
        let out = render("```rust\nlet x = 42;\n```", Some(80));
        let joined: Vec<String> = out.lines.iter().map(|l| l.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("rust")),
            "代码块标头应包含语言名称：{joined:?}"
        );
        assert!(
            joined.iter().any(|l| l.contains("let x = 42")),
            "代码内容应渲染：{joined:?}"
        );
    }

    /// diff 语法块应当正确输出增删行内容。
    #[test]
    fn diff_block_highlights_changes() {
        let out = render("```diff\n+added\n-removed\n```", Some(80));
        let joined: Vec<String> = out.lines.iter().map(|l| l.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("+added")),
            "diff 块应保留加号行：{joined:?}"
        );
        assert!(
            joined.iter().any(|l| l.contains("-removed")),
            "diff 块应保留减号行：{joined:?}"
        );
    }
}
