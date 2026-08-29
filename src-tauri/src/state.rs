//! 应用全局状态。

use std::path::PathBuf;
use std::sync::Arc;

use agent_core::{LlmClient, LlmConfig, Registry, Result, Session};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::tools::ReadRegistry;

/// 系统提示词。
///
/// 放在应用层而非 core：agent 的人格与职责是产品决策，core 不该规定。
pub const SYSTEM_PROMPT: &str = "\
你是一个运行在桌面应用中的编程助手，可以读取、检索和修改用户工作区里的文件。

工作方式：
- 需要了解代码时先用工具查看，不要凭猜测回答。
- 用 Glob 按文件名找文件，用 Grep 按内容搜索，用 Read 读具体内容。
- 修改文件前必须先 Read 它。改局部用 Edit，创建新文件用 Write。
- Edit 的 old_string 必须与原文逐字符一致（含缩进），且在文件中唯一。
- 需要运行命令、编译、测试时用 Bash。查找文件和搜索内容不要用 Bash，
  Glob 和 Grep 更快且输出更适合阅读。
- 工具报错时按错误信息里的提示纠正后重试，不要反复用同样的参数。

回答要求：
- 简洁准确，直接给结论，不要复述用户的问题。
- 代码用 Markdown 代码块并标注语言。
- 引用文件位置时写成 `路径:行号`。";

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
    /// 会话文件所在目录。
    pub app_data: PathBuf,
    /// 当前进行中轮次的取消令牌。新轮次开始时替换，取消时触发。
    pub active_turn: Mutex<Option<CancellationToken>>,
}

impl AppState {
    pub fn new(app_data: PathBuf) -> Result<Self> {
        let read_registry = Arc::new(ReadRegistry::new());

        // 密钥从系统凭据管理器恢复；其余配置由前端启动时从 store 读出后推过来
        let config = LlmConfig {
            api_key: crate::secret::load(),
            ..LlmConfig::default()
        };

        Ok(Self {
            config: RwLock::new(config),
            client: LlmClient::new()?,
            registry: crate::tools::registry(read_registry.clone()),
            read_registry,
            workspace_root: RwLock::new(default_workspace()),
            session: Mutex::new(Session::default()),
            app_data,
            active_turn: Mutex::new(None),
        })
    }

    /// 取消正在进行的轮次（如果有）。
    pub async fn cancel_active(&self) {
        if let Some(token) = self.active_turn.lock().await.take() {
            token.cancel();
        }
    }

    /// 把当前会话落盘。失败只记日志 —— 存不下不该影响正在进行的对话。
    pub async fn persist(&self) {
        let session = self.session.lock().await.clone();
        if let Err(e) = crate::persist::save(&self.app_data, &session).await {
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
