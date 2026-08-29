//! 工具实现。
//!
//! 放在应用层而非 `agent-core`：工具的权限边界（能碰哪些路径、要不要审批）
//! 和 UI 呈现方式都与宿主强相关。core 只定义 `Tool` trait。

mod bash;
mod diff;
mod edit;
mod glob;
mod grep;
mod path;
mod read;
mod read_registry;
mod write;

use std::sync::Arc;

use agent_core::Registry;

pub use read_registry::ReadRegistry;

/// 构建工具注册表。
///
/// `ReadRegistry` 由 Read / Write / Edit 共享 —— Read 往里记，写类工具据此
/// 校验。它不放进 `ToolCtx` 是因为那是 core 的类型，不该知道这个应用层概念。
pub fn registry(read_registry: Arc<ReadRegistry>) -> Registry {
    let mut registry = Registry::new();
    registry.register(Arc::new(read::ReadTool {
        registry: read_registry.clone(),
    }));
    registry.register(Arc::new(glob::GlobTool));
    registry.register(Arc::new(grep::GrepTool));
    registry.register(Arc::new(write::WriteTool {
        registry: read_registry.clone(),
    }));
    registry.register(Arc::new(edit::EditTool {
        registry: read_registry,
    }));
    registry.register(Arc::new(bash::BashTool));
    registry
}
