//! 界面展示模型与终端原生格式化。
//!
//! 包含：
//! - `UiEntry`、`AssistantSegment`、`ToolCard` 展示条目结构
//! - 工具输出提炼与 Diff 结构化渲染
//! - 历史区折行与 ANSI 原生着色流式输出
//! - 多行输入自适应与视觉光标精确定位
//! - 斜杠命令浮层行格式化

use std::io::{self, Write};
use std::path::Path;

use agent_core::events::DiffTag;
use agent_core::session::Status;
use agent_core::{Session, ToolResultBody};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::composer::Composer;
use crate::markdown::StreamingMarkdown;
use crate::slash::SlashCommandDef;

/// 展示用条目 —— 轮次期间由事件驱动，与 Session 互为投影。
pub enum UiEntry {
    System,
    User {
        text: String,
    },
    Summary {
        text: String,
    },
    /// 系统提示、帮助信息或斜杠命令反馈。
    Notice {
        text: String,
    },
    Assistant {
        reasoning: String,
        segments: Vec<AssistantSegment>,
        /// 轮次结束时的事件是否标记了取消/错误。
        status: Option<Status>,
    },
    /// 轮次被取消且没有任何产出时（空 assistant 被会话丢弃），
    /// 单独保留一行取消标记 —— 用户需要知道自己点了停止。
    CancelledMarker,
}

pub enum AssistantSegment {
    /// 思维链思考片段。
    Reasoning(String),
    /// 文本片段：流式期间用增量渲染器，提交的行已按 Markdown 着色。
    Text(StreamingMarkdown),
    /// 工具调用卡片。
    Tool(ToolCard),
}

pub struct ToolCard {
    pub call_id: String,
    pub name: String,
    pub args: serde_json::Value,
    pub preview: String,
    pub progress: String,
    pub result: Option<ToolResultBody>,
    pub ok: Option<bool>,
    pub duration_ms: u64,
}

impl ToolCard {
    pub fn new(call_id: String, name: String) -> Self {
        Self {
            call_id,
            name,
            args: serde_json::Value::Null,
            preview: String::new(),
            progress: String::new(),
            result: None,
            ok: None,
            duration_ms: 0,
        }
    }

    /// 提炼并格式化核心参数，避免展示难看的裸 JSON。
    pub fn formatted_header(&self) -> (String, String) {
        match self.name.as_str() {
            "Bash" => {
                let cmd = self
                    .args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                ("$".to_string(), cmd.to_string())
            }
            "Edit" => {
                let path = self.args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                ("Edit:".to_string(), path.to_string())
            }
            "Write" => {
                let path = self.args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                ("Write:".to_string(), path.to_string())
            }
            "Read" => {
                let path = self.args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let offset = self.args.get("offset").and_then(|v| v.as_u64());
                let limit = self.args.get("limit").and_then(|v| v.as_u64());
                let extra = match (offset, limit) {
                    (Some(o), Some(l)) => format!(" (L{}-{})", o, o + l),
                    (Some(o), None) => format!(" (L{}+)", o),
                    _ => String::new(),
                };
                ("Read:".to_string(), format!("{path}{extra}"))
            }
            "Grep" => {
                let query = self
                    .args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let path = self.args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let extra = if path.is_empty() {
                    String::new()
                } else {
                    format!(" in {path}")
                };
                ("Grep:".to_string(), format!("\"{query}\"{extra}"))
            }
            "Glob" => {
                let pat = self
                    .args
                    .get("pattern")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                ("Glob:".to_string(), pat.to_string())
            }
            _ => {
                let args_str = if self.args.is_null() {
                    String::new()
                } else if let Some(s) = self.args.as_str() {
                    s.to_string()
                } else {
                    self.args.to_string()
                };
                (format!("{}:", self.name), args_str)
            }
        }
    }

    /// 格式化耗时展示。
    pub fn formatted_duration(&self) -> String {
        if self.duration_ms == 0 {
            String::new()
        } else if self.duration_ms < 1000 {
            format!("{}ms", self.duration_ms)
        } else {
            format!("{:.1}s", (self.duration_ms as f64) / 1000.0)
        }
    }

    /// 单行摘要：优先取结构化结果，退化到进度/预览。
    pub fn summary_line(&self) -> String {
        if let Some(result) = &self.result {
            return match result {
                ToolResultBody::Text { content, .. } => {
                    let c = content.trim();
                    let mut s = c.chars().take(60).collect::<String>();
                    if c.chars().count() > 60 {
                        s.push('…');
                    }
                    s
                }
                ToolResultBody::Diff {
                    path,
                    added,
                    removed,
                    ..
                } => {
                    format!("+{added} -{removed} {path}")
                }
                ToolResultBody::Exec {
                    command, exit_code, ..
                } => {
                    format!("`{command}` → {:?}", exit_code)
                }
                ToolResultBody::Error { message } => format!("错误：{message}"),
            };
        }
        if !self.progress.is_empty() {
            let mut s = self.progress.trim().chars().take(60).collect::<String>();
            if self.progress.chars().count() > 60 {
                s.push('…');
            }
            return s;
        }
        self.preview.clone()
    }
}

/// 历史行（已渲染产物）。
pub struct HistoryRow {
    pub content: Line<'static>,
    pub style: Style,
}

