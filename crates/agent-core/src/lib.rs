//! Coding agent 的可复用核心。
//!
//! 这个 crate 不依赖任何 GUI 框架，只负责三件事：
//!
//! 1. **LLM 协议** —— OpenAI 兼容的流式 chat completions 客户端
//! 2. **轮次循环** —— 消费流、聚合增量、响应取消，产出结构化结果
//! 3. **事件出口** —— 通过 [`EventSink`] trait 把过程推给外部，
//!    由宿主决定是走 IPC、写终端还是收进测试断言
//!
//! 工具（Read/Write/Bash 等）刻意留在应用层：它们的权限边界和 UI 呈现
//! 与宿主强相关，塞进 core 只会让 core 背上不该有的假设。
//!
//! # 示例
//!
//! ```no_run
//! # use agent_core::{LlmClient, LlmConfig, ThrottledSink, EventSink, AgentEvent,
//! #                  Registry, Session, ToolEnv, turn};
//! # use std::sync::Arc;
//! # use tokio_util::sync::CancellationToken;
//! struct Printer;
//! impl EventSink for Printer {
//!     fn emit(&self, event: AgentEvent) {
//!         if let AgentEvent::AssistantDelta { text, .. } = event {
//!             print!("{text}");
//!         }
//!     }
//! }
//!
//! # async fn demo() -> agent_core::Result<()> {
//! let client = LlmClient::new()?;
//! let config = LlmConfig::default();
//! let mut session = Session::default();
//! session.push_user("你好");
//!
//! let mut sink = ThrottledSink::new(Arc::new(Printer), "turn-1");
//! let env = ToolEnv { workspace_root: std::path::PathBuf::from(".") };
//!
//! let outcome = turn::run(
//!     &client, &config, &mut session, &Registry::new(), &env,
//!     &mut sink, CancellationToken::new(),
//! ).await;
//! println!("\n结束原因：{}", outcome.finish_reason);
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod error;
pub mod events;
pub mod llm;
pub mod session;
pub mod sink;
pub mod tools;
pub mod turn;

pub use config::LlmConfig;
pub use error::{Error, Result};
pub use events::{AgentEvent, DiffHunk, DiffLine, DiffSegment, DiffTag, ToolResultBody};
pub use llm::types::{Message, Role, Usage};
pub use llm::LlmClient;
pub use session::{Session, ToolRecord};
pub use sink::{EventSink, ProgressReporter, ThrottledSink};
pub use tools::{Registry, Tool, ToolCtx, ToolEnv, ToolError, ToolOutcome};
pub use turn::TurnOutcome;
