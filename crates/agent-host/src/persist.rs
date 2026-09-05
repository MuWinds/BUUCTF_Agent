//! 会话存储：按工作区归档的多个会话。
//!
//! 参考 DeepSeek Harness（`~/.dsh/sessions/<workspace>/<session>/…`）与
//! Codex（`~/.codex/sessions/<date>/<rollout>.jsonl`）的做法：会话按工作区
//! 分目录存放，同一工作区下保留多段历史，而不是全局只有一份。
//!
//! 布局：
//!
//! ```text
//! <app_data>/sessions/<workspace_slug>/<session_id>.json
//! ```
//!
//! 每个文件同时携带元信息（标题、模型、时间戳）与会话正文 —— 列表页只读
//! 元信息即可，不用把整段历史都解析出来。
//!
//! 写入走「临时文件 + 原子重命名」：直接覆写的话，进程在写到一半时被杀
//! 会留下一个残缺的 JSON，下次启动直接读不出来。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_core::session::Entry;
use agent_core::Session;
use serde::{Deserialize, Serialize};

/// 旧版全局唯一的会话文件名。首次启动时会被迁移进新布局。
const LEGACY_FILE: &str = "session.json";

/// 会话文件的完整内容：元信息 + 会话正文。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub id: String,
    /// 列表页显示的一行标题：取第一条用户消息。
    pub title: String,
    pub workspace: String,
    pub model: String,
    /// Unix 毫秒。
    pub created_at: u64,
    pub updated_at: u64,
    pub session: Session,
}

/// 列表页用到的会话摘要，不含会话正文。
#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub workspace: String,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    /// 非 system 条目的数量（用户 + 助手）。
    pub message_count: usize,
}

/// `list_sessions` 的返回：当前会话 id + 全部摘要。
///
/// 前端靠 `current_id` 在列表里标出正在编辑的那段，删它时也知道要清空界面。
#[derive(Debug, Clone, Serialize)]
pub struct SessionList {
    pub current_id: String,
    pub sessions: Vec<SessionSummary>,
}

fn sessions_root(app_data: &Path) -> PathBuf {
    app_data.join("sessions")
}

/// 把工作区路径编码成文件系统安全的目录名，参考 .dsh 的做法（分隔符转 `-`）。
pub fn workspace_slug(workspace: &Path) -> String {
    workspace
        .display()
        .to_string()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn workspace_dir(app_data: &Path, workspace: &Path) -> PathBuf {
    sessions_root(app_data).join(workspace_slug(workspace))
}

fn session_file(app_data: &Path, workspace: &Path, id: &str) -> PathBuf {
    workspace_dir(app_data, workspace).join(format!("{id}.json"))
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 标题取第一条用户消息，超过 30 字符截断并加省略号。
fn title_of(session: &Session) -> String {
    let title = session
        .entries
        .iter()
        .find_map(|e| match e {
            Entry::User { text } => Some(text.trim()),
            _ => None,
        })
        .unwrap_or("新对话");

    let mut out: String = title.chars().take(30).collect();
    if title.chars().count() > 30 {
        out.push('…');
    }
    out
}

fn summary_of(file: &SessionFile) -> SessionSummary {
    SessionSummary {
        id: file.id.clone(),
        title: file.title.clone(),
        workspace: file.workspace.clone(),
        model: file.model.clone(),
        created_at: file.created_at,
        updated_at: file.updated_at,
        message_count: file
            .session
            .entries
            .iter()
            .filter(|e| !matches!(e, Entry::System { .. }))
            .count(),
    }
}

async fn load_file(path: &Path) -> Option<SessionFile> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&text).ok()
}

/// 保存会话。
///
/// 空会话直接删文件，避免列表里出现一堆空壳。已存在的会话保留创建时间，
/// 否则每轮保存都会把它顶成"最新创建"。
pub async fn save(
    app_data: &Path,
    workspace: &Path,
    model: &str,
    id: &str,
    session: &Session,
) -> std::io::Result<()> {
    let path = session_file(app_data, workspace, id);

    if session.is_empty() {
        return match tokio::fs::remove_file(&path).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }

    tokio::fs::create_dir_all(workspace_dir(app_data, workspace)).await?;

    let now = now_millis();
    let created_at = load_file(&path).await.map(|f| f.created_at).unwrap_or(now);

    let data = SessionFile {
        id: id.to_string(),
        title: title_of(session),
        workspace: workspace.display().to_string(),
        model: model.to_string(),
        created_at,
        updated_at: now,
        session: session.clone(),
    };

    let json = serde_json::to_string(&data)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // 先写临时文件再原子重命名：中途被杀也不会留下半个 JSON
    let temp = path.with_extension("json.tmp");
    tokio::fs::write(&temp, json).await?;
    tokio::fs::rename(&temp, &path).await?;

    Ok(())
}

pub async fn load(app_data: &Path, workspace: &Path, id: &str) -> Option<Session> {
    load_file(&session_file(app_data, workspace, id))
        .await
        .map(|f| f.session)
}

/// 列出某工作区下的全部会话，最近更新的在前。
pub async fn list(app_data: &Path, workspace: &Path) -> Vec<SessionSummary> {
    let mut summaries = Vec::new();

    let dir = workspace_dir(app_data, workspace);
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return summaries;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("json") {
            continue;
        }
        if let Some(file) = load_file(&path).await {
            summaries.push(summary_of(&file));
        }
    }

    summaries.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    summaries
}

