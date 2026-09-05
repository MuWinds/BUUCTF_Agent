//! TUI 应用主体：主循环 + 事件路由 + 轮次状态机。
//!
//! 架构参考 codex 与 Claude Code 的主屏原生流式终端架构：
//! - `composer`：输入框状态与多行编辑
//! - `slash`：斜杠命令与前缀补全
//! - `view`：展示条目、ANSI 格式化与提示符渲染
//! - `app`（本模块）：主事件循环、流式交互与生命周期管理

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use agent_core::session::Status;
use agent_core::{
    turn, AgentEvent, LlmClient, LlmConfig, Registry, Session, ToolEnv, ToolResultBody,
};
use agent_host::tools::ReadRegistry;
use crossterm::cursor;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, ClearType};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::composer::{Composer, CtrlCAction};
use crate::markdown::StreamingMarkdown;
use crate::sink::{ChannelSink, UiMessage};
use crate::slash::{execute_slash_command, slash_completions, SlashContext};
use crate::terminal::Terminal;
use crate::view::{
    apply_compaction, compute_visual_input, format_completion_lines, rebuild_entries, spinner,
    AssistantSegment, ToolCard, UiEntry,
};

/// 一次轮次的可中断状态。
struct ActiveTurn {
    turn_id: String,
    cancel: CancellationToken,
}

/// 主应用状态。
pub struct App {
    config: LlmConfig,
    client: Arc<LlmClient>,
    registry: Arc<Registry>,
    workspace: PathBuf,
    app_data: PathBuf,

    /// 当前会话 ID。
    session_id: String,
    /// 空闲时持有的会话；轮次期间归 agent task 所有。
    session: Option<Session>,
    /// 轮次期间由事件维护的展示模型。
    ui_entries: Vec<UiEntry>,

    // 输入与界面状态
    composer: Composer,
    busy: bool,
    usage: Option<String>,
    frame: u64,
    should_exit: bool,
    /// 是否展开显示工具详细输出（如 Diff 结构化行、完整命令输出等）及思考过程
    show_tool_details: bool,
    /// 斜杠命令补全候选列表的高亮索引
    completion_idx: usize,
    /// 已推入终端主屏的条目数量
    committed_entries: usize,
    /// 当前渲染的提示符区域占用的行数
    rendered_prompt_lines: usize,
    /// 当前光标所处的视觉行索引（用于从任意行精确回跳到首行清屏）
    rendered_cursor_row: usize,
    /// 是否处于多行编辑模式（开启后 Enter 换行，Ctrl+D / Ctrl+S 发送）
    multiline_mode: bool,
    /// 当前是否处于正在显示单行 Spinner 的状态
    status_line_active: bool,
    /// 当前是否正在实时流式输出模型生成的文本（此时 spinner 暂停渲染，避免刷屏覆盖文字）
    is_streaming_text: bool,

    // agent 通道与轮次控制
    rx: mpsc::UnboundedReceiver<UiMessage>,
    /// channel 保活发送端：`App::new` 之后没有任何 agent 在跑，若不持有
    /// sender，rx 端会立即收到关闭信号，主循环的 `recv()` 马上返回 `None`
    /// 而退出 —— 「一晃而过」就是这么来的。轮次开始时由 task 接棒持有。
    keepalive_tx: mpsc::UnboundedSender<UiMessage>,
    active_turn: Option<ActiveTurn>,
    /// 插队等待的输入：忙碌时新消息先取消当前轮次，等完成消息送回
    /// 会话后再开新一轮。
    pending_input: Option<String>,
}

impl App {
    pub fn new(config: LlmConfig, workspace: PathBuf) -> anyhow::Result<Self> {
        let client = Arc::new(LlmClient::new()?);
        let read_registry = Arc::new(ReadRegistry::new());
        let registry = Arc::new(agent_host::tools::registry(read_registry));

        // 会话目录与 Tauri 版一致：`<app_data>/sessions/<workspace>/…`
        let app_data = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("coding-agent");

        let (tx, rx) = mpsc::unbounded_channel();
        let keepalive_tx = tx;

        Ok(Self {
            config,
            client,
            registry,
            workspace,
            app_data,
            session_id: agent_host::persist::new_id(),
            session: None,
            ui_entries: Vec::new(),
            composer: Composer::new(),
            busy: false,
            usage: None,
            frame: 0,
            should_exit: false,
            show_tool_details: false,
            completion_idx: 0,
            committed_entries: 0,
            rendered_prompt_lines: 0,
            rendered_cursor_row: 0,
            multiline_mode: false,
            status_line_active: false,
            is_streaming_text: false,
            rx,
            keepalive_tx,
            active_turn: None,
            pending_input: None,
        })
    }

