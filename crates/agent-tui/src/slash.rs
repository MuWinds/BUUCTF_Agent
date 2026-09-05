//! 斜杠命令定义、补全匹配与本地执行调度。
//!
//! 在用户回车发送前拦截 `/` 开头的输入，
//! 本地处理 `/help`、`/clear`、`/compact`、`/model`、`/diff`、
//! `/sessions`、`/new`、`/resume`、`/detail`、`/exit`、`/quit` 等指令。

use std::path::Path;
use std::sync::Arc;

use agent_core::{LlmClient, LlmConfig, Session};

use crate::view::{apply_compaction, rebuild_entries, UiEntry};

/// 斜杠命令元数据定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandDef {
    pub name: &'static str,
    pub args: &'static str,
    pub description: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommandDef] = &[
    SlashCommandDef {
        name: "/help",
        args: "",
        description: "显示所有可用斜杠命令与快捷键说明",
    },
    SlashCommandDef {
        name: "/clear",
        args: "",
        description: "清空当前会话，开启全新上下文",
    },
    SlashCommandDef {
        name: "/compact",
        args: "",
        description: "手动压缩长对话历史，折叠早期消息",
    },
    SlashCommandDef {
        name: "/model",
        args: "[名称]",
        description: "查看或切换当前使用的 AI 模型",
    },
    SlashCommandDef {
        name: "/diff",
        args: "",
        description: "查看当前工作区未提交的代码变更 (git diff)",
    },
    SlashCommandDef {
        name: "/sessions",
        args: "",
        description: "列出当前工作区的所有历史会话",
    },
    SlashCommandDef {
        name: "/new",
        args: "",
        description: "保存当前会话并创建全新空白会话",
    },
    SlashCommandDef {
        name: "/resume",
        args: "<ID>",
        description: "切换并恢复指定的历史会话",
    },
    SlashCommandDef {
        name: "/detail",
        args: "",
        description: "切换工具输出详细展开/精简模式",
    },
    SlashCommandDef {
        name: "/multiline",
        args: "",
        description: "切换多行输入模式 (Enter 换行，Ctrl+D / Ctrl+S 发送)",
    },
    SlashCommandDef {
        name: "/editor",
        args: "",
        description: "在外部文本编辑器 (Notepad/nano) 中编辑提示词",
    },
    SlashCommandDef {
        name: "/exit",
        args: "",
        description: "退出终端应用 (也可输入 /quit)",
    },
    SlashCommandDef {
        name: "/quit",
        args: "",
        description: "退出终端应用 (也可输入 /exit)",
    },
];

/// 返回当前匹配的斜杠命令补全列表。
///
/// 仅当输入以 `/` 开头且光标位于命令名称部分（未输入参数）时返回候选列表。
pub fn slash_completions(input: &str, cursor: usize) -> Vec<&'static SlashCommandDef> {
    let trimmed = input.trim_start();
    if !trimmed.starts_with('/') {
        return Vec::new();
    }
    // 如果输入包含空格，且光标在空格之后，说明已经在输入参数了，不再弹出补全
    if let Some(space_idx) = trimmed.find(' ') {
        let space_pos_in_input = input.len() - trimmed.len() + space_idx;
        if cursor > space_pos_in_input {
            return Vec::new();
        }
    }
    let first_word = trimmed.split_whitespace().next().unwrap_or(trimmed);
    SLASH_COMMANDS
        .iter()
        .filter(|cmd| cmd.name.starts_with(first_word))
        .collect()
}

/// 帮助信息文本。
pub fn help_message() -> &'static str {
    "\
可用斜杠命令与快捷键：
  /help            - 显示此帮助信息
  /clear           - 清空当前对话历史，开启新会话
  /compact         - 手动触发当前对话上下文压缩
  /model [名称]    - 查看或切换当前模型
  /diff            - 查看工作区当前未提交的代码变更 (git diff)
  /sessions        - 查看当前工作区的所有历史会话
  /new             - 保存当前会话并开启全新会话
  /resume <id>     - 切换并恢复指定历史会话
  /detail          - 切换工具输出详细展开/折叠
  /multiline       - 切换多行输入模式 (快捷键 Ctrl+T)
  /editor          - 在外部文本编辑器中编辑提示词 (快捷键 Ctrl+G)
  /exit            - 退出终端应用 (也可输入 /quit)
  /quit            - 退出终端应用 (也可输入 /exit)