/// 把包含多个 Span 的 Line 按照换行符与最大显示宽度折行成单物理行列表。
///
/// 保留所有 Span 颜色与修饰，CJK 字符按双宽计算。每一个返回的 `Line` 在终端上
/// 严格占据恰好 1 行物理行，彻底避免外部渲染控件内部折行导致高度算错与底部裁剪。
pub fn wrap_line(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![line];
    }

    let base_style = line.style;
    let has_trailing_newline = line
        .spans
        .last()
        .map(|s| s.content.ends_with('\n'))
        .unwrap_or(false);

    let mut physical_lines = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in line.spans {
        let style = span.style;
        let content = span.content;

        let mut sub_lines = content.split('\n').peekable();
        while let Some(sub_str) = sub_lines.next() {
            let mut remaining = sub_str;
            while !remaining.is_empty() {
                let space = max_width.saturating_sub(current_width);
                if space == 0 {
                    physical_lines.push(Line::from(std::mem::take(&mut current_spans)));
                    current_width = 0;
                    continue;
                }

                let mut take_bytes = 0;
                let mut take_width = 0;

                for (byte_idx, ch) in remaining.char_indices() {
                    let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if take_width + ch_w > space {
                        break;
                    }
                    take_width += ch_w;
                    take_bytes = byte_idx + ch.len_utf8();
                }

                if take_bytes == 0 {
                    if current_width > 0 {
                        physical_lines.push(Line::from(std::mem::take(&mut current_spans)));
                        current_width = 0;
                        continue;
                    } else {
                        let first_ch = remaining.chars().next().unwrap();
                        take_bytes = first_ch.len_utf8();
                        take_width = unicode_width::UnicodeWidthChar::width(first_ch).unwrap_or(0);
                    }
                }

                let piece = remaining[..take_bytes].to_string();
                current_spans.push(Span::styled(piece, style));
                current_width += take_width;
                remaining = &remaining[take_bytes..];
            }

            if sub_lines.peek().is_some() {
                physical_lines.push(Line::from(std::mem::take(&mut current_spans)));
                current_width = 0;
            }
        }
    }

    if has_trailing_newline && current_spans.is_empty() {
        physical_lines.push(Line::from(Vec::<Span<'static>>::new()));
    }

    if !current_spans.is_empty() || physical_lines.is_empty() {
        physical_lines.push(Line::from(current_spans));
    }

    physical_lines
        .into_iter()
        .map(|l| l.patch_style(base_style))
        .collect()
}

/// 简单 spinner：按帧号轮换一组字符。
pub fn spinner(frame: u64) -> &'static str {
    const FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
    FRAMES[(frame as usize) % FRAMES.len()]
}

/// 自动压缩后，把展示模型里最老的若干条目替换成摘要气泡。
pub fn apply_compaction(entries: &mut Vec<UiEntry>, removed_entries: usize, summary: &str) {
    let mut removed = 0;
    let mut target: Option<usize> = None;
    for (i, entry) in entries.iter().enumerate() {
        if !matches!(entry, UiEntry::System) {
            removed += 1;
            if removed == removed_entries {
                target = Some(i + 1);
                break;
            }
        }
    }
    if let Some(end) = target {
        let mut new_entries = vec![UiEntry::Summary {
            text: summary.to_string(),
        }];
        new_entries.extend(entries.drain(end..));
        *entries = new_entries;
    }
}

/// 从会话重建展示条目（轮次结束、启动恢复时调用）。
pub fn rebuild_entries(session: Option<&Session>, ui_entries: &mut Vec<UiEntry>) {
    // 会话把空的 assistant 条目丢掉了，但用户需要看到取消标记：
    // 重建前记下尾部是否有 CancelledMarker，重建后补回去。
    let had_cancel_marker = matches!(ui_entries.last(), Some(UiEntry::CancelledMarker));

    let mut entries = Vec::new();
    if let Some(session) = session {
        for entry in &session.entries {
            match entry {
                agent_core::session::Entry::System { .. } => entries.push(UiEntry::System),
                agent_core::session::Entry::User { text } => {
                    entries.push(UiEntry::User { text: text.clone() })
                }
                agent_core::session::Entry::Summary { text } => {
                    entries.push(UiEntry::Summary { text: text.clone() })
                }
                agent_core::session::Entry::Assistant {
                    segments,
                    reasoning,
                    status,
                    ..
                } => {
                    let mut ui_segments = Vec::new();
                    for seg in segments {
                        match seg {
                            agent_core::session::Segment::Reasoning { text } => {
                                ui_segments.push(AssistantSegment::Reasoning(text.clone()));
                            }
                            agent_core::session::Segment::Text { text } => {
                                let mut renderer = StreamingMarkdown::default();
                                renderer.push(text);
                                renderer.finalize();
                                ui_segments.push(AssistantSegment::Text(renderer));
                            }
                            agent_core::session::Segment::Tool { call } => {
                                ui_segments.push(AssistantSegment::Tool(ToolCard {
                                    call_id: call.call_id.clone(),
                                    name: call.name.clone(),
                                    args: call.args.clone(),
                                    preview: call.preview.clone(),
                                    progress: String::new(),
                                    result: Some(call.ui.clone()),
                                    ok: Some(call.ok),
                                    duration_ms: call.duration_ms,
                                }));
                            }
                        }
                    }
                    entries.push(UiEntry::Assistant {
                        reasoning: reasoning.clone(),
                        segments: ui_segments,
                        status: Some(*status),
                    });
                }
            }
        }
    }
    *ui_entries = entries;
    if had_cancel_marker && !matches!(ui_entries.last(), Some(UiEntry::CancelledMarker)) {
        ui_entries.push(UiEntry::CancelledMarker);
    }
}

