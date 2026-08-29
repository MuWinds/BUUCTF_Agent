//! 「Edit 前必须先 Read」的约束。
//!
//! 两个作用：
//!
//! 1. **防盲改** —— 模型没看过文件就改，多半是在凭想象编辑
//! 2. **防覆盖** —— 记下 Read 时的修改时间，Edit 时比对；文件在这期间
//!    被外部改过（用户在编辑器里保存了）就拒绝，避免把别人的改动冲掉

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use agent_core::ToolError;

#[derive(Default)]
pub struct ReadRegistry {
    /// 路径 → 该文件被 Read 时的修改时间。
    seen: Mutex<HashMap<PathBuf, SystemTime>>,
}

impl ReadRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次成功的读取。
    pub fn record(&self, path: &Path, modified: SystemTime) {
        self.seen
            .lock()
            .expect("ReadRegistry 锁被毒化")
            .insert(path.to_path_buf(), modified);
    }

    /// 校验这个文件可以被编辑。
    ///
    /// 错误消息直接告诉模型下一步该干什么 —— 这类提示的措辞质量
    /// 直接决定模型能不能自己走出来。
    pub fn check(&self, path: &Path, current: SystemTime, shown: &str) -> Result<(), ToolError> {
        let seen = self.seen.lock().expect("ReadRegistry 锁被毒化");

        let Some(recorded) = seen.get(path) else {
            return Err(ToolError::recoverable(format!(
                "编辑 `{shown}` 之前必须先用 Read 读取它的当前内容。"
            )));
        };

        if *recorded != current {
            return Err(ToolError::recoverable(format!(
                "`{shown}` 在你读取之后被外部修改过。请重新用 Read 读取最新内容，\
                 确认改动仍然适用后再编辑。"
            )));
        }

        Ok(())
    }

    /// 写入之后刷新记录，让模型可以连续编辑同一个文件。
    pub fn refresh(&self, path: &Path, modified: SystemTime) {
        self.record(path, modified);
    }

    /// 工作区切换时清空 —— 旧工作区的记录对新工作区毫无意义。
    pub fn clear(&self) {
        self.seen.lock().expect("ReadRegistry 锁被毒化").clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn path() -> PathBuf {
        PathBuf::from("/tmp/a.rs")
    }

    #[test]
    fn rejects_edit_without_read() {
        let registry = ReadRegistry::new();
        let err = registry
            .check(&path(), SystemTime::UNIX_EPOCH, "a.rs")
            .expect_err("没读过就编辑必须被拒");
        assert!(err.to_string().contains("必须先用 Read"));
    }

    #[test]
    fn allows_edit_after_read() {
        let registry = ReadRegistry::new();
        let time = SystemTime::UNIX_EPOCH;
        registry.record(&path(), time);

        registry
            .check(&path(), time, "a.rs")
            .expect("读过且未变动，应当放行");
    }

    #[test]
    fn rejects_edit_when_file_changed_externally() {
        let registry = ReadRegistry::new();
        registry.record(&path(), SystemTime::UNIX_EPOCH);

        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let err = registry
            .check(&path(), later, "a.rs")
            .expect_err("外部修改过必须被拒");
        assert!(err.to_string().contains("被外部修改过"));
    }

    #[test]
    fn refresh_allows_consecutive_edits() {
        let registry = ReadRegistry::new();
        registry.record(&path(), SystemTime::UNIX_EPOCH);

        let after_write = SystemTime::UNIX_EPOCH + Duration::from_secs(5);
        registry.refresh(&path(), after_write);

        registry
            .check(&path(), after_write, "a.rs")
            .expect("写入后刷新过，应当允许继续编辑");
    }

    #[test]
    fn clear_forgets_everything() {
        let registry = ReadRegistry::new();
        registry.record(&path(), SystemTime::UNIX_EPOCH);
        registry.clear();

        registry
            .check(&path(), SystemTime::UNIX_EPOCH, "a.rs")
            .expect_err("清空后应当恢复到未读状态");
    }
}