pub async fn delete(app_data: &Path, workspace: &Path, id: &str) -> std::io::Result<()> {
    match tokio::fs::remove_file(session_file(app_data, workspace, id)).await {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// 启动时装载：迁移旧版单一会话文件，并返回当前工作区最近一次会话。
///
/// 返回 `(会话, 会话 id)`。没有任何历史时返回空会话与新生成的 id。
pub async fn bootstrap(app_data: &Path, workspace: &Path, model: &str) -> (Session, String) {
    migrate_legacy(app_data, workspace, model).await;

    for summary in list(app_data, workspace).await {
        if let Some(session) = load(app_data, workspace, &summary.id).await {
            return (session, summary.id);
        }
    }

    (Session::default(), new_id())
}

/// 把旧版全局唯一 `session.json` 导入为当前工作区下的一个会话。
///
/// 旧版没有元信息，标题与时间戳按当前内容推导；导入后删除旧文件，
/// 下次启动不会再走这条路。文件损坏则直接删掉，不反复报错。
async fn migrate_legacy(app_data: &Path, workspace: &Path, model: &str) {
    let legacy = app_data.join(LEGACY_FILE);
    let text = match tokio::fs::read_to_string(&legacy).await {
        Ok(t) => t,
        Err(_) => return,
    };
    let Ok(session) = serde_json::from_str::<Session>(&text) else {
        let _ = tokio::fs::remove_file(&legacy).await;
        return;
    };
    if session.is_empty() {
        let _ = tokio::fs::remove_file(&legacy).await;
        return;
    }

    let id = new_id();
    if let Err(e) = save(app_data, workspace, model, &id, &session).await {
        tracing::warn!("迁移旧会话失败：{e}");
    }
    let _ = tokio::fs::remove_file(&legacy).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> Session {
        let mut session = Session::default();
        session.push_system("sys");
        session.push_user("你好，帮我看看这个项目");
        session.start_assistant();
        session.push_text("好的，我先读一下 README");
        session
    }

    fn workspace() -> &'static Path {
        Path::new("C:\\Users\\me\\proj")
    }

    #[tokio::test]
    async fn round_trips_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let session = sample_session();

        save(dir.path(), workspace(), "gpt-4o", "abc", &session)
            .await
            .expect("保存应当成功");
        let restored = load(dir.path(), workspace(), "abc")
            .await
            .expect("应当能读回");
        assert_eq!(
            restored.to_messages().len(),
            session.to_messages().len(),
            "恢复后的消息数量应当一致"
        );
    }

    #[tokio::test]
    async fn lists_sessions_newest_first_with_summary() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), workspace(), "gpt-4o", "a", &sample_session())
            .await
            .unwrap();
        save(dir.path(), workspace(), "gpt-4o", "b", &sample_session())
            .await
            .unwrap();

        let list = list(dir.path(), workspace()).await;
        assert_eq!(list.len(), 2);
        let s = &list[0];
        assert_eq!(s.title, "你好，帮我看看这个项目");
        assert_eq!(s.message_count, 2, "system 不计入消息数");
        assert!(s.created_at > 0);
        assert!(s.updated_at >= s.created_at, "更新时间不得早于创建时间");
    }

    #[tokio::test]
    async fn saving_empty_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), workspace(), "gpt-4o", "a", &sample_session())
            .await
            .unwrap();
        assert_eq!(list(dir.path(), workspace()).await.len(), 1);

        save(dir.path(), workspace(), "gpt-4o", "a", &Session::default())
            .await
            .unwrap();
        assert!(list(dir.path(), workspace()).await.is_empty());
    }

    #[tokio::test]
    async fn corrupt_file_is_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir_all(workspace_dir(dir.path(), workspace()))
            .await
            .unwrap();
        tokio::fs::write(session_file(dir.path(), workspace(), "bad"), "{ not json")
            .await
            .unwrap();

        assert!(
            list(dir.path(), workspace()).await.is_empty(),
            "损坏文件应当被跳过"
        );
    }

    #[tokio::test]
    async fn leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), workspace(), "gpt-4o", "a", &sample_session())
            .await
            .unwrap();

        let mut entries = tokio::fs::read_dir(workspace_dir(dir.path(), workspace()))
            .await
            .unwrap();
        let mut names = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        assert!(!names.iter().any(|n| n.ends_with(".tmp")));
    }

    #[tokio::test]
    async fn workspace_slug_is_filesystem_safe() {
        assert_eq!(
            workspace_slug(Path::new("C:\\Users\\me\\proj")),
            "C--Users-me-proj"
        );
        assert_eq!(workspace_slug(Path::new("/home/me/proj")), "-home-me-proj");
    }

    #[tokio::test]
    async fn bootstrap_imports_legacy_single_session() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(LEGACY_FILE);
        let session = sample_session();
        tokio::fs::write(&legacy, serde_json::to_string(&session).unwrap())
            .await
            .unwrap();

        let (loaded, id) = bootstrap(dir.path(), workspace(), "gpt-4o").await;
        assert_eq!(loaded.to_messages().len(), session.to_messages().len());
        assert_eq!(
            load(dir.path(), workspace(), &id)
                .await
                .unwrap()
                .to_messages()
                .len(),
            session.to_messages().len()
        );
        assert!(!legacy.exists(), "导入后旧文件应当删除");
        assert_eq!(list(dir.path(), workspace()).await.len(), 1);
    }

    #[tokio::test]
    async fn bootstrap_returns_empty_when_no_history() {
        let dir = tempfile::tempdir().unwrap();
        let (session, id) = bootstrap(dir.path(), workspace(), "gpt-4o").await;
        assert!(session.is_empty());
        assert!(!id.is_empty());
    }
}