fn render_reasoning_lines(text: &str, show_tool_details: bool, rows: &mut Vec<HistoryRow>) {
    if text.is_empty() {
        return;
    }
    if show_tool_details {
        rows.push(HistoryRow {
            content: Line::from(vec![Span::styled(
                "◌ 思考过程：",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )]),
            style: Style::default(),
        });
        for r_line in text.lines() {
            rows.push(HistoryRow {
                content: Line::from(format!("  │ {r_line}")),
                style: Style::default().fg(Color::DarkGray),
            });
        }
    } else {
        let mut r = text.chars().take(80).collect::<String>();
        if text.chars().count() > 80 {
            r.push('…');
        }
        rows.push(HistoryRow {
            content: Line::from(format!("◌ {r}")),
            style: Style::default().fg(Color::DarkGray),
        });
    }
}

/// 从展示模型投影出未折行的逻辑历史行。
pub fn build_history_rows(
    entries: &[UiEntry],
    show_tool_details: bool,
    frame: u64,
) -> Vec<HistoryRow> {
    let mut rows = Vec::new();
    for entry in entries {
        match entry {
            UiEntry::System => {}
            UiEntry::User { text } => {
                let mut lines = text.lines();
                if let Some(first) = lines.next() {
                    rows.push(HistoryRow {
                        content: Line::from(format!("❯ {first}")),
                        style: Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    });
                    for rest in lines {
                        rows.push(HistoryRow {
                            content: Line::from(format!("  {rest}")),
                            style: Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        });
                    }
                } else {
                    rows.push(HistoryRow {
                        content: Line::from("❯ "),
                        style: Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    });
                }
            }
            UiEntry::Summary { text } => {
                rows.push(HistoryRow {
                    content: Line::from(format!("⟳ {text}")),
                    style: Style::default().fg(Color::Magenta),
                });
            }
            UiEntry::Notice { text } => {
                for line in text.lines() {
                    rows.push(HistoryRow {
                        content: Line::from(format!("ℹ {line}")),
                        style: Style::default().fg(Color::Cyan),
                    });
                }
            }
            UiEntry::CancelledMarker => {
                rows.push(HistoryRow {
                    content: Line::from("✕ 已取消"),
                    style: Style::default().fg(Color::Red),
                });
            }
            UiEntry::Assistant {
                reasoning,
                segments,
                status,
            } => {
                let has_reasoning_segment = segments
                    .iter()
                    .any(|s| matches!(s, AssistantSegment::Reasoning(_)));
                // 兼容旧会话：若 segments 里没有单独的思考片段但 reasoning 字段有值，在头部输出一次
                if !has_reasoning_segment && !reasoning.is_empty() {
                    render_reasoning_lines(reasoning, show_tool_details, &mut rows);
                }
                for seg in segments {
                    match seg {
                        AssistantSegment::Reasoning(r) => {
                            render_reasoning_lines(r, show_tool_details, &mut rows);
                        }
                        AssistantSegment::Text(renderer) => {
                            // 已提交行直接复用；尾部行推入待折行。
                            for line in renderer.lines() {
                                rows.push(HistoryRow {
                                    content: line.clone(),
                                    style: Style::default(),
                                });
                            }
                            if renderer.has_pending() {
                                rows.push(HistoryRow {
                                    content: Line::from(renderer.tail().to_string()),
                                    style: Style::default(),
                                });
                            }
                        }
                        AssistantSegment::Tool(card) => {
                            let (icon, color) = match card.ok {
                                Some(true) => ("✓", Color::Green),
                                Some(false) => ("✗", Color::Red),
                                None => (spinner(frame), Color::Yellow),
                            };
                            let (prefix, detail) = card.formatted_header();
                            let duration = card.formatted_duration();
                            let summary = card.summary_line();

                            let mut spans = vec![
                                Span::styled(
                                    format!("{icon} "),
                                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    prefix,
                                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                                ),
                            ];
                            if !detail.is_empty() {
                                spans.push(Span::raw(" "));
                                spans.push(Span::styled(
                                    detail,
                                    Style::default()
                                        .fg(Color::White)
                                        .add_modifier(Modifier::BOLD),
                                ));
                            }
                            if !duration.is_empty() {
                                spans.push(Span::raw(" "));
                                spans.push(Span::styled(
                                    format!("({duration})"),
                                    Style::default().fg(Color::DarkGray),
                                ));
                            }
                            if !summary.is_empty() {
                                spans.push(Span::styled(
                                    " — ",
                                    Style::default().fg(Color::DarkGray),
                                ));
                                spans.push(Span::styled(
                                    summary,
                                    Style::default().fg(if card.ok == Some(false) {
                                        Color::Red
                                    } else {
                                        Color::DarkGray
                                    }),
                                ));
                            }
                            rows.push(HistoryRow {
                                content: Line::from(spans),
                                style: Style::default(),
                            });

                            if card.ok.is_none() && !card.progress.is_empty() {
                                let last_prog =
                                    card.progress.lines().last().unwrap_or(&card.progress);
                                rows.push(HistoryRow {
                                    content: Line::from(format!("  │ {last_prog}")),
                                    style: Style::default().fg(Color::Yellow),
                                });
                            }

                            // 详细输出与 Diff 结构化审查呈现
                            if let Some(result) = &card.result {
                                match result {
                                    ToolResultBody::Diff {
                                        path,
                                        hunks,
                                        added,
                                        removed,
                                    } => {
                                        // 无论是否开启全局详细模式，Diff 结构化信息都提供清晰的可视化呈现
                                        rows.push(HistoryRow {
                                            content: Line::from(format!(
                                                "  ┌── Diff: {path} (+{added} -{removed}) ──"
                                            )),
                                            style: Style::default().fg(Color::Cyan),
                                        });
                                        for hunk in hunks {
                                            let old_start = hunk
                                                .lines
                                                .iter()
                                                .find_map(|l| l.old_line)
                                                .unwrap_or(1);
                                            let old_count = hunk
                                                .lines
                                                .iter()
                                                .filter(|l| l.tag != DiffTag::Ins)
                                                .count();
                                            let new_start = hunk
                                                .lines
                                                .iter()
                                                .find_map(|l| l.new_line)
                                                .unwrap_or(1);
                                            let new_count = hunk
                                                .lines
                                                .iter()
                                                .filter(|l| l.tag != DiffTag::Del)
                                                .count();
                                            let hunk_header = format!(
                                                "@@ -{old_start},{old_count} +{new_start},{new_count} @@"
                                            );
                                            rows.push(HistoryRow {
                                                content: Line::from(vec![
                                                    Span::styled(
                                                        "  │ ",
                                                        Style::default().fg(Color::Cyan),
                                                    ),
                                                    Span::styled(
                                                        hunk_header,
                                                        Style::default()
                                                            .fg(Color::Cyan)
                                                            .add_modifier(Modifier::DIM),
                                                    ),
                                                ]),
                                                style: Style::default(),
                                            });
                                            for line in &hunk.lines {
                                                let mut spans = Vec::new();
                                                match line.tag {
                                                    DiffTag::Ins => {
                                                        let new_no = line
                                                            .new_line
                                                            .map(|n| format!("{n:3} "))
                                                            .unwrap_or_else(|| "    ".into());
                                                        spans.push(Span::styled(
                                                            format!("  + {new_no}│ "),
                                                            Style::default().fg(Color::Green),
                                                        ));
                                                        for seg in &line.segments {
                                                            let mut s =
                                                                Style::default().fg(Color::Green);
                                                            if seg.emphasis {
                                                                s = s.add_modifier(
                                                                    Modifier::BOLD
                                                                        | Modifier::UNDERLINED,
                                                                );
                                                            }
                                                            spans.push(Span::styled(
                                                                seg.text.clone(),
                                                                s,
                                                            ));
                                                        }
                                                    }
                                                    DiffTag::Del => {
                                                        let old_no = line
                                                            .old_line
                                                            .map(|n| format!("{n:3} "))
                                                            .unwrap_or_else(|| "    ".into());
                                                        spans.push(Span::styled(
                                                            format!("  - {old_no}│ "),
                                                            Style::default().fg(Color::Red),
                                                        ));
                                                        for seg in &line.segments {
                                                            let mut s =
                                                                Style::default().fg(Color::Red);
                                                            if seg.emphasis {
                                                                s = s.add_modifier(
                                                                    Modifier::BOLD
                                                                        | Modifier::UNDERLINED,
                                                                );
                                                            }
                                                            spans.push(Span::styled(
                                                                seg.text.clone(),
                                                                s,
                                                            ));
                                                        }
                                                    }
                                                    DiffTag::Eq => {
                                                        let old_no = line
                                                            .old_line
                                                            .map(|n| format!("{n:3} "))
                                                            .unwrap_or_else(|| "    ".into());
                                                        spans.push(Span::styled(
                                                            format!("    {old_no}│ "),
                                                            Style::default().fg(Color::DarkGray),
                                                        ));
                                                        for seg in &line.segments {
                                                            spans.push(Span::styled(
                                                                seg.text.clone(),
                                                                Style::default()
                                                                    .fg(Color::DarkGray),
                                                            ));
                                                        }
                                                    }
                                                }
                                                rows.push(HistoryRow {
                                                    content: Line::from(spans),
                                                    style: Style::default(),
                                                });
                                            }
                                        }
                                        rows.push(HistoryRow {
                                            content: Line::from("  └──"),
                                            style: Style::default().fg(Color::Cyan),
                                        });
                                    }
                                    ToolResultBody::Exec {
                                        command,
                                        exit_code,
                                        output,
                                        timed_out,
                                        killed,
                                        ..
                                    } => {
                                        let should_show =
                                            show_tool_details || card.ok == Some(false);
                                        if should_show && !output.trim().is_empty() {
                                            let frame_color = if card.ok == Some(false) {
                                                Color::Red
                                            } else {
                                                Color::Blue
                                            };
                                            let status_tag = if *timed_out {
                                                " (超时)"
                                            } else if *killed {
                                                " (已中断)"
                                            } else {
                                                ""
                                            };
                                            rows.push(HistoryRow {
                                                content: Line::from(format!(
                                                    "  ┌── 输出: {command} (退出码: {exit_code:?}{status_tag}) ──"
                                                )),
                                                style: Style::default().fg(frame_color),
                                            });
                                            for out_line in output.lines().take(40) {
                                                rows.push(HistoryRow {
                                                    content: Line::from(format!("  │ {out_line}")),
                                                    style: Style::default().fg(
                                                        if card.ok == Some(false) {
                                                            Color::Red
                                                        } else {
                                                            Color::DarkGray
                                                        },
                                                    ),
                                                });
                                            }
                                            rows.push(HistoryRow {
                                                content: Line::from("  └──"),
                                                style: Style::default().fg(frame_color),
                                            });
                                        }
                                    }
                                    ToolResultBody::Text { content, truncated }
                                        if show_tool_details =>
                                    {
                                        let tag = if *truncated { "（已截断）" } else { "" };
                                        rows.push(HistoryRow {
                                            content: Line::from(format!("  ┌── 详情{tag} ──")),
                                            style: Style::default().fg(Color::DarkGray),
                                        });
                                        for t_line in content.lines().take(30) {
                                            rows.push(HistoryRow {
                                                content: Line::from(format!("  │ {t_line}")),
                                                style: Style::default().fg(Color::DarkGray),
                                            });
                                        }
                                        rows.push(HistoryRow {
                                            content: Line::from("  └──"),
                                            style: Style::default().fg(Color::DarkGray),
                                        });
                                    }
                                    ToolResultBody::Error { message } => {
                                        rows.push(HistoryRow {
                                            content: Line::from(vec![
                                                Span::styled(
                                                    "  ✕ 错误：",
                                                    Style::default()
                                                        .fg(Color::Red)
                                                        .add_modifier(Modifier::BOLD),
                                                ),
                                                Span::styled(
                                                    message.clone(),
                                                    Style::default().fg(Color::Red),
                                                ),
                                            ]),
                                            style: Style::default(),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if let Some(status) = status {
                    let (icon, color) = match status {
                        Status::Done => ("✓", Color::Green),
                        Status::Cancelled => ("✕", Color::Red),
                        Status::Error => ("✗", Color::Red),
                    };
                    rows.push(HistoryRow {
                        content: Line::from(format!("{icon} 轮次结束")),
                        style: Style::default().fg(color),
                    });
                }
            }
        }
    }
    rows
}

/// 单个视觉输入行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualInputLine {
    pub prompt: &'static str,
    pub text: String,
}

/// 计算输入文本在给定可用宽度下的视觉折行与光标的视觉行列坐标。
pub fn compute_visual_input(
    composer: &Composer,
    max_text_width: usize,
) -> (Vec<VisualInputLine>, usize, usize) {
    let lines_vec = composer.lines();
    let max_text_width = max_text_width.max(1);

    let mut visual_lines = Vec::new();
    let mut cursor_visual_row = 0;
    let mut cursor_visual_col = 0;

    let mut char_count_before_line = 0;

    for (logical_row, logical_line) in lines_vec.iter().enumerate() {
        let is_first_line = logical_row == 0;
        let line_len = logical_line.len();
        let line_start_byte = char_count_before_line;
        let line_end_byte = line_start_byte + line_len;

        if logical_line.is_empty() {
            let v_idx = visual_lines.len();
            let prompt = if is_first_line { "❯ " } else { "  " };
            visual_lines.push(VisualInputLine {
                prompt,
                text: String::new(),
            });
            if composer.cursor >= line_start_byte && composer.cursor <= line_end_byte {
                cursor_visual_row = v_idx;
                cursor_visual_col = 0;
            }
        } else {
            let mut current_sub_line = String::new();
            let mut current_sub_width = 0;
            let mut first_sub = true;

            for (byte_idx, ch) in logical_line.char_indices() {
                let absolute_byte = line_start_byte + byte_idx;
                let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);

                if !current_sub_line.is_empty() && current_sub_width + ch_w > max_text_width {
                    let prompt = if is_first_line && first_sub {
                        "❯ "
                    } else {
                        "  "
                    };
                    visual_lines.push(VisualInputLine {
                        prompt,
                        text: std::mem::take(&mut current_sub_line),
                    });
                    current_sub_width = 0;
                    first_sub = false;
                }

                if composer.cursor == absolute_byte {
                    cursor_visual_row = visual_lines.len();
                    cursor_visual_col = current_sub_width;
                }

                current_sub_line.push(ch);
                current_sub_width += ch_w;
            }

            // 处理位于该逻辑行末尾的光标
            if composer.cursor == line_end_byte {
                cursor_visual_row = visual_lines.len();
                cursor_visual_col = current_sub_width;
            }

            let prompt = if is_first_line && first_sub {
                "❯ "
            } else {
                "  "
            };
            visual_lines.push(VisualInputLine {
                prompt,
                text: current_sub_line,
            });
        }

        char_count_before_line = line_end_byte + 1; // +1 为换行符 '\n'
    }

    if visual_lines.is_empty() {
        visual_lines.push(VisualInputLine {
            prompt: "❯ ",
            text: String::new(),
        });
    }

    (visual_lines, cursor_visual_row, cursor_visual_col)
}

/// 将 Ratatui 的 Line 转换为带有 ANSI 转义序列的字符串（用于终端原生输出）。
pub fn line_to_ansi(line: &Line<'_>) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for span in &line.spans {
        let style = span.style;
        let mut codes: Vec<String> = Vec::new();

        if style.add_modifier.contains(Modifier::BOLD) {
            codes.push("1".into());
        }
        if style.add_modifier.contains(Modifier::DIM) {
            codes.push("2".into());
        }
        if style.add_modifier.contains(Modifier::ITALIC) {
            codes.push("3".into());
        }
        if style.add_modifier.contains(Modifier::UNDERLINED) {
            codes.push("4".into());
        }

        if let Some(fg) = style.fg {
            match fg {
                Color::Reset => codes.push("39".into()),
                Color::Black => codes.push("30".into()),
                Color::Red => codes.push("31".into()),
                Color::Green => codes.push("32".into()),
                Color::Yellow => codes.push("33".into()),
                Color::Blue => codes.push("34".into()),
                Color::Magenta => codes.push("35".into()),
                Color::Cyan => codes.push("36".into()),
                Color::Gray => codes.push("37".into()),
                Color::DarkGray => codes.push("90".into()),
                Color::LightRed => codes.push("91".into()),
                Color::LightGreen => codes.push("92".into()),
                Color::LightYellow => codes.push("93".into()),
                Color::LightBlue => codes.push("94".into()),
                Color::LightMagenta => codes.push("95".into()),
                Color::LightCyan => codes.push("96".into()),
                Color::White => codes.push("97".into()),
                Color::Rgb(r, g, b) => codes.push(format!("38;2;{r};{g};{b}")),
                Color::Indexed(i) => codes.push(format!("38;5;{i}")),
            }
        }

        if let Some(bg) = style.bg {
            match bg {
                Color::Reset => codes.push("49".into()),
                Color::Black => codes.push("40".into()),
                Color::Red => codes.push("41".into()),
                Color::Green => codes.push("42".into()),
                Color::Yellow => codes.push("43".into()),
                Color::Blue => codes.push("44".into()),
                Color::Magenta => codes.push("45".into()),
                Color::Cyan => codes.push("46".into()),
                Color::Gray => codes.push("47".into()),
                Color::DarkGray => codes.push("100".into()),
                Color::LightRed => codes.push("101".into()),
                Color::LightGreen => codes.push("102".into()),
                Color::LightYellow => codes.push("103".into()),
                Color::LightBlue => codes.push("104".into()),
                Color::LightMagenta => codes.push("105".into()),
                Color::LightCyan => codes.push("106".into()),
                Color::White => codes.push("107".into()),
                Color::Rgb(r, g, b) => codes.push(format!("48;2;{r};{g};{b}")),
                Color::Indexed(i) => codes.push(format!("48;5;{i}")),
            }
        }

        if codes.is_empty() {
            out.push_str(&span.content);
        } else {
            let _ = write!(out, "\x1b[{}m{}\x1b[0m", codes.join(";"), span.content);
        }
    }
    out
}

/// 将单个 UiEntry 转化为带样式 Line 列表。
pub fn format_entry_to_lines(
    entry: &UiEntry,
    show_tool_details: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let raw_rows = build_history_rows(std::slice::from_ref(entry), show_tool_details, 0);
    let mut lines = Vec::new();
    for row in raw_rows {
        let style = row.style;
        let wrapped = wrap_line(row.content, width);
        for line in wrapped {
            lines.push(line.patch_style(style));
        }
    }
    lines
}

/// 格式化并直接将单个 UiEntry 打印到终端主屏
pub fn print_entry(entry: &UiEntry, show_tool_details: bool, width: usize) {
    let lines = format_entry_to_lines(entry, show_tool_details, width);
    let mut stdout = io::stdout();
    for line in lines {
        let ansi = line_to_ansi(&line);
        let _ = write!(stdout, "{ansi}\r\n");
    }
    let _ = stdout.flush();
}

/// 打印欢迎 Banner 到终端主屏。
pub fn print_banner(workspace: &Path, model: &str, session_id: &str) {
    let ws_name = workspace
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| workspace.display().to_string());
    let session_prefix: String = session_id.chars().take(8).collect();
    let mut stdout = io::stdout();
    let _ = write!(
        stdout,
        "\r\n\x1b[1;36mBUUCTF Coding Agent\x1b[0m \x1b[90m(v0.1.0)\x1b[0m\r\n\
         \x1b[90m工作区: \x1b[0m\x1b[1m{}\x1b[0m \x1b[90m│ 模型: \x1b[0m\x1b[1m{}\x1b[0m \x1b[90m│ 会话: [{}]\x1b[0m\r\n\
         \x1b[90m输入问题开始对话，Ctrl+T 切换多行模式，Ctrl+G 外部编辑器，/help 查看命令，Ctrl+C 退出。\x1b[0m\r\n\
         \x1b[90m────────────────────────────────────────────────────────────────\x1b[0m\r\n",
        ws_name, model, session_prefix
    );
    let _ = stdout.flush();
}

/// 将斜杠命令补全弹窗渲染为行字符串列表。
pub fn format_completion_lines(
    completions: &[&SlashCommandDef],
    selected_idx: usize,
    max_width: usize,
) -> Vec<String> {
    if completions.is_empty() {
        return Vec::new();
    }
    let max_items = 6;
    let current_idx = selected_idx.min(completions.len().saturating_sub(1));
    let start_idx = if current_idx >= max_items {
        current_idx + 1 - max_items
    } else {
        0
    };
    let end_idx = (start_idx + max_items).min(completions.len());

    let mut lines = Vec::new();
    let border_w = max_width.saturating_sub(48).min(20);
    lines.push(format!(
        "\x1b[36m┌─ ⚡ 命令补全 (Tab 补全 / ↑↓ 选择 / Enter 确认) {}\x1b[0m",
        "─".repeat(border_w)
    ));
    for (i, cmd) in completions[start_idx..end_idx].iter().enumerate() {
        let real_idx = start_idx + i;
        let is_selected = real_idx == current_idx;
        let args_pad = if !cmd.args.is_empty() {
            format!("{:<8} ", cmd.args)
        } else {
            "         ".to_string()
        };
        if is_selected {
            lines.push(format!(
                "\x1b[36m│\x1b[0m \x1b[7;1;36m ▸ {:<10} {args_pad}│ {}\x1b[0m",
                cmd.name, cmd.description
            ));
        } else {
            lines.push(format!(
                "\x1b[36m│\x1b[0m   \x1b[1;37m{:<10}\x1b[0m \x1b[33m{args_pad}\x1b[0m\x1b[90m│ {}\x1b[0m",
                cmd.name, cmd.description
            ));
        }
    }
    lines.push(format!(
        "\x1b[36m└{}\x1b[0m",
        "─".repeat(max_width.saturating_sub(2).min(68))
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::events::{DiffHunk, DiffLine, DiffSegment, DiffTag};

    /// 输入内容与头部信息必须在格式化与视觉计算中正确呈现。
    #[test]
    fn input_and_header_are_visible_in_render() {
        let mut composer = Composer::new();
        composer.insert_text("正在输入的内容");
        let (v_lines, _, _) = compute_visual_input(&composer, 60);
        assert!(v_lines.iter().any(|l| l.text.contains("正在输入的内容")));

        let entry = UiEntry::User {
            text: "正在输入的内容".into(),
        };
        let lines = format_entry_to_lines(&entry, false, 60);
        let joined: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert!(joined.iter().any(|l| l.contains("正在输入的内容")));
    }

    /// 超宽的历史行格式化为回滚行时必须折行而不是截断。
    #[test]
    fn long_history_lines_wrap_instead_of_truncating() {
        let long = format!("{}END", "a".repeat(190));
        let entry = UiEntry::User { text: long };
        let lines = format_entry_to_lines(&entry, false, 60);
        assert!(lines.len() > 1, "超长行必须折行");
        let joined: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("END")),
            "超宽行折行后尾部必须完整可见：{joined:?}"
        );
    }

    /// 历史条目格式化为行集合，支持带样式的回滚区写入。
    #[test]
    fn entry_formats_into_scrollback_lines() {
        let entry = UiEntry::User {
            text: "你好世界".into(),
        };
        let lines = format_entry_to_lines(&entry, false, 60);
        assert!(!lines.is_empty());
        let joined: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        assert!(joined.iter().any(|s| s.contains("你好世界")));
    }

    /// wrap_line 正确根据宽度对包含中文或长文本的行进行硬换行，且每行物理宽度不超限。
    #[test]
    fn wrap_line_splits_by_width_and_preserves_spans() {
        let line = Line::from(vec![
            Span::raw("hello "),
            Span::raw("world this is a long text"),
        ]);
        let wrapped = wrap_line(line, 10);
        assert!(wrapped.len() > 1);
        for l in &wrapped {
            let width: usize = l
                .spans
                .iter()
                .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                .sum();
            assert!(width <= 10, "每行物理宽度必须 <= 10，实际为 {width}");
        }
    }

    /// 单行超宽文本输入自动视觉折行，光标位于正确行与列。
    #[test]
    fn input_area_wraps_long_lines_and_calculates_cursor() {
        let mut composer = Composer::new();
        // 60 个字符，当限制宽度为 20 时应折成 3 行
        let long_str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ01234567";
        composer.insert_text(long_str);
        assert_eq!(composer.cursor, 60);

        let (visual_lines, cursor_row, cursor_col) = compute_visual_input(&composer, 20);
        assert_eq!(visual_lines.len(), 3, "60 字符在宽度 20 下应折为 3 行");
        assert_eq!(visual_lines[0].prompt, "❯ ");
        assert_eq!(visual_lines[1].prompt, "  ");
        assert_eq!(visual_lines[2].prompt, "  ");
        assert_eq!(cursor_row, 2, "光标在末尾应处于第 3 行（索引 2）");
        assert_eq!(cursor_col, 20, "最后一行包含 20 字符，光标应在列 20");

        // 测试包含换行符的多行
        let mut multiline_composer = Composer::new();
        multiline_composer.insert_text("first\nsecond");
        let (ml_lines, ml_row, ml_col) = compute_visual_input(&multiline_composer, 50);
        assert_eq!(ml_lines.len(), 2);
        assert_eq!(ml_row, 1);
        assert_eq!(ml_col, 6);
    }

    /// spinner 按帧轮换且周期循环。
    #[test]
    fn spinner_cycles() {
        assert_eq!(spinner(0), "⠋");
        assert_eq!(spinner(8), "⠋", "第 9 帧应回到第一格");
        assert_ne!(spinner(0), spinner(1));
    }

    /// Diff 审查应当输出带有加减号与行号的结构化对比行。
    #[test]
    fn diff_rendering_produces_structured_hunks() {
        let hunk = DiffHunk {
            lines: vec![
                DiffLine {
                    tag: DiffTag::Del,
                    old_line: Some(10),
                    new_line: None,
                    segments: vec![DiffSegment {
                        text: "old code".into(),
                        emphasis: true,
                    }],
                },
                DiffLine {
                    tag: DiffTag::Ins,
                    old_line: None,
                    new_line: Some(10),
                    segments: vec![DiffSegment {
                        text: "new code".into(),
                        emphasis: true,
                    }],
                },
            ],
        };

        let mut card = ToolCard::new("c1".into(), "Edit".into());
        card.ok = Some(true);
        card.result = Some(ToolResultBody::Diff {
            path: "src/lib.rs".into(),
            hunks: vec![hunk],
            added: 1,
            removed: 1,
        });

        let entries = vec![UiEntry::Assistant {
            reasoning: String::new(),
            segments: vec![AssistantSegment::Tool(card)],
            status: Some(Status::Done),
        }];

        let rows = build_history_rows(&entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();

        assert!(
            joined.iter().any(|l| l.contains("Diff: src/lib.rs")),
            "应包含 Diff 标题：{joined:?}"
        );
        assert!(
            joined
                .iter()
                .any(|l| l.contains("-") && l.contains("old code")),
            "应包含被删除的代码行：{joined:?}"
        );
        assert!(
            joined
                .iter()
                .any(|l| l.contains("+") && l.contains("new code")),
            "应包含新插入的代码行：{joined:?}"
        );
    }

    /// Diff 审查输出包含 @@ Hunk 标头。
    #[test]
    fn diff_rendering_shows_hunk_header() {
        let hunk = DiffHunk {
            lines: vec![
                DiffLine {
                    tag: DiffTag::Del,
                    old_line: Some(10),
                    new_line: None,
                    segments: vec![DiffSegment {
                        text: "old code".into(),
                        emphasis: true,
                    }],
                },
                DiffLine {
                    tag: DiffTag::Ins,
                    old_line: None,
                    new_line: Some(10),
                    segments: vec![DiffSegment {
                        text: "new code".into(),
                        emphasis: true,
                    }],
                },
            ],
        };

        let mut card = ToolCard::new("c1".into(), "Edit".into());
        card.ok = Some(true);
        card.duration_ms = 45;
        card.result = Some(ToolResultBody::Diff {
            path: "src/lib.rs".into(),
            hunks: vec![hunk],
            added: 1,
            removed: 1,
        });

        let entries = vec![UiEntry::Assistant {
            reasoning: String::new(),
            segments: vec![AssistantSegment::Tool(card)],
            status: Some(Status::Done),
        }];

        let rows = build_history_rows(&entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();

        assert!(
            joined.iter().any(|l| l.contains("@@ -10,1 +10,1 @@")),
            "应包含 @@ -10,1 +10,1 @@ Hunk 标头：{joined:?}"
        );
    }

    /// 工具卡片显示提炼后的命令参数、耗时及状态图标，告别裸 JSON。
    #[test]
    fn tool_card_displays_formatted_header_and_duration() {
        let mut card = ToolCard::new("c1".into(), "Bash".into());
        card.args = serde_json::json!({ "command": "cargo test" });
        card.ok = Some(true);
        card.duration_ms = 1250;
        card.result = Some(ToolResultBody::Exec {
            command: "cargo test".into(),
            exit_code: Some(0),
            output: "all passed".into(),
            truncated: false,
            timed_out: false,
            killed: false,
        });

        let entries = vec![UiEntry::Assistant {
            reasoning: String::new(),
            segments: vec![AssistantSegment::Tool(card)],
            status: Some(Status::Done),
        }];

        let rows = build_history_rows(&entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();
        assert!(
            joined
                .iter()
                .any(|l| l.contains("$") && l.contains("cargo test") && l.contains("1.2s")),
            "Bash 卡片应包含 $、命令名与格式化耗时：{joined:?}"
        );
    }

    /// 工具详情切换能够控制 Exec 输出的展开与折叠。
    #[test]
    fn tool_details_toggle_controls_exec_output() {
        let mut card = ToolCard::new("c2".into(), "Bash".into());
        card.ok = Some(true);
        card.result = Some(ToolResultBody::Exec {
            command: "echo test".into(),
            exit_code: Some(0),
            output: "test output line\nsecond line".into(),
            truncated: false,
            timed_out: false,
            killed: false,
        });

        let entries = vec![UiEntry::Assistant {
            reasoning: String::new(),
            segments: vec![AssistantSegment::Tool(card)],
            status: Some(Status::Done),
        }];

        // 折叠模式下
        let rows_collapsed = build_history_rows(&entries, false, 0);
        let joined_c: Vec<String> = rows_collapsed
            .iter()
            .map(|r| r.content.to_string())
            .collect();
        assert!(
            !joined_c.iter().any(|l| l.contains("test output line")),
            "折叠模式下不应展开输出全文"
        );

        // 展开模式下
        let rows_expanded = build_history_rows(&entries, true, 0);
        let joined_e: Vec<String> = rows_expanded
            .iter()
            .map(|r| r.content.to_string())
            .collect();
        assert!(
            joined_e.iter().any(|l| l.contains("test output line")),
            "展开模式下应包含详细输出：{joined_e:?}"
        );
    }
}
