//! 宿主层：与 GUI 无关的共享实现。
//!
//! 桌面版（Tauri）与终端版（TUI）共用这一层：
//!
//! - **工具实现** —— Bash / Read / Write / Edit / Grep / Glob / Diff，
//!   权限边界与文件操作规则与宿主界面无关
//! - **会话存储** —— 按工作区归档的多会话落盘（persist）
//! - **上下文文件** —— 从工作区向上收集 AGENTS.md / CLAUDE.md（context_files）
//! - **密钥存储** —— 系统凭据管理器（secret）
//! - **系统提示词组装** —— 人格 + 工具清单 + 工作区约定
//!
//! 这里不依赖任何 GUI 框架，也不含 agent 轮次逻辑 —— 那属于 `agent-core`。

pub mod context_files;
pub mod persist;
pub mod secret;
pub mod tools;

use std::path::Path;

use agent_core::Registry;

/// 系统提示词。
///
/// 放在宿主层而非 core：agent 的人格与职责是产品决策，core 不该规定。
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
fn render_tools(registry: &Registry) -> String {
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
fn render_guidelines(registry: &Registry) -> String {
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
pub fn system_prompt(workspace_root: &Path, registry: &Registry) -> String {
    let tools = render_tools(registry);
    let guidelines = render_guidelines(registry);

    let mut prompt = SYSTEM_PROMPT
        .replace("{TOOLS}", &tools)
        .replace("{GUIDELINES}", &guidelines);

    let files = context_files::load(workspace_root);

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