快捷键：
  Ctrl+T           - 切换多行模式 (开启后 Enter 直接换行，Ctrl+D 或 Ctrl+S 发送)
  Ctrl+G           - 在外部编辑器 (Notepad/nano) 中编写大段多行文本并回填
  Shift+Enter / Ctrl+Enter / Ctrl+J - 单行模式下插入换行 (也支持行尾 \\ 加 Enter 续行)
  Up / Down        - 多行输入时光标上下移动；在首行/末行时翻阅历史输入
  Ctrl+A / Ctrl+E  - 快速跳至输入首/尾
  Ctrl+U / Ctrl+K  - 清空光标前/后文本
  Ctrl+W           - 删除光标前单词
  Ctrl+O           - 快速切换工具详细输出展开/折叠
  PageUp / PageDown - 滚动历史对话
  鼠标左键拖选     - 原生划选终端文本、双击选词，右键或快捷键直接复制 (对齐 Codex / Claude Code)
  Ctrl+C           - 忙碌时中断当前轮次，空闲时退出
  Esc              - 中断当前轮次或清空输入"
}

/// 斜杠命令执行上下文。
pub struct SlashContext<'a> {
    pub busy: bool,
    pub config: &'a mut LlmConfig,
    pub client: &'a Arc<LlmClient>,
    pub workspace: &'a Path,
    pub app_data: &'a Path,
    pub session_id: &'a mut String,
    pub session: &'a mut Option<Session>,
    pub ui_entries: &'a mut Vec<UiEntry>,
    pub show_tool_details: &'a mut bool,
    pub multiline_mode: &'a mut bool,
    pub should_exit: &'a mut bool,
}

