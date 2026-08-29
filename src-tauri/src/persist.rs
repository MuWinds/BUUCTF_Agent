//! 会话持久化。
//!
//! 只存 [`Session`] 一份数据 —— 它同时承载发给模型的信息和界面展示所需的
//! 结构，发给模型的消息由它投影得出。这样不存在"两份数据不一致"的问题。
//!
//! 写入走「临时文件 + 原子重命名」：直接覆写的话，进程在写到一半时被杀
//! 会留下一个残缺的 JSON，下次启动直接读不出来。

use std::path::{Path, PathBuf};

use agent_core::Session;

const FILE_NAME: &str = "session.json";

/// 会话文件的位置。
pub fn path(app_data: &Path) -> PathBuf {
    app_data.join(FILE_NAME)
}

/// 读取上次的会话。文件不存在或损坏时返回空会话，不让启动失败。
pub async fn load(app_data: &Path) -> Session {
    let file = path(app_data);

    let text = match tokio::fs::read_to_string(&file).await {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Session::default(),
        Err(e) => {
            tracing::warn!("读取会话失败：{e}");
            return Session::default();
        }
    };

    match serde_json::from_str(&text) {
        Ok(session) => session,
        Err(e) => {
            // 结构变更或文件损坏。备份一份便于排查，然后从空会话开始 ——
            // 总比让用户卡在启动失败上要好。
            tracing::warn!("会话文件无法解析：{e}");
            let backup = file.with_extension("json.bak");
            let _ = tokio::fs::rename(&file, &backup).await;
            Session::default()
        }
    }
}

/// 保存会话。
///
/// 空会话直接删文件，避免下次启动加载出一个空壳。
pub async fn save(app_data: &Path, session: &Session) -> std::io::Result<()> {
    let file = path(app_data);

    if session.is_empty() {
        return match tokio::fs::remove_file(&file).await {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }

    tokio::fs::create_dir_all(app_data).await?;

    let json = serde_json::to_string(session)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // 先写临时文件再原子重命名：中途被杀也不会留下半个 JSON
    let temp = file.with_extension("json.tmp");
    tokio::fs::write(&temp, json).await?;
    tokio::fs::rename(&temp, &file).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_empty_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).await.is_empty());
    }

    #[tokio::test]
    async fn round_trips_a_session() {
        let dir = tempfile::tempdir().unwrap();

        let mut session = Session::default();
        session.push_user("hi");
        session.start_assistant();
        session.push_text("hello");

        save(dir.path(), &session).await.expect("保存应当成功");
        let restored = load(dir.path()).await;

        assert_eq!(
            restored.to_messages().len(),
            session.to_messages().len(),
            "恢复后的消息数量应当一致"
        );
    }

    /// 损坏的文件不该让启动失败，且应留下备份。
    #[tokio::test]
    async fn recovers_from_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(path(dir.path()), "{ not json")
            .await
            .unwrap();

        assert!(load(dir.path()).await.is_empty(), "损坏时应当退回空会话");
        assert!(
            path(dir.path()).with_extension("json.bak").exists(),
            "应当留下备份文件便于排查"
        );
    }

    /// 保存空会话等于清除，不该留下空壳文件。
    #[tokio::test]
    async fn saving_empty_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();

        let mut session = Session::default();
        session.push_user("hi");
        save(dir.path(), &session).await.unwrap();
        assert!(path(dir.path()).exists());

        save(dir.path(), &Session::default()).await.unwrap();
        assert!(!path(dir.path()).exists(), "空会话应当把文件删掉");
    }

    /// 保存后不该残留临时文件。
    #[tokio::test]
    async fn leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();

        let mut session = Session::default();
        session.push_user("hi");
        save(dir.path(), &session).await.unwrap();

        assert!(!path(dir.path()).with_extension("json.tmp").exists());
    }
}
