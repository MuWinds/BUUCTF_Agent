//! 输入框状态与多行文本编辑逻辑。
//!
//! 负责多行文本编辑（Unicode 字符插入/退格/按词回退/移动）、
//! 光标在多行文本中的行列定位计算、输入历史翻阅与草稿保存、
//! 以及 Ctrl+C 双阶段拦截控制。

use std::time::{Duration, Instant};

/// Ctrl+C 触发时的意图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlCAction {
    /// 第一次 Ctrl+C 清空了非空输入框中的内容。
    ClearedInput,
    /// 输入框为空，第一次触发，需要提示用户「再按一次退出」。
    PromptConfirm,
    /// 1.5 秒内第二次触发，确认退出应用。
    ConfirmExit,
}

/// 输入框状态与历史管理。
pub struct Composer {
    /// 当前输入文本。
    pub input: String,
    /// 光标在 `input` 中的字节偏移量（必须落在 Unicode 字符边界）。
    pub cursor: usize,
    /// 历史输入列表。
    pub input_history: Vec<String>,
    /// 当前翻阅的历史索引（`None` 表示正在编辑当前草稿）。
    pub history_idx: Option<usize>,
    /// 暂存的用户当前草稿。
    pub draft_input: String,
    /// 上一次按下 Ctrl+C 的时间戳（用于空输入时的连续双击退出确认）。
    pub last_ctrl_c: Option<Instant>,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    /// 创建全新空白的输入框。
    pub fn new() -> Self {
        Self {
            input: String::new(),
            cursor: 0,
            input_history: Vec::new(),
            history_idx: None,
            draft_input: String::new(),
            last_ctrl_c: None,
        }
    }

    /// 当前输入是否为空。
    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// 清空当前输入与光标状态。
    pub fn clear(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.history_idx = None;
        self.last_ctrl_c = None;
    }

    /// 在当前光标处插入文本（自动将 \r\n 或裸 \r 规整化为标准 \n）。
    pub fn insert_text(&mut self, text: &str) {
        if text.contains('\r') {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            self.input.insert_str(self.cursor, &normalized);
            self.cursor += normalized.len();
        } else {
            self.input.insert_str(self.cursor, text);
            self.cursor += text.len();
        }
    }