    /// 主循环。
    pub async fn run(&mut self, tui: &mut Terminal) -> anyhow::Result<bool> {
        // 恢复最近一次会话，并投影为展示条目
        let model = self.config.model.clone();
        let (session, session_id) =
            agent_host::persist::bootstrap(&self.app_data, &self.workspace, &model).await;
        self.session = Some(session);
        self.session_id = session_id;
        self.rebuild_entries();

        // 打印主屏 Banner
        crate::view::print_banner(&self.workspace, &self.config.model, &self.session_id);

        // 打印已恢复的历史条目（如果有）
        let (w, _) = Terminal::size();
        let width = (w as usize).max(20);
        for entry in &self.ui_entries {
            crate::view::print_entry(entry, self.show_tool_details, width);
        }
        self.committed_entries = self.ui_entries.len();

        // 渲染初始交互输入框
        self.render_prompt()?;

        // 动画 tick：100ms 一拍，驱动忙碌时 spinner
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                event = tui.next_event() => {
                    match event {
                        Some(Ok(event)) => self.handle_terminal_event(event).await?,
                        Some(Err(e)) => {
                            tracing::error!("终端事件错误：{e}");
                            break;
                        }
                        None => break,
                    }
                }
                msg = self.rx.recv() => {
                    match msg {
                        Some(msg) => self.handle_ui_message(msg).await,
                        None => break,
                    }
                }
                _ = ticker.tick() => {
                    if self.busy && !self.is_streaming_text {
                        self.frame = self.frame.wrapping_add(1);
                        self.render_busy_status()?;
                    }
                }
            }
            if self.should_exit {
                self.clear_prompt()?;
                self.clear_busy_status()?;
                let mut stdout = io::stdout();
                let _ = write!(stdout, "\r\n\x1b[90m会话已结束，再见！\x1b[0m\r\n");
                let _ = stdout.flush();
                break;
            }
        }

        // 退出前如果还有轮次在跑，取消并等它收尾，避免丢会话。
        if self.busy {
            self.cancel_active().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.persist_if_idle().await;
        Ok(true)
    }

    // 事件处理

    async fn handle_terminal_event(
        &mut self,
        event: crossterm::event::Event,
    ) -> anyhow::Result<()> {
        match event {
            crossterm::event::Event::Key(key) => self.handle_key(key).await?,
            crossterm::event::Event::Paste(text) => {
                let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                self.composer.insert_text(&normalized);
                if !self.busy {
                    self.render_prompt()?;
                }
            }
            crossterm::event::Event::Resize(_, _) if !self.busy => {
                self.render_prompt()?;
            }
            _ => {}
        }
        Ok(())
    }

    /// 渲染底部的交互式多行提示符与斜杠命令补全菜单。
    fn render_prompt(&mut self) -> anyhow::Result<()> {
        let (width, _) = Terminal::size();
        let width = (width as usize).max(20);
        let mut stdout = io::stdout();

        if self.rendered_prompt_lines > 0 {
            if self.rendered_cursor_row > 0 {
                execute!(stdout, cursor::MoveUp(self.rendered_cursor_row as u16))?;
            }
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::FromCursorDown)
            )?;
        }

        let avail_width = width.saturating_sub(4).max(10);
        let (visual_lines, cursor_row, cursor_col) =
            compute_visual_input(&self.composer, avail_width);

        let completions = slash_completions(&self.composer.input, self.composer.cursor);
        let completion_lines = format_completion_lines(&completions, self.completion_idx, width);

        // 多行模式提示或长文本提示
        let hint_line = if self.multiline_mode {
            Some("\x1b[36m└─ [多行模式] Enter 换行 │ Ctrl+S / Ctrl+D 发送 │ Ctrl+T 退出多行\x1b[0m")
        } else if visual_lines.len() > 1 && completion_lines.is_empty() {
            Some("\x1b[90m└─ Enter 发送 │ Shift+Enter / \\+Enter 换行 │ Ctrl+T 切换多行模式\x1b[0m")
        } else {
            None
        };

        let mut total_lines: usize = 0;

        for (i, v_line) in visual_lines.iter().enumerate() {
            let is_last =
                i + 1 == visual_lines.len() && completion_lines.is_empty() && hint_line.is_none();
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::CurrentLine)
            )?;
            let prompt_colored = if self.multiline_mode && i == 0 {
                "\x1b[1;36m❯ \x1b[0m"
            } else {
                "\x1b[1;32m❯ \x1b[0m"
            };
            let prompt = if i == 0 { prompt_colored } else { "  " };
            let _ = write!(stdout, "{}{}", prompt, v_line.text);
            total_lines += 1;
            if !is_last {
                let _ = write!(stdout, "\r\n");
            }
        }

        for (i, c_line) in completion_lines.iter().enumerate() {
            let is_last = i + 1 == completion_lines.len() && hint_line.is_none();
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::CurrentLine)
            )?;
            let _ = write!(stdout, "{c_line}");
            total_lines += 1;
            if !is_last {
                let _ = write!(stdout, "\r\n");
            }
        }

        if let Some(h) = hint_line {
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::CurrentLine)
            )?;
            let _ = write!(stdout, "{h}");
            total_lines += 1;
        }

        let up_distance = total_lines.saturating_sub(1).saturating_sub(cursor_row);
        if up_distance > 0 {
            execute!(stdout, cursor::MoveUp(up_distance as u16))?;
        }
        let prompt_prefix_w =
            crate::markdown::width_of(if cursor_row == 0 { "❯ " } else { "  " });
        let target_col = (prompt_prefix_w + cursor_col).min(width.saturating_sub(1));
        execute!(
            stdout,
            cursor::MoveToColumn(target_col as u16),
            cursor::Show
        )?;

        self.rendered_prompt_lines = total_lines;
        self.rendered_cursor_row = cursor_row;
        stdout.flush()?;
        Ok(())
    }

    /// 清理提示符渲染区域。
    fn clear_prompt(&mut self) -> anyhow::Result<()> {
        if self.rendered_prompt_lines > 0 {
            let mut stdout = io::stdout();
            if self.rendered_cursor_row > 0 {
                execute!(stdout, cursor::MoveUp(self.rendered_cursor_row as u16))?;
            }
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::FromCursorDown)
            )?;
            execute!(stdout, cursor::Hide)?;
            self.rendered_prompt_lines = 0;
            self.rendered_cursor_row = 0;
            stdout.flush()?;
        }
        Ok(())
    }

    /// 获取当前忙碌状态提示文本。
    fn active_status_text(&self) -> String {
        if let Some(UiEntry::Assistant { segments, .. }) = self.ui_entries.last() {
            for seg in segments.iter().rev() {
                if let AssistantSegment::Tool(card) = seg {
                    if card.ok.is_none() {
                        let (prefix, target) = card.formatted_header();
                        let target_short = if target.len() > 40 {
                            format!(
                                "{}…",
                                &target[..target
                                    .char_indices()
                                    .map(|(i, _)| i)
                                    .nth(37)
                                    .unwrap_or(target.len())]
                            )
                        } else {
                            target
                        };
                        return format!("执行工具: {prefix} {target_short}");
                    }
                }
            }
        }
        self.usage
            .clone()
            .unwrap_or_else(|| "正在思考中…".to_string())
    }

    /// 渲染单行忙碌状态 / Spinner。
    fn render_busy_status(&mut self) -> anyhow::Result<()> {
        let mut stdout = io::stdout();
        let spin = spinner(self.frame);
        let status = self.active_status_text();
        execute!(
            stdout,
            cursor::MoveToColumn(0),
            terminal::Clear(ClearType::CurrentLine)
        )?;
        let _ = write!(
            stdout,
            "\x1b[1;33m{spin}\x1b[0m \x1b[90m{status} (按 Ctrl+C 中断)\x1b[0m"
        );
        stdout.flush()?;
        self.status_line_active = true;
        Ok(())
    }

    /// 清理单行忙碌状态。
    fn clear_busy_status(&mut self) -> anyhow::Result<()> {
        if self.status_line_active {
            let mut stdout = io::stdout();
            execute!(
                stdout,
                cursor::MoveToColumn(0),
                terminal::Clear(ClearType::CurrentLine)
            )?;
            stdout.flush()?;
            self.status_line_active = false;
        }
        Ok(())
    }

    /// 打开系统外部编辑器（Notepad 或 $EDITOR）供用户编辑多行长提示词。
    fn open_external_editor(&mut self) -> anyhow::Result<()> {
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| {
                if cfg!(windows) {
                    "notepad".to_string()
                } else {
                    "nano".to_string()
                }
            });

        // 使用无锁独立文件，写入后立即释放句柄，避免 Windows 下文件句柄占用导致记事本报 ERROR_SHARING_VIOLATION 弹出“另存为”
        let draft_path =
            std::env::temp_dir().join(format!("buuctf_agent_draft_{}.txt", std::process::id()));
        std::fs::write(&draft_path, self.composer.input.as_bytes())?;

        // 挂起终端交互模式（恢复光标、关闭 raw mode、弹出键盘增强协议）
        Terminal::pause()?;

        let status = std::process::Command::new(&editor)
            .arg(&draft_path)
            .status();

        // 恢复终端交互模式（进入 raw mode、隐藏光标、重新启用键盘增强与 bracketed-paste）
        Terminal::resume()?;

        if let Ok(s) = status {
            if s.success() {
                if let Ok(content) = std::fs::read_to_string(&draft_path) {
                    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
                    self.composer.input = normalized;
                    self.composer.cursor = self.composer.input.len();
                }
            }
        }
        let _ = std::fs::remove_file(&draft_path);
        self.render_prompt()?;
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        // 键盘增强协议下只处理产生输入的 Press 与 Repeat
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(());
        }
        // Ctrl+C：忙碌时取消轮次；有输入内容时第一次按清空输入框；空输入时连续按两次退出
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if self.busy {
                self.cancel_active().await;
                self.clear_busy_status().ok();
                self.is_streaming_text = false;
                let mut stdout = io::stdout();
                let _ = write!(stdout, "\r\n\x1b[33m[轮次已中断]\x1b[0m\r\n");
                let _ = stdout.flush();
                self.composer.last_ctrl_c = None;
                self.render_prompt().ok();
            } else {
                match self.composer.handle_ctrl_c() {
                    CtrlCAction::ClearedInput => {
                        self.completion_idx = 0;
                        self.render_prompt().ok();
                    }
                    CtrlCAction::PromptConfirm => {
                        let notice = UiEntry::Notice {
                            text: "再按一次 Ctrl+C 退出，或输入 /exit".into(),
                        };
                        let (w, _) = Terminal::size();
                        let width = (w as usize).max(20);
                        crate::view::print_entry(&notice, self.show_tool_details, width);
                        self.ui_entries.push(notice);
                        self.committed_entries = self.ui_entries.len();
                        self.render_prompt().ok();
                    }
                    CtrlCAction::ConfirmExit => {
                        self.should_exit = true;
                        return Ok(());
                    }
                }
            }
            return Ok(());
        }
        // Ctrl+O：切换工具输出与思考过程的详细展示/折叠
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('o') {
            self.show_tool_details = !self.show_tool_details;
            let msg = if self.show_tool_details {
                "已开启详细视图（完整输出与 Diff 审查）"
            } else {
                "已开启精简视图"
            };
            let notice = UiEntry::Notice { text: msg.into() };
            let (w, _) = Terminal::size();
            let width = (w as usize).max(20);
            crate::view::print_entry(&notice, self.show_tool_details, width);
            self.ui_entries.push(notice);
            self.committed_entries = self.ui_entries.len();
            self.render_prompt().ok();
            return Ok(());
        }
        // Esc：忙碌时取消，否则清空输入
        if key.code == KeyCode::Esc {
            if self.busy {
                self.cancel_active().await;
                self.clear_busy_status().ok();
                self.is_streaming_text = false;
                let mut stdout = io::stdout();
                let _ = write!(stdout, "\r\n\x1b[33m[轮次已中断]\x1b[0m\r\n");
                let _ = stdout.flush();
                self.render_prompt().ok();
            } else {
                self.composer.clear();
                self.completion_idx = 0;
                self.render_prompt().ok();
            }
            return Ok(());
        }
        // Ctrl+T：快速切换多行输入模式
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('t') {
            self.multiline_mode = !self.multiline_mode;
            if !self.busy {
                self.render_prompt().ok();
            }
            return Ok(());
        }
        // Ctrl+G：在系统外部文本编辑器中编写长提示词
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g') {
            if !self.busy {
                let _ = self.open_external_editor();
            }
            return Ok(());
        }
        // Ctrl+S 或 Ctrl+D：提交发送输入（特别方便在多行模式下直接发送）
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && (key.code == KeyCode::Char('s') || key.code == KeyCode::Char('d'))
        {
            if !self.busy && !self.composer.input.trim().is_empty() {
                self.send_input().await;
            }
            return Ok(());
        }

        let completions = slash_completions(&self.composer.input, self.composer.cursor);
        let has_completions = !completions.is_empty();

        // Readline 快捷键
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p') => {
                    if has_completions {
                        if self.completion_idx == 0 {
                            self.completion_idx = completions.len().saturating_sub(1);
                        } else {
                            self.completion_idx -= 1;
                        }
                        if !self.busy {
                            self.render_prompt().ok();
                        }
                        return Ok(());
                    }
                }
                KeyCode::Char('n') => {
                    if has_completions {
                        self.completion_idx = (self.completion_idx + 1) % completions.len();
                        if !self.busy {
                            self.render_prompt().ok();
                        }
                        return Ok(());
                    }
                }
                KeyCode::Char('a') => {
                    self.composer.move_to_start();
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
                KeyCode::Char('e') => {
                    self.composer.move_to_end();
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
                KeyCode::Char('u') => {
                    self.composer.clear_to_start();
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
                KeyCode::Char('k') => {
                    self.composer.clear_to_end();
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
                KeyCode::Char('w') => {
                    self.composer.delete_word_backward();
                    self.composer.history_idx = None;
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
                KeyCode::Char('j') => {
                    self.composer.insert_text("\n");
                    self.composer.history_idx = None;
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        // Alt+Backspace 单词退格
        if key.modifiers.contains(KeyModifiers::ALT) && key.code == KeyCode::Backspace {
            self.composer.delete_word_backward();
            self.composer.history_idx = None;
            if !self.busy {
                self.render_prompt().ok();
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Tab => {
                if has_completions {
                    let idx = self.completion_idx.min(completions.len().saturating_sub(1));
                    let cmd = completions[idx];
                    self.composer.input = format!("{} ", cmd.name);
                    self.composer.cursor = self.composer.input.len();
                    self.completion_idx = 0;
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
            }
            KeyCode::BackTab => {
                if has_completions {
                    if self.completion_idx == 0 {
                        self.completion_idx = completions.len().saturating_sub(1);
                    } else {
                        self.completion_idx -= 1;
                    }
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                    return Ok(());
                }
            }
            KeyCode::Enter => {
                if self.multiline_mode {
                    // 多行模式下：Enter 直接换行；Ctrl+Enter 或 Alt+Enter 触发提交发送
                    if key.modifiers.contains(KeyModifiers::ALT)
                        || key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        self.send_input().await;
                    } else {
                        self.composer.insert_text("\n");
                        self.composer.history_idx = None;
                        if !self.busy {
                            self.render_prompt().ok();
                        }
                    }
                } else if key.modifiers.contains(KeyModifiers::ALT)
                    || key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::CONTROL)
                {
                    // 常规模式下：Shift+Enter / Alt+Enter / Ctrl+Enter 换行
                    self.composer.insert_text("\n");
                    self.composer.history_idx = None;
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                } else if self.composer.try_newline_continuation() {
                    // 行尾以 \ 结尾按下 Enter，自动消除 \ 并换行续行
                    self.composer.history_idx = None;
                    if !self.busy {
                        self.render_prompt().ok();
                    }
                } else if has_completions {
                    let idx = self.completion_idx.min(completions.len().saturating_sub(1));
                    let cmd = completions[idx];
                    if self.composer.input.trim() != cmd.name && !cmd.args.is_empty() {
                        self.composer.input = format!("{} ", cmd.name);
                        self.composer.cursor = self.composer.input.len();
                        self.completion_idx = 0;
                        if !self.busy {
                            self.render_prompt().ok();
                        }
                    } else {
                        self.composer.input = cmd.name.to_string();
                        self.composer.cursor = self.composer.input.len();
                        self.completion_idx = 0;
                        self.send_input().await;
                    }
                } else {
                    self.send_input().await;
                }
            }
            KeyCode::Up => {
                if has_completions {
                    if self.completion_idx == 0 {
                        self.completion_idx = completions.len().saturating_sub(1);
                    } else {
                        self.completion_idx -= 1;
                    }
                } else if !self.composer.move_cursor_up() {
                    self.composer.history_up();
                }
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::Down => {
                if has_completions {
                    self.completion_idx = (self.completion_idx + 1) % completions.len();
                } else if !self.composer.move_cursor_down() {
                    self.composer.history_down();
                }
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::Char(c) => {
                if c == '\n' || c == '\r' {
                    if self.multiline_mode
                        || key.modifiers.contains(KeyModifiers::ALT)
                        || key.modifiers.contains(KeyModifiers::SHIFT)
                        || key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        self.composer.insert_text("\n");
                        self.composer.history_idx = None;
                        if !self.busy {
                            self.render_prompt().ok();
                        }
                    } else {
                        self.send_input().await;
                    }
                    return Ok(());
                }
                self.composer.history_idx = None;
                let mut s = String::new();
                s.push(c);
                self.composer.insert_text(&s);
                self.completion_idx = 0;
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::Backspace => {
                self.composer.history_idx = None;
                self.composer.backspace();
                self.completion_idx = 0;
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::Left => {
                self.composer.move_cursor(-1);
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::Right => {
                self.composer.move_cursor(1);
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::Home => {
                self.composer.move_to_start();
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            KeyCode::End => {
                self.composer.move_to_end();
                if !self.busy {
                    self.render_prompt().ok();
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_ui_message(&mut self, msg: UiMessage) {
        match msg {
            UiMessage::Agent(event) => {
                self.stream_agent_event(&event);
                self.apply_agent_event(event);
            }
            UiMessage::TurnFinished { turn_id, session } => {
                let is_current = self
                    .active_turn
                    .as_ref()
                    .is_some_and(|t| t.turn_id == turn_id);
                if is_current {
                    self.session = Some(session);
                    self.rebuild_entries();
                    self.busy = false;
                    self.is_streaming_text = false;
                    self.active_turn = None;
                    self.clear_busy_status().ok();
                    self.committed_entries = self.ui_entries.len();

                    self.persist_if_idle().await;
                    if let Some(text) = self.pending_input.take() {
                        self.start_turn(text);
                    } else {
                        self.render_prompt().ok();
                    }
                }
            }
        }
    }

    /// 将 AgentEvent 实时流式直接打印到终端主屏，保证用户实时看到回复与工具进度，杜绝整屏闪烁。
    fn stream_agent_event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::TurnStart { .. } => {
                self.busy = true;
                self.is_streaming_text = false;
            }
            AgentEvent::AssistantDelta { text, .. } => {
                if self.status_line_active {
                    let _ = self.clear_busy_status();
                }
                self.is_streaming_text = true;
                let mut stdout = io::stdout();
                let formatted = text.replace("\r\n", "\n").replace('\n', "\r\n");
                let _ = write!(stdout, "{formatted}");
                let _ = stdout.flush();
            }
            AgentEvent::ReasoningDelta { text, .. } => {
                if self.show_tool_details {
                    if self.status_line_active {
                        let _ = self.clear_busy_status();
                    }
                    let mut stdout = io::stdout();
                    let formatted = text.replace("\r\n", "\n").replace('\n', "\r\n  \x1b[90m│ ");
                    let _ = write!(stdout, "\x1b[90m{formatted}\x1b[0m");
                    let _ = stdout.flush();
                }
            }
            AgentEvent::ToolCallStart { name, .. } => {
                if self.status_line_active {
                    let _ = self.clear_busy_status();
                }
                self.is_streaming_text = false;
                let mut stdout = io::stdout();
                let _ = write!(
                    stdout,
                    "\r\n\x1b[1;36m◆ 执行工具: \x1b[0m\x1b[1m{name}\x1b[0m"
                );
                let _ = stdout.flush();
            }
            AgentEvent::ToolCallReady { preview, .. } => {
                let mut stdout = io::stdout();
                if !preview.is_empty() {
                    let _ = write!(stdout, " \x1b[90m({preview})\x1b[0m\r\n");
                } else {
                    let _ = write!(stdout, "\r\n");
                }
                let _ = stdout.flush();
            }
            AgentEvent::ToolProgress { chunk, .. } => {
                if self.show_tool_details {
                    if self.status_line_active {
                        let _ = self.clear_busy_status();
                    }
                    let mut stdout = io::stdout();
                    let formatted = chunk
                        .replace("\r\n", "\n")
                        .replace('\n', "\r\n  \x1b[33m│ ");
                    let _ = write!(stdout, "  \x1b[33m│ \x1b[0m{formatted}\r\n");
                    let _ = stdout.flush();
                }
            }
            AgentEvent::ToolResult {
                ok,
                duration_ms,
                result,
                ..
            } => {
                if self.status_line_active {
                    let _ = self.clear_busy_status();
                }
                let mut stdout = io::stdout();
                if *ok {
                    let _ = write!(
                        stdout,
                        "\x1b[32m✔ 工具执行成功\x1b[0m \x1b[90m({duration_ms}ms)\x1b[0m\r\n"
                    );
                } else {
                    let _ = write!(
                        stdout,
                        "\x1b[31m✖ 工具执行失败\x1b[0m \x1b[90m({duration_ms}ms)\x1b[0m\r\n"
                    );
                }
                if self.show_tool_details {
                    match result {
                        ToolResultBody::Exec { output, .. } if !output.trim().is_empty() => {
                            for line in output.lines().take(40) {
                                let _ = write!(stdout, "  \x1b[90m│\x1b[0m {line}\r\n");
                            }
                        }
                        ToolResultBody::Diff {
                            path,
                            added,
                            removed,
                            ..
                        } => {
                            let _ = write!(
                                stdout,
                                "  \x1b[90m│ 改动: {path} (+{added} / -{removed})\x1b[0m\r\n"
                            );
                        }
                        _ => {}
                    }
                }
                let _ = stdout.flush();
            }
            AgentEvent::Error { code, message, .. } => {
                if self.status_line_active {
                    let _ = self.clear_busy_status();
                }
                let mut stdout = io::stdout();
                let _ = write!(stdout, "\r\n\x1b[31m[错误 {code}]: {message}\x1b[0m\r\n");
                let _ = stdout.flush();
            }
            AgentEvent::Retry {
                attempt,
                max_retries,
                retry_after_ms,
                ..
            } => {
                let limit = max_retries
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "∞".into());
                let mut stdout = io::stdout();
                let _ = write!(
                    stdout,
                    "\x1b[33m[重试 {attempt}/{limit}]，{retry_after_ms}ms 后重试…\x1b[0m\r\n"
                );
                let _ = stdout.flush();
            }
            AgentEvent::ContextCompacted {
                removed_entries,
                summary,
                ..
            } => {
                let mut stdout = io::stdout();
                let _ = write!(
                    stdout,
                    "\x1b[90m[上下文已压缩：折叠了 {removed_entries} 条旧消息 —— {summary}]\x1b[0m\r\n"
                );
                let _ = stdout.flush();
            }
            AgentEvent::TurnEnd {
                finish_reason,
                elapsed_ms,
                ..
            } => {
                if self.status_line_active {
                    let _ = self.clear_busy_status();
                }
                self.is_streaming_text = false;
                let mut stdout = io::stdout();
                if finish_reason == "cancelled" {
                    let _ = write!(stdout, "\r\n\x1b[33m[轮次已中断]\x1b[0m\r\n");
                } else if finish_reason == "error" {
                    let _ = write!(
                        stdout,
                        "\r\n\x1b[31m[轮次出错]\x1b[0m \x1b[90m({elapsed_ms}ms)\x1b[0m\r\n"
                    );
                } else {
                    let _ = write!(
                        stdout,
                        "\r\n\x1b[32m✓ 轮次结束\x1b[0m \x1b[90m({elapsed_ms}ms)\x1b[0m\r\n"
                    );
                }
                let _ = stdout.flush();
            }
            _ => {}
        }
    }

    /// 把 `AgentEvent` 应用到展示模型。
    fn apply_agent_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::TurnStart { .. } => {
                self.busy = true;
                self.ui_entries.push(UiEntry::Assistant {
                    reasoning: String::new(),
                    segments: Vec::new(),
                    status: None,
                });
            }
            AgentEvent::AssistantDelta { text, .. } => {
                self.push_text_to_current(&text);
            }
            AgentEvent::ReasoningDelta { text, .. } => {
                self.push_reasoning_to_current(&text);
            }
            AgentEvent::ToolCallStart { call_id, name, .. } => {
                self.push_tool_to_current(ToolCard::new(call_id, name));
            }
            AgentEvent::ToolCallReady {
                call_id,
                args,
                preview,
                ..
            } => {
                self.update_tool(&call_id, |card| {
                    card.args = args;
                    card.preview = preview;
                });
            }
            AgentEvent::ToolProgress { call_id, chunk, .. } => {
                self.update_tool(&call_id, |card| card.progress.push_str(&chunk));
            }
            AgentEvent::ToolResult {
                call_id,
                ok,
                duration_ms,
                result,
                ..
            } => {
                self.update_tool(&call_id, |card| {
                    card.ok = Some(ok);
                    card.duration_ms = duration_ms;
                    card.result = Some(result);
                });
            }
            AgentEvent::ContextCompacted {
                removed_entries,
                summary,
                ..
            } => {
                apply_compaction(&mut self.ui_entries, removed_entries, &summary);
            }
            AgentEvent::Usage {
                total_tokens,
                prompt_tokens,
                completion_tokens,
                context_used,
                context_limit,
                ..
            } => {
                self.usage = Some(format!(
                    "tok {total_tokens}（in {prompt_tokens} / out {completion_tokens}）· 上下文 {context_used}/{context_limit}"
                ));
            }
            AgentEvent::Error { code, message, .. } => {
                tracing::error!("agent 错误 [{code}]：{message}");
                self.usage = Some(format!("错误 [{code}] {message}"));
            }
            AgentEvent::Retry {
                attempt,
                max_retries,
                retry_after_ms,
                ..
            } => {
                let limit = max_retries
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "∞".into());
                tracing::warn!("重试 {attempt}/{limit}，{retry_after_ms}ms 后重试");
                self.usage = Some(format!("重试 {attempt}/{limit}…"));
            }
            AgentEvent::TurnEnd {
                finish_reason,
                elapsed_ms,
                ..
            } => {
                let status = match finish_reason.as_str() {
                    "cancelled" => Status::Cancelled,
                    "error" => Status::Error,
                    _ => Status::Done,
                };
                let empty = matches!(
                    self.ui_entries.last(),
                    Some(UiEntry::Assistant { segments, .. }) if segments.is_empty()
                );
                if matches!(status, Status::Cancelled | Status::Error) && empty {
                    self.ui_entries.push(UiEntry::CancelledMarker);
                } else {
                    self.mark_current_status(status);
                }
                if let Some(usage) = self.usage.clone() {
                    self.usage = Some(format!("{usage} · {elapsed_ms}ms"));
                }
            }
        }
    }

    fn mark_current_status(&mut self, status: Status) {
        if let Some(UiEntry::Assistant { status: s, .. }) = self.ui_entries.last_mut() {
            *s = Some(status);
        }
    }

    fn push_text_to_current(&mut self, text: &str) {
        if let Some(UiEntry::Assistant { segments, .. }) = self.ui_entries.last_mut() {
            match segments.last_mut() {
                Some(AssistantSegment::Text(renderer)) => renderer.push(text),
                _ => {
                    let mut renderer = StreamingMarkdown::default();
                    renderer.push(text);
                    segments.push(AssistantSegment::Text(renderer));
                }
            }
        }
    }

    fn push_reasoning_to_current(&mut self, text: &str) {
        if let Some(UiEntry::Assistant {
            reasoning,
            segments,
            ..
        }) = self.ui_entries.last_mut()
        {
            reasoning.push_str(text);
            match segments.last_mut() {
                Some(AssistantSegment::Reasoning(existing)) => existing.push_str(text),
                _ => segments.push(AssistantSegment::Reasoning(text.to_string())),
            }
        }
    }

    fn push_tool_to_current(&mut self, card: ToolCard) {
        if let Some(UiEntry::Assistant { segments, .. }) = self.ui_entries.last_mut() {
            segments.push(AssistantSegment::Tool(card));
        }
    }

    fn update_tool(&mut self, call_id: &str, f: impl FnOnce(&mut ToolCard)) {
        for entry in self.ui_entries.iter_mut().rev() {
            if let UiEntry::Assistant { segments, .. } = entry {
                for seg in segments.iter_mut().rev() {
                    if let AssistantSegment::Tool(card) = seg {
                        if card.call_id == call_id {
                            f(card);
                            return;
                        }
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // 动作
    // ------------------------------------------------------------------

    async fn send_input(&mut self) {
        let text = self.composer.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer.clear();
        self.completion_idx = 0;
        self.composer.push_history(text.clone());
        self.clear_prompt().ok();

        if text == "/editor" {
            let _ = self.open_external_editor();
            return;
        }

        // 斜杠命令拦截：本地优先调度
        if text.starts_with('/') {
            let mut ctx = SlashContext {
                busy: self.busy,
                config: &mut self.config,
                client: &self.client,
                workspace: &self.workspace,
                app_data: &self.app_data,
                session_id: &mut self.session_id,
                session: &mut self.session,
                ui_entries: &mut self.ui_entries,
                show_tool_details: &mut self.show_tool_details,
                multiline_mode: &mut self.multiline_mode,
                should_exit: &mut self.should_exit,
            };
            execute_slash_command(&text, &mut ctx).await;
            if self.committed_entries > self.ui_entries.len() {
                self.committed_entries = 0;
            }
            let (w, _) = Terminal::size();
            let width = (w as usize).max(20);
            for entry in &self.ui_entries[self.committed_entries..] {
                crate::view::print_entry(entry, self.show_tool_details, width);
            }
            self.committed_entries = self.ui_entries.len();
            if !self.should_exit {
                self.render_prompt().ok();
            }
            return;
        }

        // 忙碌时插队：取消当前轮次，等待它的完成消息把会话送回来后，
        // 再以新消息开新一轮 —— 会话不会丢，顺序也不会乱。
        if self.busy {
            self.cancel_active().await;
            self.pending_input = Some(text);
            return;
        }
        self.start_turn(text);
    }

    fn start_turn(&mut self, text: String) {
        // 取出会话交给 task；轮次期间 UI 用展示模型。
        let mut session = self.session.take().unwrap_or_default();
        if session.is_empty() {
            session.push_system(agent_host::system_prompt(&self.workspace, &self.registry));
        }
        session.push_user(&text);

        let user_entry = UiEntry::User { text };
        let (w, _) = Terminal::size();
        let width = (w as usize).max(20);
        crate::view::print_entry(&user_entry, self.show_tool_details, width);
        self.ui_entries.push(user_entry);
        self.committed_entries = self.ui_entries.len();
        self.busy = true;
        self.render_busy_status().ok();

        let (tx, rx) = mpsc::unbounded_channel();
        self.keepalive_tx = tx.clone();
        self.rx = rx;

        let config = self.config.clone();
        let client = self.client.clone();
        let registry = self.registry.clone();
        let env = ToolEnv {
            workspace_root: self.workspace.clone(),
        };
        let cancel = CancellationToken::new();
        let preempt = Arc::new(AtomicBool::new(false));
        let turn_id = uuid::Uuid::new_v4().to_string();
        self.active_turn = Some(ActiveTurn {
            turn_id: turn_id.clone(),
            cancel: cancel.clone(),
        });

        let app_data = self.app_data.clone();
        let workspace = self.workspace.clone();
        let model = config.model.clone();
        let session_id = self.current_session_id();
        let send_tx = tx.clone();

        tokio::spawn(async move {
            let mut session = session;
            match agent_core::compact::maybe_compact(&client, &config, &mut session).await {
                Ok(Some(compaction)) => {
                    let _ = send_tx.send(UiMessage::Agent(AgentEvent::ContextCompacted {
                        turn_id: turn_id.clone(),
                        removed_entries: compaction.removed_entries,
                        summary: compaction.summary,
                    }));
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("自动压缩失败，继续原样对话：{e}"),
            }

            let mut sink = agent_core::ThrottledSink::new(
                Arc::new(ChannelSink::new(send_tx.clone())),
                turn_id.clone(),
            );
            let outcome = turn::run(
                &client,
                &config,
                &mut session,
                &registry,
                &env,
                &mut sink,
                cancel,
                &preempt,
            )
            .await;
            tracing::info!(finish_reason = %outcome.finish_reason, "轮次结束");

            let _ = agent_host::persist::save(&app_data, &workspace, &model, &session_id, &session)
                .await;
            let _ = send_tx.send(UiMessage::TurnFinished { turn_id, session });
        });
    }

    /// 取消当前轮次。
    async fn cancel_active(&mut self) {
        if let Some(turn) = self.active_turn.as_ref() {
            turn.cancel.cancel();
        }
    }

    /// 空闲时落盘当前会话。
    async fn persist_if_idle(&mut self) {
        if self.busy {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        let model = self.config.model.clone();
        let id = self.current_session_id();
        if let Err(e) =
            agent_host::persist::save(&self.app_data, &self.workspace, &model, &id, &session).await
        {
            tracing::warn!("保存会话失败：{e}");
        }
        self.session = Some(session);
    }

    fn current_session_id(&self) -> String {
        self.session_id.clone()
    }

    fn rebuild_entries(&mut self) {
        rebuild_entries(self.session.as_ref(), &mut self.ui_entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::ToolResultBody;
    use command_group::{CommandGroup, GroupChild};
    use std::io::{BufRead, BufReader};
    use std::process::{Command as StdCommand, Stdio};

    fn test_app() -> App {
        App::new(LlmConfig::default(), PathBuf::from(".")).unwrap()
    }

    /// 事件流应当驱动展示模型：delta 累积成文本片段。
    #[test]
    fn assistant_delta_accumulates_into_text_segment() {
        let mut app = test_app();
        app.apply_agent_event(AgentEvent::TurnStart {
            turn_id: "t1".into(),
            model: "m".into(),
        });
        app.apply_agent_event(AgentEvent::AssistantDelta {
            turn_id: "t1".into(),
            text: "你好".into(),
        });
        app.apply_agent_event(AgentEvent::AssistantDelta {
            turn_id: "t1".into(),
            text: "，世界".into(),
        });

        let rows = crate::view::build_history_rows(&app.ui_entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("你好，世界")),
            "delta 应累积成完整文本：{joined:?}"
        );
    }

    /// 工具卡片的生命周期：开始 → 参数就绪 → 结果，状态逐段推进。
    #[test]
    fn tool_card_goes_through_full_lifecycle() {
        let mut app = test_app();
        app.apply_agent_event(AgentEvent::TurnStart {
            turn_id: "t1".into(),
            model: "m".into(),
        });
        app.apply_agent_event(AgentEvent::ToolCallStart {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            name: "Read".into(),
        });
        app.apply_agent_event(AgentEvent::ToolCallReady {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            name: "Read".into(),
            args: serde_json::json!({ "path": "a.txt" }),
            preview: "读取 a.txt".into(),
        });
        app.apply_agent_event(AgentEvent::ToolResult {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            ok: true,
            duration_ms: 12,
            result: ToolResultBody::Text {
                content: "文件内容".into(),
                truncated: false,
            },
        });

        let rows = crate::view::build_history_rows(&app.ui_entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("✓") && l.contains("Read")),
            "完成的工具应以 ✓ 呈现：{joined:?}"
        );
    }

    /// 插队等待的输入在轮次结束后应当立即开新一轮。
    #[test]
    fn pending_input_is_flushed_after_turn_finishes() {
        let mut app = test_app();
        app.pending_input = Some("第二条消息".into());

        let turn_id = "t1".to_string();
        app.active_turn = Some(ActiveTurn {
            turn_id: turn_id.clone(),
            cancel: CancellationToken::new(),
        });
        app.busy = true;

        let msg = UiMessage::TurnFinished {
            turn_id: turn_id.clone(),
            session: Session::default(),
        };
        let mut app = app;
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            app.handle_ui_message(msg).await;
        });

        assert!(
            app.pending_input.is_none(),
            "pending 输入应被消费并开启新一轮"
        );
        assert!(
            app.busy || app.active_turn.is_some(),
            "新一轮应已开始（busy 或 active_turn 生效）"
        );
    }

    /// 空闲时 channel 必须保持打开。
    #[tokio::test]
    async fn idle_channel_stays_open() {
        let mut app = test_app();
        assert!(
            !app.keepalive_tx.is_closed(),
            "App 持有保活 sender，channel 不应关闭"
        );
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(200), app.rx.recv()).await;
        assert!(
            result.is_err(),
            "空闲时 recv 应挂起等待而非返回 None：{result:?}"
        );
    }

    /// 轮次结束后 channel 同样不能关闭。
    #[tokio::test]
    async fn channel_stays_open_after_turn() {
        let server = FakeLlm::start();
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(
            LlmConfig {
                base_url: server.base_url.clone(),
                api_key: String::new(),
                model: "basic-chat".into(),
                ..Default::default()
            },
            dir.path().to_path_buf(),
        )
        .unwrap();
        app.app_data = dir.path().to_path_buf();

        app.start_turn("你好".into());
        while let Some(msg) = app.rx.recv().await {
            app.handle_ui_message(msg).await;
            if app.active_turn.is_none() {
                break;
            }
        }
        assert!(
            !app.keepalive_tx.is_closed(),
            "轮次结束后保活 sender 应仍在，channel 不关闭"
        );
    }

    /// 键盘 Release 事件必须忽略。
    #[tokio::test]
    async fn key_release_does_not_duplicate_input() {
        let mut app = test_app();
        for c in ['介', '绍'] {
            app.handle_key(KeyEvent::new_with_kind(
                KeyCode::Char(c),
                KeyModifiers::NONE,
                KeyEventKind::Press,
            ))
            .await
            .unwrap();
            app.handle_key(KeyEvent::new_with_kind(
                KeyCode::Char(c),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ))
            .await
            .unwrap();
        }
        assert_eq!(
            app.composer.input, "介绍",
            "Release 事件不得重复插入字符，实际：{:?}",
            app.composer.input
        );
    }

    // ---------- 端到端：真协议 + 真服务 ----------

    const SERVER: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/fake-llm/server.mjs"
    );

    struct FakeLlm {
        child: GroupChild,
        base_url: String,
    }

    impl FakeLlm {
        fn start() -> Self {
            let mut child = StdCommand::new("node")
                .arg(SERVER)
                .args(["--port", "0"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .group_spawn()
                .expect("启动 fake-llm 失败，请确认 node 在 PATH 上");

            let stdout = child.inner().stdout.take().expect("stdout 已被取走");
            let mut line = String::new();
            BufReader::new(stdout)
                .read_line(&mut line)
                .expect("读取 fake-llm 就绪行失败");

            let base_url = line
                .strip_prefix("FAKE_LLM_READY ")
                .unwrap_or_else(|| panic!("fake-llm 未按约定输出就绪行，实际收到：{line:?}"))
                .trim()
                .to_string();

            Self { child, base_url }
        }
    }

    impl Drop for FakeLlm {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// 完整轮次走通：发送 → 事件流 → 展示模型更新。
    #[tokio::test]
    async fn full_turn_updates_display_model() {
        let server = FakeLlm::start();
        let dir = tempfile::tempdir().unwrap();

        let mut app = App::new(
            LlmConfig {
                base_url: server.base_url.clone(),
                api_key: String::new(),
                model: "basic-chat".into(),
                ..Default::default()
            },
            dir.path().to_path_buf(),
        )
        .unwrap();
        app.app_data = dir.path().to_path_buf();

        app.start_turn("你好，介绍一下这个项目".into());

        let mut finished = false;
        while let Some(msg) = app.rx.recv().await {
            app.handle_ui_message(msg).await;
            if app.active_turn.is_none() {
                finished = true;
                break;
            }
        }
        assert!(finished, "轮次应在收到 TurnFinished 后结束");

        let rows = crate::view::build_history_rows(&app.ui_entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("agent-core")),
            "展示模型应包含模型回答的内容：{joined:?}"
        );
        assert!(
            joined.iter().any(|l| l.contains("✓")),
            "轮次结束后应显示完成标记：{joined:?}"
        );
    }

    /// 取消：轮次进行中触发取消，展示模型应标记为已取消。
    #[tokio::test]
    async fn cancel_turn_marks_status() {
        let server = FakeLlm::start();
        let dir = tempfile::tempdir().unwrap();

        let mut app = App::new(
            LlmConfig {
                base_url: server.base_url.clone(),
                api_key: String::new(),
                model: "cancellable".into(),
                ..Default::default()
            },
            dir.path().to_path_buf(),
        )
        .unwrap();
        app.app_data = dir.path().to_path_buf();

        app.start_turn("开始吧".into());

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        app.cancel_active().await;

        let mut finished = false;
        while let Some(msg) = app.rx.recv().await {
            app.handle_ui_message(msg).await;
            if app.active_turn.is_none() {
                finished = true;
                break;
            }
        }
        assert!(finished, "取消后轮次仍应正常收尾");

        let rows = crate::view::build_history_rows(&app.ui_entries, false, 0);
        let joined: Vec<String> = rows.iter().map(|r| r.content.to_string()).collect();
        assert!(
            joined.iter().any(|l| l.contains("✕")),
            "取消的轮次应显示取消标记：{joined:?}"
        );
    }

    /// 斜杠命令应当被本地拦截并反馈 Notice，不流入 agent 轮次。
    #[tokio::test]
    async fn slash_commands_intercepted_locally() {
        let mut app = test_app();
        app.composer.insert_text("/help");
        app.send_input().await;

        assert!(!app.busy, "/help 不应触发大模型轮次");
        assert!(
            matches!(app.ui_entries.last(), Some(UiEntry::Notice { text }) if text.contains("/help")),
            "执行 /help 后应推入包含命令清单的 Notice"
        );

        // 测试 /model 切换
        app.composer.insert_text("/model new-custom-model");
        app.send_input().await;
        assert_eq!(app.config.model, "new-custom-model");

        // 测试 /clear 清屏重置
        app.composer.insert_text("/clear");
        app.send_input().await;
        assert!(
            matches!(app.ui_entries.last(), Some(UiEntry::Notice { text }) if text.contains("已清空")),
            "执行 /clear 后应重置并推入清空提示"
        );
    }

    /// 斜杠命令候选匹配与 Tab 键补全交互。
    #[tokio::test]
    async fn slash_completions_filtering_and_tab_autocomplete() {
        let mut app = test_app();
        assert!(slash_completions(&app.composer.input, app.composer.cursor).is_empty());

        // 输入 "/" 匹配全部斜杠命令
        app.composer.insert_text("/");
        let all = slash_completions(&app.composer.input, app.composer.cursor);
        assert_eq!(all.len(), crate::slash::SLASH_COMMANDS.len());

        // 输入 "/c" 匹配 /clear 和 /compact
        app.composer.insert_text("c");
        let matches = slash_completions(&app.composer.input, app.composer.cursor);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].name, "/clear");
        assert_eq!(matches[1].name, "/compact");

        // Tab 补全首个选项，并在末尾追加空格
        let key_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        app.handle_key(key_tab).await.unwrap();
        assert_eq!(app.composer.input, "/clear ");
        // 带空格后光标在后，补全浮窗应自动收起
        assert!(slash_completions(&app.composer.input, app.composer.cursor).is_empty());

        // 测试上下方向键循环切换选中项
        app.composer.input = "/c".into();
        app.composer.cursor = 2;
        assert_eq!(app.completion_idx, 0);
        let key_down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        app.handle_key(key_down).await.unwrap();
        assert_eq!(app.completion_idx, 1);
        let key_tab2 = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        app.handle_key(key_tab2).await.unwrap();
        assert_eq!(app.composer.input, "/compact ");
    }

    /// 斜杠命令 /exit 与 /quit 均可正常退出应用。
    #[tokio::test]
    async fn slash_commands_exit_and_quit() {
        let mut app = test_app();
        assert!(!app.should_exit);

        app.composer.insert_text("/exit");
        app.send_input().await;
        assert!(app.should_exit);

        app.should_exit = false;
        app.composer.insert_text("/quit");
        app.send_input().await;
        assert!(app.should_exit);
    }

    /// 第一次 Ctrl+C 清空当前对话框，空输入时连续按两次退出。
    #[tokio::test]
    async fn ctrl_c_first_clears_input_then_exits() {
        let mut app = test_app();
        app.composer.insert_text("正在输入一段未发送的草稿...");
        assert_eq!(app.composer.input, "正在输入一段未发送的草稿...");

        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

        // 第一次按 Ctrl+C：清空当前对话框，不退出
        app.handle_key(ctrl_c).await.unwrap();
        assert!(
            app.composer.input.is_empty(),
            "第一次 Ctrl+C 应当清空对话框输入"
        );
        assert!(!app.should_exit, "清空输入时不应直接退出应用");

        // 对话框已空时，按一次给出提示
        app.handle_key(ctrl_c).await.unwrap();
        assert!(!app.should_exit, "空输入首次按 Ctrl+C 应提示而非退出");
        assert!(
            matches!(app.ui_entries.last(), Some(UiEntry::Notice { text }) if text.contains("再按一次 Ctrl+C 退出")),
            "应给出再按一次退出的提示"
        );

        // 紧接着再次按 Ctrl+C：退出应用
        app.handle_key(ctrl_c).await.unwrap();
        assert!(app.should_exit, "短时间内二次按 Ctrl+C 应当退出应用");
    }

    /// 思维链、正文与工具卡片按时间线顺序进入展示模型。
    #[tokio::test]
    async fn timeline_order_preserved_in_ui_entries() {
        let mut app = test_app();
        app.apply_agent_event(AgentEvent::TurnStart {
            turn_id: "t1".into(),
            model: "test-model".into(),
        });
        app.apply_agent_event(AgentEvent::ReasoningDelta {
            turn_id: "t1".into(),
            text: "思考 1".into(),
        });
        app.apply_agent_event(AgentEvent::AssistantDelta {
            turn_id: "t1".into(),
            text: "文本 1".into(),
        });
        app.apply_agent_event(AgentEvent::ToolCallStart {
            turn_id: "t1".into(),
            call_id: "c1".into(),
            name: "Read".into(),
        });
        app.apply_agent_event(AgentEvent::ReasoningDelta {
            turn_id: "t1".into(),
            text: "思考 2".into(),
        });
        app.apply_agent_event(AgentEvent::AssistantDelta {
            turn_id: "t1".into(),
            text: "文本 2".into(),
        });

        let Some(UiEntry::Assistant { segments, .. }) = app.ui_entries.last() else {
            panic!("应当有助手条目");
        };
        assert_eq!(segments.len(), 5);
        assert!(matches!(&segments[0], AssistantSegment::Reasoning(r) if r == "思考 1"));
        assert!(matches!(&segments[1], AssistantSegment::Text(_)));
        assert!(matches!(&segments[2], AssistantSegment::Tool(_)));
        assert!(matches!(&segments[3], AssistantSegment::Reasoning(r) if r == "思考 2"));
        assert!(matches!(&segments[4], AssistantSegment::Text(_)));
    }
}
