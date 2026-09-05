//! 终端生命周期管理。
//!
//! 在终端主屏缓冲（Main Screen Buffer）中运行：
//! - 不进入 AlternateScreen，会话历史直接推入终端原生 Scrollback，退出后依然完整保留；
//! - 不开启 MouseCapture，终端保持原生的鼠标左键直接划选、双击选词与复制体验；
//! - 鼠标滚轮由终端原生处理，平滑滚动上下文；
//! - 退出时安全恢复终端原始 Raw Mode 与键盘状态。

use std::io::{self, IsTerminal};

use anyhow::Result;
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal;
use tokio_stream::StreamExt;

/// 已初始化的终端。持有 crossterm 事件流。
pub struct Terminal {
    /// 事件流被 drop 时 crossterm 会停止读 stdin —— 这是「把终端
    /// 让给子进程」的机制，也是退出时的清理路径。
    events: EventStream,
}

impl Terminal {
    /// 初始化终端，进入 Raw Mode，准备流式交互。
    pub fn init() -> Result<Self> {
        if !io::stdin().is_terminal() {
            return Err(anyhow::anyhow!(
                "stdin 不是终端：请在真实的终端窗口里运行（或不要重定向输入）"
            ));
        }
        if !io::stdout().is_terminal() {
            return Err(anyhow::anyhow!(
                "stdout 不是终端：请在真实的终端窗口里运行（或不要重定向输出）"
            ));
        }

        terminal::enable_raw_mode()?;
        let _ = execute!(
            io::stdout(),
            Hide,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            ),
            EnableBracketedPaste
        );

        Ok(Self {
            events: EventStream::new(),
        })
    }

    /// 临时挂起终端交互模式（拉起子进程或外部编辑器时使用）。
    pub fn pause() -> Result<()> {
        let _ = execute!(
            io::stdout(),
            Show,
            PopKeyboardEnhancementFlags,
            DisableBracketedPaste
        );
        let _ = terminal::disable_raw_mode();
        Ok(())
    }

    /// 从临时挂起中恢复终端交互模式。
    pub fn resume() -> Result<()> {
        terminal::enable_raw_mode()?;
        let _ = execute!(
            io::stdout(),
            Hide,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            ),
            EnableBracketedPaste
        );
        Ok(())
    }

    /// 离开终端交互模式，恢复终端到进入前的状态。
    pub fn restore() -> Result<()> {
        let _ = execute!(
            io::stdout(),
            Show,
            PopKeyboardEnhancementFlags,
            DisableBracketedPaste
        );
        let _ = terminal::disable_raw_mode();
        Ok(())
    }

    /// 下一个终端输入事件。
    pub async fn next_event(&mut self) -> Option<io::Result<Event>> {
        self.events.next().await
    }

    /// 获取当前终端尺寸 (列宽, 行高)。
    pub fn size() -> (u16, u16) {
        terminal::size().unwrap_or((80, 24))
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = Self::restore();
    }
}