    /// 向前退格删除一个 Unicode 字符。
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // 找到光标前最后一个字符的**起点**：drain 的区间是 [起点, 光标)。
        let prev = self.input[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.input.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// 按字符移动光标（`delta` 为 ±1）。
    ///
    /// 无论光标位于字符串头部、中间还是末尾，均按 Unicode 字符边界逐个步进，
    /// 不会在末尾按左键时突跳到行首。
    pub fn move_cursor(&mut self, delta: i32) {
        let chars: Vec<(usize, char)> = self.input.char_indices().collect();
        let current_idx = if self.cursor >= self.input.len() {
            chars.len()
        } else {
            chars
                .iter()
                .position(|&(i, _)| i >= self.cursor)
                .unwrap_or(chars.len())
        };
        let new_idx = (current_idx as i32 + delta).clamp(0, chars.len() as i32) as usize;
        self.cursor = if new_idx >= chars.len() {
            self.input.len()
        } else {
            chars[new_idx].0
        };
    }

    /// 在多行输入中将光标垂直向上移动一行。
    ///
    /// 若光标不在第 0 行，将光标移动到上一行的相应列位置，并返回 `true`；
    /// 若光标已处于第 0 行，不移动并返回 `false`（以便上层按需触发历史记录翻阅）。
    pub fn move_cursor_up(&mut self) -> bool {
        let lines_vec = self.lines();
        if lines_vec.len() <= 1 {
            return false;
        }
        let (cursor_row, cursor_col_byte) = self.calculate_cursor_offset(&lines_vec);
        if cursor_row == 0 {
            return false;
        }
        let target_row = cursor_row - 1;
        let current_line = &lines_vec[cursor_row];
        let col_char_count = current_line[..cursor_col_byte.min(current_line.len())]
            .chars()
            .count();

        let target_line = &lines_vec[target_row];
        let target_char_indices: Vec<(usize, char)> = target_line.char_indices().collect();
        let target_col_byte = target_char_indices
            .get(col_char_count)
            .map(|&(i, _)| i)
            .unwrap_or(target_line.len());

        let mut offset = 0;
        for (r, l) in lines_vec.iter().enumerate() {
            if r == target_row {
                offset += target_col_byte;
                break;
            }
            offset += l.len() + 1; // +1 为换行符 '\n'
        }
        self.cursor = offset.min(self.input.len());
        true
    }

    /// 在多行输入中将光标垂直向下移动一行。
    ///
    /// 若光标不在最后一行，将光标移动到下一行的相应列位置，并返回 `true`；
    /// 若光标已处于最后一行，不移动并返回 `false`（以便上层按需触发历史记录翻阅）。
    pub fn move_cursor_down(&mut self) -> bool {
        let lines_vec = self.lines();
        if lines_vec.len() <= 1 {
            return false;
        }
        let (cursor_row, cursor_col_byte) = self.calculate_cursor_offset(&lines_vec);
        if cursor_row >= lines_vec.len() - 1 {
            return false;
        }
        let target_row = cursor_row + 1;
        let current_line = &lines_vec[cursor_row];
        let col_char_count = current_line[..cursor_col_byte.min(current_line.len())]
            .chars()
            .count();

        let target_line = &lines_vec[target_row];
        let target_char_indices: Vec<(usize, char)> = target_line.char_indices().collect();
        let target_col_byte = target_char_indices
            .get(col_char_count)
            .map(|&(i, _)| i)
            .unwrap_or(target_line.len());

        let mut offset = 0;
        for (r, l) in lines_vec.iter().enumerate() {
            if r == target_row {
                offset += target_col_byte;
                break;
            }
            offset += l.len() + 1; // +1 为换行符 '\n'
        }
        self.cursor = offset.min(self.input.len());
        true
    }

    /// 检查并执行行尾 `\` 换行续行。
    ///
    /// 若光标位于续行符 `\`（或反斜杠加空白）之后，按下回车时自动消除续行符并插入换行符 `\n`。
    pub fn try_newline_continuation(&mut self) -> bool {
        let prefix = &self.input[..self.cursor];
        let trimmed = prefix.trim_end_matches([' ', '\t']);
        if trimmed.ends_with('\\') && !trimmed.ends_with("\\\\") {
            let backslash_pos = trimmed.len() - 1;
            self.input.drain(backslash_pos..self.cursor);
            self.cursor = backslash_pos;
            self.insert_text("\n");
            true
        } else {
            false
        }
    }

    /// 跳到输入最前端。
    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// 跳到输入最末尾。
    pub fn move_to_end(&mut self) {
        self.cursor = self.input.len();
    }

    /// 清空光标之前的所有文本。
    pub fn clear_to_start(&mut self) {
        self.input.drain(..self.cursor);
        self.cursor = 0;
        self.history_idx = None;
    }

    /// 清空光标之后的所有文本。
    pub fn clear_to_end(&mut self) {
        self.input.truncate(self.cursor);
        self.history_idx = None;
    }

    /// 按词回退删除（跳过光标前的空白字符，然后删除直到前一个空白边界）。
    pub fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prefix = &self.input[..self.cursor];
        let mut char_indices: Vec<(usize, char)> = prefix.char_indices().collect();
        // 先跳过光标前的空白
        while let Some(&(_, c)) = char_indices.last() {
            if c.is_whitespace() {
                char_indices.pop();
            } else {
                break;
            }
        }
        // 再跳过非空白单词字符
        while let Some(&(_, c)) = char_indices.last() {
            if !c.is_whitespace() {
                char_indices.pop();
            } else {
                break;
            }
        }
        let target_pos = char_indices
            .last()
            .map(|&(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        self.input.drain(target_pos..self.cursor);
        self.cursor = target_pos;
    }

    /// 向上翻阅历史输入，并在初次翻阅时暂存当前草稿。
    pub fn history_up(&mut self) {
        if !self.input_history.is_empty() {
            match self.history_idx {
                None => {
                    self.draft_input = self.input.clone();
                    let last = self.input_history.len() - 1;
                    self.history_idx = Some(last);
                    self.input = self.input_history[last].clone();
                    self.cursor = self.input.len();
                }
                Some(idx) if idx > 0 => {
                    let next_idx = idx - 1;
                    self.history_idx = Some(next_idx);
                    self.input = self.input_history[next_idx].clone();
                    self.cursor = self.input.len();
                }
                _ => {}
            }
        }
    }

    /// 向下翻阅历史输入，直到恢复最初暂存的草稿。
    pub fn history_down(&mut self) {
        if let Some(idx) = self.history_idx {
            if idx + 1 < self.input_history.len() {
                let next_idx = idx + 1;
                self.history_idx = Some(next_idx);
                self.input = self.input_history[next_idx].clone();
                self.cursor = self.input.len();
            } else {
                self.history_idx = None;
                self.input = self.draft_input.clone();
                self.cursor = self.input.len();
            }
        }
    }

    /// 记录一条已提交的历史输入。
    pub fn push_history(&mut self, text: String) {
        self.input_history.push(text);
    }

    /// 处理 Ctrl+C 按键逻辑：
    /// - 若输入框有未提交草稿，立即清空输入框并返回 `ClearedInput`；
    /// - 若输入框为空且距离上次按键超过 1.5s，返回 `PromptConfirm`；
    /// - 若输入框为空且 1.5s 内连续二次按下，返回 `ConfirmExit`。
    pub fn handle_ctrl_c(&mut self) -> CtrlCAction {
        if !self.is_empty() {
            self.clear();
            CtrlCAction::ClearedInput
        } else {
            let now = Instant::now();
            if let Some(prev) = self.last_ctrl_c {
                if now.duration_since(prev) <= Duration::from_millis(1500) {
                    return CtrlCAction::ConfirmExit;
                }
            }
            self.last_ctrl_c = Some(now);
            CtrlCAction::PromptConfirm
        }
    }

    /// 将当前输入按换行切分为多行。
    pub fn lines(&self) -> Vec<String> {
        self.input.split('\n').map(String::from).collect()
    }

    /// 计算多行文本中光标所在的行索引与当前行的字节偏移。
    pub fn calculate_cursor_offset(&self, lines_vec: &[String]) -> (usize, usize) {
        let mut char_count = 0;
        let mut cursor_row = 0;
        let mut cursor_col_byte = 0;
        for (row, line_str) in lines_vec.iter().enumerate() {
            let next_count = char_count + line_str.len();
            if self.cursor <= next_count || row == lines_vec.len() - 1 {
                cursor_row = row;
                cursor_col_byte = self.cursor.saturating_sub(char_count);
                break;
            }
            char_count = next_count + 1; // +1 为换行符 '\n'
        }
        (cursor_row, cursor_col_byte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 输入编辑：插入、退格、光标移动都按 Unicode 字符（而非裸字节）工作。
    #[test]
    fn input_editing_handles_unicode() {
        let mut composer = Composer::new();
        composer.insert_text("你好");
        assert_eq!(composer.input, "你好");
        assert_eq!(composer.cursor, 6, "光标应落在字节末尾");

        composer.backspace();
        assert_eq!(composer.input, "你");
        assert_eq!(composer.cursor, 3, "退格应删掉一个完整字符");

        composer.move_cursor(-1);
        assert_eq!(composer.cursor, 0, "向前移动一个字符应到头部");
        composer.move_cursor(1);
        assert_eq!(composer.cursor, 3, "向后移动一个字符应回到尾部");
    }

    /// Readline 快捷键与按词回退。
    #[test]
    fn readline_shortcuts_and_word_deletion() {
        let mut composer = Composer::new();
        composer.insert_text("hello world");

        composer.delete_word_backward();
        assert_eq!(composer.input, "hello ");

        composer.insert_text("test string");
        composer.move_to_start();
        assert_eq!(composer.cursor, 0);

        composer.move_to_end();
        assert_eq!(composer.cursor, composer.input.len());

        composer.clear_to_start();
        assert_eq!(composer.input, "");
    }

    /// 上下键翻阅历史并能无损恢复草稿。
    #[test]
    fn history_navigation_and_draft_restoration() {
        let mut composer = Composer::new();
        composer.push_history("第一条命令".into());
        composer.push_history("第二条命令".into());

        composer.insert_text("正在写的新草稿");
        assert_eq!(composer.input, "正在写的新草稿");

        // 向上翻阅历史
        composer.history_up();
        assert_eq!(composer.input, "第二条命令");

        composer.history_up();
        assert_eq!(composer.input, "第一条命令");

        // 向下恢复
        composer.history_down();
        assert_eq!(composer.input, "第二条命令");

        composer.history_down();
        assert_eq!(composer.input, "正在写的新草稿");
    }

    /// 第一次 Ctrl+C 清空当前对话框，空输入时连续按两次退出。
    #[test]
    fn ctrl_c_first_clears_input_then_exits() {
        let mut composer = Composer::new();
        composer.insert_text("正在输入一段未发送的草稿...");

        // 第一次按 Ctrl+C：清空当前对话框
        assert_eq!(composer.handle_ctrl_c(), CtrlCAction::ClearedInput);
        assert!(composer.is_empty(), "第一次 Ctrl+C 应当清空对话框输入");

        // 对话框已空时，按一次给出提示
        assert_eq!(composer.handle_ctrl_c(), CtrlCAction::PromptConfirm);

        // 紧接着再次按 Ctrl+C：确认退出应用
        assert_eq!(composer.handle_ctrl_c(), CtrlCAction::ConfirmExit);
    }

    /// 光标在末尾与头部移动时必须严格逐字步进，绝不可突跳到行首或行尾。
    #[test]
    fn cursor_moves_by_single_character_at_boundaries() {
        let mut composer = Composer::new();
        composer.insert_text("hello");
        assert_eq!(composer.cursor, 5);

        // 在末尾按左键，必须只前移 1 个字符（到 'o' 的位置 4），绝不能跳到 0
        composer.move_cursor(-1);
        assert_eq!(composer.cursor, 4, "末尾按左键应退到最后一个字符");

        composer.move_cursor(-1);
        assert_eq!(composer.cursor, 3);

        // 移到头部再按左键，光标保持在 0
        composer.move_to_start();
        assert_eq!(composer.cursor, 0);
        composer.move_cursor(-1);
        assert_eq!(composer.cursor, 0, "头部按左键应保持在头部");

        // 从头部向右步进
        composer.move_cursor(1);
        assert_eq!(composer.cursor, 1);

        // Unicode CJK 测试
        let mut cjk_composer = Composer::new();
        cjk_composer.insert_text("你好世界");
        assert_eq!(cjk_composer.cursor, 12);

        // 在末尾按左键，必须退到 '界' 的起始字节 9，而不是跳到 0
        cjk_composer.move_cursor(-1);
        assert_eq!(
            cjk_composer.cursor, 9,
            "中文末尾按左键应退一个汉字（3 字节）"
        );

        cjk_composer.move_cursor(-1);
        assert_eq!(cjk_composer.cursor, 6);
    }

    /// 多行输入时光标垂直上下移动与边缘退化。
    #[test]
    fn multiline_cursor_navigation_and_history_fallback() {
        let mut composer = Composer::new();
        composer.insert_text("第一行文本\n第二行较短\n第三行内容");

        // 光标在末尾（第三行）
        assert_eq!(composer.lines().len(), 3);
        let (r3, _) = composer.calculate_cursor_offset(&composer.lines());
        assert_eq!(r3, 2);

        // 向上移动到第二行
        assert!(composer.move_cursor_up(), "从第三行应能上移到第二行");
        let (r2, _) = composer.calculate_cursor_offset(&composer.lines());
        assert_eq!(r2, 1);

        // 向上移动到第一行
        assert!(composer.move_cursor_up(), "从第二行应能上移到第一行");
        let (r1, _) = composer.calculate_cursor_offset(&composer.lines());
        assert_eq!(r1, 0);

        // 在第一行再次向上移动，应返回 false 以便交由翻阅历史处理
        assert!(!composer.move_cursor_up(), "第一行再次上移应返回 false");

        // 向下移动到第二行
        assert!(composer.move_cursor_down(), "第一行应能下移到第二行");
        let (r2_down, _) = composer.calculate_cursor_offset(&composer.lines());
        assert_eq!(r2_down, 1);

        // 向下移动到第三行
        assert!(composer.move_cursor_down(), "第二行应能下移到第三行");
        let (r3_down, _) = composer.calculate_cursor_offset(&composer.lines());
        assert_eq!(r3_down, 2);

        // 在最后一行再次向下移动，应返回 false
        assert!(!composer.move_cursor_down(), "最后一行再次下移应返回 false");
    }

    /// 行尾反斜杠续行检测与换行转换。
    #[test]
    fn line_continuation_with_backslash() {
        let mut composer = Composer::new();
        composer.insert_text("echo hello\\");

        assert!(
            composer.try_newline_continuation(),
            "尾部带有 \\ 应成功触发换行续行"
        );
        assert_eq!(composer.input, "echo hello\n");
        assert_eq!(composer.lines().len(), 2);

        // 带有尾部空格的反斜杠也应能成功续行
        let mut space_composer = Composer::new();
        space_composer.insert_text("echo hello\\  ");
        assert!(space_composer.try_newline_continuation());
        assert_eq!(space_composer.input, "echo hello\n");

        // 双反斜杠表示转义本身，不应触发续行
        let mut escaped_composer = Composer::new();
        escaped_composer.insert_text("echo hello\\\\");
        assert!(!escaped_composer.try_newline_continuation());
    }

    /// 文本插入自动规整化 \r\n 为 \n。
    #[test]
    fn insert_text_normalizes_crlf() {
        let mut composer = Composer::new();
        composer.insert_text("line1\r\nline2\rline3\nline4");
        assert_eq!(composer.input, "line1\nline2\nline3\nline4");
        assert_eq!(composer.lines().len(), 4);
    }
}
