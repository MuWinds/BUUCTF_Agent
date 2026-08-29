//! 工作区路径沙箱。
//!
//! 所有接受路径参数的工具都必须先过这里。模型会（有意或无意地）生成
//! `../../Windows/System32/...` 这样的路径，没有这层校验就是任人宰割。

use std::path::{Path, PathBuf};

use agent_core::ToolError;

/// 把模型给的路径解析成工作区内的绝对路径。
///
/// 拒绝所有逃逸到工作区之外的路径。错误消息写明工作区在哪，
/// 好让模型据此改用正确的相对路径重试。
pub fn resolve(workspace_root: &Path, input: &str) -> Result<PathBuf, ToolError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(ToolError::recoverable("路径不能为空"));
    }

    let candidate = {
        let p = Path::new(raw);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            workspace_root.join(p)
        }
    };

    // 先做纯词法归一，再比对前缀。
    // 不用 canonicalize：它要求路径必须已存在，而 Write 工具恰恰要处理不存在的路径。
    let normalized = normalize(&candidate);
    let root = normalize(workspace_root);

    if !normalized.starts_with(&root) {
        return Err(ToolError::recoverable(format!(
            "路径 `{raw}` 在工作区之外。工作区是 `{}`，只能访问它下面的文件。",
            root.display()
        )));
    }

    Ok(normalized)
}

/// 词法归一：消掉 `.` 与 `..`，不碰文件系统。
///
/// 刻意不解析符号链接 —— 那需要访问磁盘，且对不存在的路径无解。
/// 代价是符号链接理论上可以指向工作区外，这个缺口留待后续处理。
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // 弹不动就说明已经在根上，`..` 无处可去，直接丢弃
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 把绝对路径转成相对工作区的显示形式。
///
/// 给模型和 UI 看的路径应当短且稳定；打印 `C:\Users\...\project\src\main.rs`
/// 既占 token 又暴露无关信息。
pub fn display(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        PathBuf::from(if cfg!(windows) { r"C:\work" } else { "/work" })
    }

    #[test]
    fn resolves_relative_paths() {
        let got = resolve(&root(), "src/main.rs").expect("应当接受工作区内的相对路径");
        assert_eq!(got, root().join("src").join("main.rs"));
    }

    #[test]
    fn collapses_dot_segments() {
        let got = resolve(&root(), "src/./util/../main.rs").expect("应当能归一化");
        assert_eq!(got, root().join("src").join("main.rs"));
    }

    #[test]
    fn rejects_escaping_paths() {
        let err = resolve(&root(), "../../etc/passwd").expect_err("必须拒绝逃逸路径");
        assert!(err.to_string().contains("工作区之外"));
    }

    #[test]
    fn rejects_absolute_paths_outside() {
        let outside = if cfg!(windows) {
            r"C:\Windows\System32"
        } else {
            "/etc/passwd"
        };
        resolve(&root(), outside).expect_err("必须拒绝工作区外的绝对路径");
    }

    #[test]
    fn accepts_absolute_paths_inside() {
        let inside = root().join("src");
        let got =
            resolve(&root(), &inside.display().to_string()).expect("工作区内的绝对路径应当接受");
        assert_eq!(got, inside);
    }

    /// 前缀相同但并非子目录的兄弟目录不能放行（`C:\work2` vs `C:\work`）。
    #[test]
    fn rejects_sibling_with_shared_prefix() {
        let sibling = if cfg!(windows) {
            r"C:\work2\secret"
        } else {
            "/work2/secret"
        };
        resolve(&root(), sibling).expect_err("兄弟目录不该被当成工作区内");
    }

    #[test]
    fn rejects_empty_path() {
        resolve(&root(), "   ").expect_err("空路径应当报错");
    }

    #[test]
    fn displays_relative_with_forward_slashes() {
        let full = root().join("src").join("main.rs");
        assert_eq!(display(&root(), &full), "src/main.rs");
    }
}
