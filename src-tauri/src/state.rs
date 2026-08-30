//! 应用全局状态。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_core::{LlmClient, LlmConfig, Registry, Result, Session};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::tools::ReadRegistry;

/// 系统提示词。
///
/// 放在应用层而非 core：agent 的人格与职责是产品决策，core 不该规定。
/// 工具清单与准则由注册表里的工具各自贡献（`Tool::prompt_contribution`），
/// 这里只做组装 —— 新增工具时不用改这段，注册进 Registry 就自动出现。
const SYSTEM_PROMPT: &str = "\
你是一个运行在桌面应用中的编程助手，可以读取、检索和修改用户工作区里的文件。

可用工具：
{TOOLS}

工作方式：
{GUIDELINES}
- 工具报错时按错误信息里的提示纠正后重试，不要反复用同样的参数。

回答要求：
- 简洁准确，直接给结论，不要复述用户的问题。
- 代码用 Markdown 代码块并标注语言。
- 引用文件位置时写成 `路径:行号`。";

/// 组装「可用工具」清单：`- 名字：一行简介`，按注册顺序。
fn render_tools(registry: &agent_core::Registry) -> String {
    let snippets = registry.prompt_snippets();
    if snippets.is_empty() {
        return "(无)".to_string();
    }
    snippets
        .into_iter()
        .map(|(name, snippet)| format!("- {name}：{snippet}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 组装工作方式准则：工具的 guidelines 展开成条目。
fn render_guidelines(registry: &agent_core::Registry) -> String {
    let guidelines = registry.prompt_guidelines();
    if guidelines.is_empty() {
        return "- 需要了解代码时先用工具查看，不要凭猜测回答。".to_string();
    }
    let mut lines: Vec<String> = guidelines.into_iter().map(|g| format!("- {g}")).collect();
    lines.insert(
        0,
        "- 需要了解代码时先用工具查看，不要凭猜测回答。".to_string(),
    );
    lines.join("\n")
}

/// 构建完整的系统提示词：固定人格 + 工具清单 + 从工作区向上收集的上下文文件。
///
/// 上下文文件的内容包在 `<project_context>` 块里，每条带来源路径 —— 与 pi 的
/// `<project_instructions path="...">` 同构。没有上下文文件时只返回固定提示词。
pub fn system_prompt(workspace_root: &Path, registry: &agent_core::Registry) -> String {
    let tools = render_tools(registry);
    let guidelines = render_guidelines(registry);

    let mut prompt = SYSTEM_PROMPT
        .replace("{TOOLS}", &tools)
        .replace("{GUIDELINES}", &guidelines);

    let files = crate::context_files::load(workspace_root);

    if files.is_empty() {
        return prompt;
    }

    prompt.push_str("\n\n<project_context>\n");
    prompt.push_str("项目约定（来自工作区及其父目录的 AGENTS.md / CLAUDE.md）：\n\n");

    for file in files {
        prompt.push_str(&format!(
            "<project_instructions path=\"{}\">\n{}\n</project_instructions>\n\n",
            file.path.display(),
            file.content.trim()
        ));
    }

    prompt.push_str("</project_context>");
    prompt
}

/// 当前会话在存储层里的身份：会话 id 属于**工作区** —— 每个工作区有一组
/// 会话，正在编辑的那段由 `session_id` 指出。新建/切换会话时替换它。
pub const DEFAULT_SESSION_ID: &str = "default";

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
            session_id: RwLock::new(DEFAULT_SESSION_ID.to_string()),
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
        let workspace = self.workspace_root.read().await.clone();
        let model = self.config.read().await.model.clone();
        let session_id = self.session_id.read().await.clone();
        if let Err(e) =
            crate::persist::save(&self.app_data, &workspace, &model, &session_id, &session).await
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