/// 执行斜杠命令。
pub async fn execute_slash_command(text: &str, ctx: &mut SlashContext<'_>) {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");
    let arg = parts.get(1).copied();

    match cmd {
        "/help" => {
            ctx.ui_entries.push(UiEntry::Notice {
                text: help_message().to_string(),
            });
        }
        "/clear" => {
            if ctx.busy {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: "当前有任务正在运行，请先按 Ctrl+C 中断后再清空。".into(),
                });
                return;
            }
            persist_session(
                ctx.app_data,
                ctx.workspace,
                &ctx.config.model,
                ctx.session_id,
                ctx.session.as_ref(),
            )
            .await;
            *ctx.session_id = agent_host::persist::new_id();
            *ctx.session = Some(Session::default());
            ctx.ui_entries.clear();
            ctx.ui_entries.push(UiEntry::Notice {
                text: "已清空对话上下文，开启全新会话。".into(),
            });
        }
        "/compact" => {
            if ctx.busy {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: "当前有任务正在运行，无法执行压缩。".into(),
                });
                return;
            }
            let mut session = ctx.session.take().unwrap_or_default();
            let client = ctx.client.clone();
            let config = ctx.config.clone();
            match agent_core::compact::maybe_compact(&client, &config, &mut session).await {
                Ok(Some(c)) => {
                    apply_compaction(ctx.ui_entries, c.removed_entries, &c.summary);
                    ctx.ui_entries.push(UiEntry::Notice {
                        text: format!("手动压缩成功：已折叠 {} 条旧消息。", c.removed_entries),
                    });
                }
                Ok(None) => {
                    ctx.ui_entries.push(UiEntry::Notice {
                        text: "当前上下文未超过压缩阈值，无需压缩。".into(),
                    });
                }
                Err(e) => {
                    ctx.ui_entries.push(UiEntry::Notice {
                        text: format!("上下文压缩失败：{e}"),
                    });
                }
            }
            *ctx.session = Some(session);
            persist_session(
                ctx.app_data,
                ctx.workspace,
                &ctx.config.model,
                ctx.session_id,
                ctx.session.as_ref(),
            )
            .await;
        }
        "/model" => {
            if let Some(new_model) = arg {
                ctx.config.model = new_model.to_string();
                ctx.ui_entries.push(UiEntry::Notice {
                    text: format!("已将模型切换为：{new_model}"),
                });
            } else {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: format!("当前模型：{}", ctx.config.model),
                });
            }
        }
        "/diff" => {
            match std::process::Command::new("git")
                .args(["diff"])
                .current_dir(ctx.workspace)
                .output()
            {
                Ok(output) => {
                    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if out.is_empty() {
                        ctx.ui_entries.push(UiEntry::Notice {
                            text: "当前工作区干净，无未提交的代码差异。".into(),
                        });
                    } else {
                        ctx.ui_entries.push(UiEntry::Notice {
                            text: format!("工作区 git diff:\n{out}"),
                        });
                    }
                }
                Err(e) => {
                    ctx.ui_entries.push(UiEntry::Notice {
                        text: format!("执行 git diff 失败：{e}"),
                    });
                }
            }
        }
        "/sessions" => {
            let list = agent_host::persist::list(ctx.app_data, ctx.workspace).await;
            if list.is_empty() {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: "当前工作区暂无历史会话。".into(),
                });
            } else {
                let mut msg = String::from("工作区历史会话列表：\n");
                for s in list {
                    let is_curr = if &s.id == ctx.session_id {
                        "▶ *"
                    } else {
                        "   "
                    };
                    let title = if s.title.is_empty() {
                        "（新会话）"
                    } else {
                        &s.title
                    };
                    msg.push_str(&format!(
                        "{is_curr} [{}] {} (消息: {}, 模型: {})\n",
                        s.id, title, s.message_count, s.model
                    ));
                }
                msg.push_str("\n提示：输入 /resume <会话ID> 切换会话");
                ctx.ui_entries.push(UiEntry::Notice { text: msg });
            }
        }
        "/new" => {
            if ctx.busy {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: "当前有任务正在运行，请先中断后再新建会话。".into(),
                });
                return;
            }
            persist_session(
                ctx.app_data,
                ctx.workspace,
                &ctx.config.model,
                ctx.session_id,
                ctx.session.as_ref(),
            )
            .await;
            *ctx.session_id = agent_host::persist::new_id();
            *ctx.session = Some(Session::default());
            ctx.ui_entries.clear();
            ctx.ui_entries.push(UiEntry::Notice {
                text: format!("已创建并切换到新会话 [{}]", ctx.session_id),
            });
        }
        "/resume" => {
            let Some(target_id) = arg else {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: "用法：/resume <会话ID>（输入 /sessions 可查看列表）".into(),
                });
                return;
            };
            if target_id == ctx.session_id {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: format!("当前已在会话 [{target_id}] 中。"),
                });
                return;
            }
            if ctx.busy {
                ctx.ui_entries.push(UiEntry::Notice {
                    text: "当前有任务正在运行，无法切换会话。".into(),
                });
                return;
            }
            persist_session(
                ctx.app_data,
                ctx.workspace,
                &ctx.config.model,
                ctx.session_id,
                ctx.session.as_ref(),
            )
            .await;
            match agent_host::persist::load(ctx.app_data, ctx.workspace, target_id).await {
                Some(loaded_session) => {
                    *ctx.session_id = target_id.to_string();
                    *ctx.session = Some(loaded_session);
                    rebuild_entries(ctx.session.as_ref(), ctx.ui_entries);
                    ctx.ui_entries.push(UiEntry::Notice {
                        text: format!("已成功恢复会话 [{target_id}]。"),
                    });
                }
                None => {
                    ctx.ui_entries.push(UiEntry::Notice {
                        text: format!("未找到 ID 为 [{target_id}] 的历史会话。"),
                    });
                }
            }
        }
        "/detail" => {
            *ctx.show_tool_details = !*ctx.show_tool_details;
            let state_str = if *ctx.show_tool_details {
                "展开"
            } else {
                "折叠"
            };
            ctx.ui_entries.push(UiEntry::Notice {
                text: format!("工具详细输出与思考过程已切换为：{state_str}。"),
            });
        }
        "/multiline" => {
            *ctx.multiline_mode = !*ctx.multiline_mode;
            let status = if *ctx.multiline_mode {
                "已开启多行输入模式：按 Enter 直接换行，按 Ctrl+D 或 Ctrl+S 发送，按 Ctrl+T 随时退出。"
            } else {
                "已退出多行输入模式，恢复单行输入模式 (Enter 发送)。"
            };
            ctx.ui_entries.push(UiEntry::Notice {
                text: status.into(),
            });
        }
        "/exit" | "/quit" => {
            *ctx.should_exit = true;
        }
        _ => {
            ctx.ui_entries.push(UiEntry::Notice {
                text: format!("未知命令：{cmd}。输入 /help 查看可用命令列表。"),
            });
        }
    }
}

