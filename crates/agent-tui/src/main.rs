//! 终端版 Coding Agent 入口。
//!
//! 参考 openai/codex 的 TUI 架构，但按本项目规模做了简化：
//! agent 在**同进程**跑（不拆 app-server），主循环用 `tokio::select!`
//! 多路复用终端事件与 agent 事件，渲染只做「历史区 + 输入区」两段。

mod app;
mod composer;
mod config;
mod markdown;
mod sink;
mod slash;
mod terminal;
mod view;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

/// 命令行参数。
///
/// 工作区默认取当前目录 —— 与 codex 一致：在哪启动就在哪干活。
/// `--config` 指向配置文件，缺省时按平台约定找（Windows 在 AppData，
/// Unix 在 `~/.config`），找不到就用内置默认值。
#[derive(Parser, Debug)]
#[command(name = "agent-tui", about = "终端版 Coding Agent")]
struct Cli {
    /// 工作区根目录。
    #[arg(long, default_value = ".")]
    workspace: PathBuf,

    /// 配置文件路径（TOML）。缺省按平台约定查找。
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    let workspace = std::fs::canonicalize(&cli.workspace).unwrap_or_else(|_| cli.workspace.clone());
    tracing::info!(workspace = %workspace.display(), "启动 agent-tui");

    let config = config::load(cli.config.as_deref())?;
    let mut tui = terminal::Terminal::init()?;

    let mut app = app::App::new(config, workspace)?;
    let result = app.run(&mut tui).await;
    terminal::Terminal::restore()?;
    result.map(|_| ())
}

/// 初始化日志。日志写文件而不是 stderr —— stderr 与 stdout 是同一个
/// 终端，打进 alternate screen 的日志行不会被 ratatui 的差分重绘清掉，
/// 画面会越用越乱。codex 的对应做法是 `~/.codex/log/codex-tui.log`，
/// 这里写 `~/.coding-agents/log/agent-tui.log`（与配置目录同根）。
fn init_tracing() {
    use std::fs::OpenOptions;
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_env("CODING_AGENT_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,agent_tui=debug,agent_core=debug"));

    let log_dir = log_dir_path();
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("创建日志目录失败（{e}），日志回退到 stderr —— TUI 画面可能被日志污染");
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_writer(std::io::stderr)
            .init();
        return;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path())
        .expect("日志文件应可创建");
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(false)
        .with_writer(file)
        .init();
}

/// 日志目录：`~/.coding-agents/log`。
///
/// 与配置目录同根（`.coding-agents`），日志和配置放一处，
/// 用户 `ls ~/.coding-agents` 就能找到全部状态。
fn log_dir_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".coding-agents")
        .join("log")
}

/// 日志文件路径：`~/.coding-agents/log/agent-tui.log`。
fn log_file_path() -> PathBuf {
    log_dir_path().join("agent-tui.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 日志必须落在用户目录下的 `.coding-agents/log` —— 与配置同根，
    /// 用户一处就能找到。改路径时这里会拦住。
    #[test]
    fn log_file_lives_under_coding_agents_dir() {
        let home = dirs::home_dir().expect("home_dir 应可用");
        assert_eq!(
            log_file_path(),
            home.join(".coding-agents")
                .join("log")
                .join("agent-tui.log")
        );
    }

    /// fmt + `with_writer(File)` 确实把日志写进文件而不是 stderr。
    ///
    /// 守着问题 4 的机制本体：日志不进终端，TUI 画面才不会被
    /// 差分重绘清不掉的垃圾行慢慢弄乱。用 `with_default` 局部挂载，
    /// 不碰全局 subscriber，也不写用户真实日志目录。
    #[test]
    fn fmt_writer_file_really_writes_to_file() {
        use std::fs::OpenOptions;
        use tracing_subscriber::EnvFilter;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smoke.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .with_ansi(false)
            .with_writer(file)
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("smoke test log line");
        });

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("smoke test log line"),
            "日志应写入文件：{content:?}"
        );
    }
}
