//! 应用全局状态。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_core::{LlmClient, LlmConfig, Registry, Result, Session};
use agent_host::tools::ReadRegistry;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// 当前会话在存储层里的身份：会话 id 属于**工作区** —— 每个工作区有一组
/// 会话，正在编辑的那段由 `session_id` 指出。新建/切换会话时替换它。
pub const DEFAULT_SESSION_ID: &str = "default";

/// 一个进行中轮次的可中断状态。
///
/// `cancel` 立即中止（流式与工具都会响应），`preempt` 是「插队」信号：
/// 只让轮次在当前 tool call 结束后停在一个干净边界，不打断正在执行的命令。
/// `turn_id` 用来校验插队信号是不是发给这一轮 —— 排队会连续启动多个轮次，
/// 迟到的信号不能误伤下一轮。
pub struct ActiveTurn {
    pub turn_id: String,
    pub cancel: CancellationToken,
    pub preempt: Arc<AtomicBool>,
}

pub struct AppState {
    pub config: RwLock<LlmConfig>,
    pub client: LlmClient,
    pub registry: Registry,
    /// Read / Write / Edit 共享，用于「编辑前必须先读」的校验。
    pub read_registry: Arc<ReadRegistry>,
    /// 工具可访问的目录边界。所有文件操作都限制在这之下。
    pub workspace_root: RwLock<PathBuf>,
    /// 会话的唯一数据源。发给模型的消息由它投影得出，UI 也直接消费它 ——
    /// 只有一份数据，不存在两边不一致的问题。
    pub session: Mutex<Session>,
    /// 当前会话 id。新建会话时换新值，列表页据此标记正在编辑的那段。
    pub session_id: RwLock<String>,
    /// 会话文件所在目录。
    pub app_data: PathBuf,
    /// 当前进行中轮次的取消与插队信号。新轮次开始时替换，取消时触发。
    pub active_turn: Mutex<Option<ActiveTurn>>,
    /// 轮次串行化锁：同一时刻只允许一个 `send_message` 在跑，
    /// 会话切换/新建等需要改动会话的命令也先拿它，避免读到轮次中的半成品。
    pub turn_gate: Mutex<()>,
}

impl AppState {
    pub fn new(app_data: PathBuf) -> Result<Self> {
        let read_registry = Arc::new(ReadRegistry::new());

        // 密钥从系统凭据管理器恢复；其余配置由前端启动时从 store 读出后推过来
        let config = LlmConfig {
            api_key: agent_host::secret::load(),
            ..LlmConfig::default()
        };

        Ok(Self {
            config: RwLock::new(config),
            client: LlmClient::new()?,
            registry: agent_host::tools::registry(read_registry.clone()),
            read_registry,
            workspace_root: RwLock::new(default_workspace()),
            session: Mutex::new(Session::default()),
            session_id: RwLock::new(DEFAULT_SESSION_ID.to_string()),
            app_data,
            active_turn: Mutex::new(None),
            turn_gate: Mutex::new(()),
        })
    }

    /// 取消正在进行的轮次（如果有）。
    pub async fn cancel_active(&self) {
        if let Some(turn) = self.active_turn.lock().await.take() {
            turn.cancel.cancel();
        }
    }

    /// 通知指定轮次「插队」：在下一个安全边界（当前 tool call 结束后）停下。
    ///
    /// 只有 `turn_id` 对得上的轮次才响应 —— 迟到信号在目标轮次结束后到来时，
    /// 不该打到排队接续的新一轮上。
    pub async fn preempt_active(&self, turn_id: &str) {
        let guard = self.active_turn.lock().await;
        if let Some(turn) = guard.as_ref() {
            if turn.turn_id == turn_id {
                turn.preempt.store(true, Ordering::SeqCst);
            }
        }
    }

    /// 把当前会话落盘。失败只记日志 —— 存不下不该影响正在进行的对话。
    pub async fn persist(&self) {
        let session = self.session.lock().await.clone();
        let workspace = self.workspace_root.read().await.clone();
        let model = self.config.read().await.model.clone();
        let session_id = self.session_id.read().await.clone();
        if let Err(e) =
            agent_host::persist::save(&self.app_data, &workspace, &model, &session_id, &session)
                .await
        {
            tracing::warn!("保存会话失败：{e}");
        }
    }
}

/// 默认工作区。
///
/// 打包后进程的当前目录是安装目录，拿它当工作区毫无意义，
/// 所以退回用户主目录，等用户自己选。dev 模式下 cwd 就是项目根，正好可用。
fn default_workspace() -> PathBuf {
    std::env::current_dir()
        .ok()
        .filter(|p| p.join("Cargo.toml").exists() || p.join("package.json").exists())
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}
