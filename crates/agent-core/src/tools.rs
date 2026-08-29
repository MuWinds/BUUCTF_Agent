//! 工具抽象。
//!
//! 接口定义在 core（轮次循环必须能调用工具），**实现留在应用层** ——
//! 工具的权限边界和 UI 呈现与宿主强相关，塞进 core 会让 core 背上
//! 不该有的假设。
//!
//! 用 `dyn Tool` 而非 enum 分发：工具执行本身是毫秒到秒级 IO，
//! 动态分发开销可忽略；而未来接 MCP、子 agent、用户自定义工具时，
//! 实现体不是编译期已知的，enum 会直接堵死这条路。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::events::ToolResultBody;
use crate::llm::types::ToolDef;
use crate::sink::ProgressReporter;

/// 工具执行的产出。
///
/// **两份输出刻意分开**：UI 要结构化数据才能渲染彩色 diff 和可折叠面板，
/// 模型只需要一句摘要。混成一份会让"好看"和"省 token"互相拉扯。
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    /// 回灌给模型的文本。
    pub llm_text: String,
    /// 推给 UI 的结构化结果。
    pub ui: ToolResultBody,
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// 模型有机会自己纠正的错误，结果会回灌让它重试。
    ///
    /// 消息措辞直接决定 agent 可用性 —— 必须写清楚**怎么改**，
    /// 而不只是说哪里错了。
    #[error("{0}")]
    Recoverable(String),

    /// 无法通过重试解决，整轮终止。
    #[error("{0}")]
    Fatal(String),
}

impl ToolError {
    pub fn recoverable(msg: impl Into<String>) -> Self {
        Self::Recoverable(msg.into())
    }
    pub fn fatal(msg: impl Into<String>) -> Self {
        Self::Fatal(msg.into())
    }
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Fatal(_))
    }
}

/// 工具运行环境中与具体调用无关的部分，由宿主提供。
#[derive(Debug, Clone)]
pub struct ToolEnv {
    /// 工作区根目录。所有路径参数都必须校验落在此目录之下。
    pub workspace_root: PathBuf,
}

/// 单次工具调用的上下文，由轮次循环为每个调用派生。
pub struct ToolCtx {
    /// 工作区根目录。所有路径参数都必须校验落在此目录之下。
    pub workspace_root: PathBuf,
    /// 用户点"停止"时触发。长时间运行的工具应当在循环里检查它。
    pub cancel: CancellationToken,
    /// 执行期间的增量输出出口，已绑定到本次调用的卡片。
    pub progress: ProgressReporter,
}

#[async_trait]
pub trait Tool: Send + Sync {
    /// 给模型看的函数名，如 `Read`。
    fn name(&self) -> &'static str;

    /// 给模型看的说明。这是 prompt 的一部分，措辞直接影响调用质量。
    fn description(&self) -> &'static str;

    /// JSON Schema 形式的参数定义。
    fn parameters_schema(&self) -> Value;

    /// 折叠状态下卡片显示的一行摘要，如 `Read(src/main.rs)`。
    ///
    /// 在**执行前**调用，因此参数可能不合法 —— 实现里不要 unwrap。
    fn preview(&self, args: &Value) -> String;

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError>;
}

/// 工具注册表。
///
/// `BTreeMap` 保证发给模型的工具顺序稳定 —— 顺序变化会让 prompt 缓存失效。
#[derive(Default)]
pub struct Registry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name(), tool);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// 生成发给模型的工具定义列表。
    pub fn definitions(&self) -> Vec<ToolDef> {
        self.tools
            .values()
            .map(|t| ToolDef::function(t.name(), t.description(), t.parameters_schema()))
            .collect()
    }

    /// 模型调了不存在的工具时，给它一个能自我纠正的提示。
    pub fn unknown_tool_message(&self, name: &str) -> String {
        let available: Vec<_> = self.tools.keys().copied().collect();
        format!(
            "不存在名为 `{name}` 的工具。可用工具：{}。请从中选择。",
            available.join("、")
        )
    }
}
