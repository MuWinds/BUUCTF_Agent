//! 配置相关命令。

use agent_core::llm::Probe;
use agent_core::{Error, LlmConfig, Result};
use serde::Serialize;
use tauri::State;

use crate::secret;
use crate::state::AppState;

#[tauri::command]
pub async fn get_llm_config(state: State<'_, AppState>) -> Result<LlmConfig> {
    Ok(state.config.read().await.clone())
}

#[derive(Serialize)]
pub struct SaveResult {
    /// 密钥是否成功存入系统凭据管理器。false 表示只留在内存里，重启后要重填。
    pub key_persisted: bool,
}

#[tauri::command]
pub async fn set_llm_config(state: State<'_, AppState>, config: LlmConfig) -> Result<SaveResult> {
    config.validate().map_err(Error::Config)?;

    // 密钥单独走系统凭据存储，不跟其他配置一起明文落盘
    let key_persisted = secret::save(config.api_key.trim());

    *state.config.write().await = config;
    Ok(SaveResult { key_persisted })
}

/// 从系统凭据管理器中删除已保存的密钥。
///
/// 单独成一个命令而不是"保存空值" —— 后者会让启动流程里的任何一次
/// 读取失败都变成静默的数据丢失。
#[tauri::command]
pub async fn clear_api_key(state: State<'_, AppState>) -> Result<()> {
    secret::clear();
    state.config.write().await.api_key.clear();
    Ok(())
}

/// 连通性自检。走与真实对话完全相同的流式路径。
#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>, config: LlmConfig) -> Result<String> {
    config.validate().map_err(Error::Config)?;

    let probe = state
        .client
        .probe(&config, std::time::Duration::from_secs(30))
        .await?;

    match probe {
        Probe::Ok => Ok(format!("连接成功：{}", config.model)),
        Probe::EmptyStream => Ok("连接成功，但服务端未返回任何内容".into()),
        Probe::ClosedImmediately => Err(Error::Config("服务端建立连接后立即关闭了数据流".into())),
        Probe::Timeout => Err(Error::Config("等待响应超时（30 秒）".into())),
    }
}
