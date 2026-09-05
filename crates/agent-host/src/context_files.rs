//! 上下文文件加载。
//!
//! 从工作区目录出发，向上遍历到文件系统根目录，收集每个目录里的上下文文件：
//! 每个目录按优先级取 `AGENTS.override.md`（覆盖）→ `AGENTS.md` / `AGENTS.MD` →
//! `CLAUDE.md` / `CLAUDE.MD`。加载顺序从最远的祖先到工作区本身，保证离工作区
//! 越近的约定越靠后 —— 与 pi 的行为一致。
//!
//! 这些是用户自己写的项目约定，文件很小（几个 Markdown），因此用同步 I/O 直接读，
//! 不为此引入异步。读失败只记日志不报错：上下文文件是增强项，不该成为对话的拦路石。

use std::path::{Path, PathBuf};

/// 每个目录里按优先级排列的候选文件名。
const CANDIDATES: [&str; 5] = [
    "AGENTS.override.md",
    "AGENTS.md",
    "AGENTS.MD",
    "CLAUDE.md",
    "CLAUDE.MD",
];

/// 从工作区向上收集到的一段上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFile {
    pub path: PathBuf,
    pub content: String,
}

/// 加载工作区及其所有祖先目录里的上下文文件。
///
/// 返回顺序：从最远的祖先目录到工作区本身。空内容文件会被跳过。
pub fn load(workspace_root: &Path) -> Vec<ContextFile> {
    let mut files = Vec::new();

    for dir in ancestors(workspace_root) {
        if let Some(file) = load_from_dir(&dir) {
            files.push(file);
        }
    }

    files
}

/// 收集某个目录里的上下文文件（若存在）。
///
/// 同一目录只取优先级最高的那个：`AGENTS.override.md` 存在时，同目录的
/// `AGENTS.md` / `CLAUDE.md` 一律不看。
fn load_from_dir(dir: &Path) -> Option<ContextFile> {
    for name in CANDIDATES {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                let content = String::from_utf8_lossy(strip_bom(&bytes)).into_owned();
                if content.trim().is_empty() {
                    // 空文件等于没写，继续试下一个候选而非放弃整个目录
                    continue;
                }
                return Some(ContextFile { path, content });
            }
            Err(e) => {
                tracing::warn!("读取上下文文件 {} 失败：{e}", path.display());
                continue;
            }
        }
    }
    None
}

/// 去掉 UTF-8 BOM。Windows 记事本保存的文件常带 BOM，不剥掉会往提示词
/// 开头塞一个不可见字符。
fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// 从工作区到根目录的全部目录，远的在前。
fn ancestors(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut current = root.to_path_buf();
    loop {
        dirs.push(current.clone());
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => break,
        }
    }
    dirs.reverse();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("测试路径应当有父目录")).expect("创建目录失败");
        fs::write(path, content).expect("写测试文件失败");
    }

    #[test]
    fn loads_agents_md_from_workspace() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        write(&dir.path().join("AGENTS.md"), "子目录约定");

        let files = load(dir.path());
        assert!(
            files.iter().any(|f| f.content == "子目录约定"),
            "工作区内的 AGENTS.md 应当被加载：{files:?}"
        );
    }

    #[test]
    fn collects_ancestors_farthest_first() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        let child = dir.path().join("child");
        write(&dir.path().join("AGENTS.md"), "父目录约定");
        write(&child.join("AGENTS.md"), "子目录约定");

        let files = load(&child);
        let parent_pos = files
            .iter()
            .position(|f| f.content == "父目录约定")
            .expect("应当加载父目录约定");
        let child_pos = files
            .iter()
            .position(|f| f.content == "子目录约定")
            .expect("应当加载子目录约定");
        assert!(
            parent_pos < child_pos,
            "远的目录应当排在近的前面：父={parent_pos}，子={child_pos}"
        );
    }

    #[test]
    fn override_replaces_agents_and_claude_in_same_dir() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        write(&dir.path().join("AGENTS.md"), "普通");
        write(&dir.path().join("CLAUDE.md"), "claude");
        write(&dir.path().join("AGENTS.override.md"), "覆盖");

        let files = load(dir.path());
        let own: Vec<_> = files
            .iter()
            .filter(|f| f.path.starts_with(dir.path()))
            .collect();
        assert_eq!(own.len(), 1, "同目录只能取一个：{own:?}");
        assert_eq!(own[0].content, "覆盖");
    }

    #[test]
    fn claude_is_fallback_when_no_agents() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        write(&dir.path().join("CLAUDE.md"), "claude only");

        let files = load(dir.path());
        assert!(
            files.iter().any(|f| f.content == "claude only"),
            "没有 AGENTS.md 时应当回退到 CLAUDE.md：{files:?}"
        );
    }

    #[test]
    fn strips_utf8_bom() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        let path = dir.path().join("AGENTS.md");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("带 BOM 的内容".as_bytes());
        fs::write(&path, bytes).expect("写测试文件失败");

        let files = load(dir.path());
        let own = files
            .iter()
            .find(|f| f.path.starts_with(dir.path()))
            .expect("应当加载该文件");
        assert_eq!(own.content, "带 BOM 的内容", "BOM 应当被剥掉");
    }

    #[test]
    fn empty_dir_contributes_nothing() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        let files = load(dir.path());
        assert!(
            !files.iter().any(|f| f.path.starts_with(dir.path())),
            "空目录不应贡献上下文文件：{files:?}"
        );
    }

    /// 空的 override 不应吞掉同目录有效的 AGENTS.md：空文件等于没写。
    #[test]
    fn empty_override_falls_through_to_agents_md() {
        let dir = tempfile::tempdir().expect("创建临时目录失败");
        write(&dir.path().join("AGENTS.override.md"), "   ");
        write(&dir.path().join("AGENTS.md"), "有效内容");

        let files = load(dir.path());
        let own: Vec<_> = files
            .iter()
            .filter(|f| f.path.starts_with(dir.path()))
            .collect();
        assert_eq!(own.len(), 1, "空 override 后应继续取 AGENTS.md：{own:?}");
        assert_eq!(own[0].content, "有效内容");
    }
}
