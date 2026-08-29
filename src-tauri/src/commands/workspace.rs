//! 工作区相关命令。

use std::path::PathBuf;

use agent_core::{Error, Result};
use tauri::State;

use crate::state::AppState;

#[tauri::command]
pub async fn get_workspace(state: State<'_, AppState>) -> Result<String> {
    Ok(state.workspace_root.read().await.display().to_string())
}

/// 切换工作区。
///
/// 会一并清空会话历史：工具读到的文件内容都属于旧工作区，
/// 留着只会让模型基于错误的上下文作答。
#[tauri::command]
pub async fn set_workspace(state: State<'_, AppState>, path: String) -> Result<String> {
    let path = PathBuf::from(path.trim());

    if !path.is_dir() {
        return Err(Error::Config(format!("`{}` 不是有效目录", path.display())));
    }

    // 存绝对路径：沙箱校验靠前缀比对，相对路径会让比对失去意义
    let absolute = path
        .canonicalize()
        .map_err(|e| Error::Config(format!("无法解析目录：{e}")))?;

    // Windows 的 canonicalize 会加上 \\?\ 前缀，显示和比对都很别扭，去掉
    let cleaned = strip_unc_prefix(&absolute);

    state.cancel_active().await;
    state.session.lock().await.clear();
    // 旧工作区的读取记录对新工作区毫无意义，留着只会让「编辑前须读」的校验失准
    state.read_registry.clear();
    *state.workspace_root.write().await = cleaned.clone();
    state.persist().await;

    Ok(cleaned.display().to_string())
}

/// 去掉 Windows 扩展长度路径前缀 `\\?\`。
fn strip_unc_prefix(path: &std::path::Path) -> PathBuf {
    let text = path.display().to_string();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}