/// 保存会话的本地辅助函数。
async fn persist_session(
    app_data: &Path,
    workspace: &Path,
    model: &str,
    session_id: &str,
    session: Option<&Session>,
) {
    if let Some(s) = session {
        if let Err(e) = agent_host::persist::save(app_data, workspace, model, session_id, s).await {
            tracing::warn!("保存会话失败：{e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 斜杠命令候选匹配与过滤。
    #[test]
    fn slash_completions_filtering() {
        assert!(slash_completions("", 0).is_empty());
        assert!(slash_completions("hello", 3).is_empty());

        // 输入 "/" 匹配全部斜杠命令
        let all = slash_completions("/", 1);
        assert_eq!(all.len(), SLASH_COMMANDS.len());

        // 输入 "/c" 匹配 /clear 和 /compact
        let matches = slash_completions("/c", 2);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].name, "/clear");
        assert_eq!(matches[1].name, "/compact");

        // 带空格后光标在后，补全应自动收起
        assert!(slash_completions("/clear ", 7).is_empty());
    }

    /// 斜杠命令执行测试：/help、/model、/exit 等。
    #[tokio::test]
    async fn slash_commands_execution() {
        let mut config = LlmConfig::default();
        let client = Arc::new(LlmClient::new().unwrap());
        let workspace = Path::new(".");
        let app_data = Path::new(".");
        let mut session_id = "test-session".to_string();
        let mut session = Some(Session::default());
        let mut ui_entries = Vec::new();
        let mut show_tool_details = false;
        let mut should_exit = false;
        let mut multiline_mode = false;

        let mut ctx = SlashContext {
            busy: false,
            config: &mut config,
            client: &client,
            workspace,
            app_data,
            session_id: &mut session_id,
            session: &mut session,
            ui_entries: &mut ui_entries,
            show_tool_details: &mut show_tool_details,
            should_exit: &mut should_exit,
            multiline_mode: &mut multiline_mode,
        };

        // 执行 /help
        execute_slash_command("/help", &mut ctx).await;
        assert!(
            matches!(ctx.ui_entries.last(), Some(UiEntry::Notice { text }) if text.contains("/help"))
        );

        // 执行 /multiline
        execute_slash_command("/multiline", &mut ctx).await;
        assert!(*ctx.multiline_mode);
        execute_slash_command("/multiline", &mut ctx).await;
        assert!(!*ctx.multiline_mode);

        // 执行 /model
        execute_slash_command("/model custom-model", &mut ctx).await;
        assert_eq!(ctx.config.model, "custom-model");

        // 执行 /exit
        execute_slash_command("/exit", &mut ctx).await;
        assert!(*ctx.should_exit);
    }
}
