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

/// 工具对系统提示词的贡献。
///
/// 与发给 API 的 `description` 分开：`snippet` 是系统提示词里「可用工具」
/// 清单的一行，`guidelines` 是额外的使用准则。宿主把启用工具的这些贡献
/// 动态组装进系统提示词，而不是让每个工具把一整套说明塞进 `description`。
#[derive(Debug, Clone, Copy)]
pub struct PromptContribution {
    /// 一行工具介绍，如 `- Read：读取文件内容`。
    pub snippet: &'static str,
    /// 与该工具相关的使用准则，逐条加入系统提示词。
    pub guidelines: &'static [&'static str],
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

    /// 对系统提示词的贡献。默认不贡献任何内容 —— 只有明确写了的
    /// 工具才出现在「可用工具」清单里，避免模板工具/未来内部工具
    /// 悄悄暴露给模型。
    fn prompt_contribution(&self) -> PromptContribution {
        PromptContribution {
            snippet: "",
            guidelines: &[],
        }
    }

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

    /// 系统提示词里「可用工具」清单：名字 → 一行简介。
    ///
    /// 顺序与 `definitions()` 一致（BTreeMap 稳定排序），只收录贡献了
    /// snippet 的工具 —— 没有贡献说明的（比如未来加的内部工具）不暴露。
    pub fn prompt_snippets(&self) -> Vec<(&'static str, &'static str)> {
        self.tools
            .iter()
            .filter_map(|(name, t)| {
                let contribution = t.prompt_contribution();
                (!contribution.snippet.is_empty()).then_some((*name, contribution.snippet))
            })
            .collect()
    }

    /// 全部工具的准则，按注册顺序去重 —— 多个工具重复的准则只写一次。
    pub fn prompt_guidelines(&self) -> Vec<&'static str> {
        let mut seen = std::collections::HashSet::new();
        self.tools
            .values()
            .flat_map(|t| t.prompt_contribution().guidelines.iter().copied())
            .filter(|g| seen.insert(*g))
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
