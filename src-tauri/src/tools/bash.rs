//! Bash 工具：在工作区里执行 shell 命令。
//!
//! Windows 上有三个必须处理的坑：
//!
//! 1. **进程树回收** —— `child.kill()` 只杀 shell 本身，它派生的子进程会活下来。
//!    `npm run dev` 被取消后 node 仍占着端口，就是这么来的。用 `command-group`
//!    把整棵树放进 Job Object，关掉 Job 即可整树回收。
//! 2. **输出编码** —— 中文系统上很多命令输出 GBK，直接按 UTF-8 解码会得到乱码。
//! 3. **shell 选择** —— 没有 `/bin/sh`，得找 PowerShell 或 Git Bash。

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agent_core::{Tool, ToolCtx, ToolError, ToolOutcome, ToolResultBody};
use async_trait::async_trait;
use command_group::AsyncCommandGroup;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command;

/// 默认超时。
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// 允许的最大超时。
const MAX_TIMEOUT: Duration = Duration::from_secs(600);

/// 回灌给模型的输出上限。命令可能吐几十 MB，全塞进上下文毫无意义。
const MAX_OUTPUT_CHARS: usize = 30_000;

pub struct BashTool;

#[derive(Deserialize)]
struct Args {
    command: String,
    /// 超时秒数。
    #[serde(default)]
    timeout: Option<u64>,
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "Bash"
    }

    fn description(&self) -> &'static str {
        "在工作区目录下执行 shell 命令，返回合并后的 stdout 与 stderr。\
         命令在工作区根目录执行，需要切换目录请在命令里自行 cd。\
         默认超时 120 秒，可用 timeout 参数调整（最大 600 秒）。\
         注意：查找文件请用 Glob、搜索内容请用 Grep、读文件请用 Read —— \
         它们比 shell 命令更快且输出更适合阅读。"
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "要执行的命令"
                },
                "timeout": {
                    "type": "integer",
                    "description": "超时秒数，默认 120，最大 600",
                    "minimum": 1
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn preview(&self, args: &Value) -> String {
        let command = args.get("command").and_then(Value::as_str).unwrap_or("?");
        let first_line = command.lines().next().unwrap_or(command);
        let clipped: String = first_line.chars().take(60).collect();
        let suffix = if first_line.chars().count() > 60 {
            "…"
        } else {
            ""
        };
        format!("Bash({clipped}{suffix})")
    }

    async fn execute(&self, args: Value, ctx: &ToolCtx) -> Result<ToolOutcome, ToolError> {
        let args: Args = serde_json::from_value(args)
            .map_err(|e| ToolError::recoverable(format!("参数不正确：{e}")))?;

        if args.command.trim().is_empty() {
            return Err(ToolError::recoverable("command 不能为空"));
        }

        let timeout = args
            .timeout
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_TIMEOUT)
            .min(MAX_TIMEOUT);

        let shell = Shell::detect().ok_or_else(|| {
            ToolError::fatal("找不到可用的 shell（尝试过 pwsh、powershell、bash）")
        })?;

        run(&args.command, shell, timeout, ctx).await
    }
}

/// 可用的 shell。
// PowerShell 是产品的正式名称，改名迁就 lint 只会让人对不上号
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shell {
    Pwsh,
    PowerShell,
    Bash,
}

impl Shell {
    /// 按优先级探测。
    ///
    /// Windows 上优先 PowerShell —— 它一定存在，且能处理绝大多数命令。
    /// Git Bash 虽然更贴近模型的习惯，但未必装了。
    fn detect() -> Option<Self> {
        #[cfg(windows)]
        let candidates = [
            (Self::Pwsh, "pwsh.exe"),
            (Self::PowerShell, "powershell.exe"),
            (Self::Bash, "bash.exe"),
        ];
        #[cfg(not(windows))]
        let candidates = [(Self::Bash, "bash")];

        candidates
            .into_iter()
            .find(|(_, exe)| which(exe))
            .map(|(shell, _)| shell)
    }

    fn program(self) -> &'static str {
        match self {
            Self::Pwsh => "pwsh",
            Self::PowerShell => "powershell",
            Self::Bash => "bash",
        }
    }

    /// 构造执行命令的参数。
    fn args(self, command: &str) -> Vec<String> {
        match self {
            Self::Pwsh | Self::PowerShell => vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                // 强制 UTF-8 输出，能消掉绝大部分中文乱码；
                // $ErrorActionPreference 让报错走 stderr 而不是直接中止
                format!(
                    "[Console]::OutputEncoding=[Text.Encoding]::UTF8; \
                     $ErrorActionPreference='Continue'; {command}"
                ),
            ],
            Self::Bash => vec!["-c".into(), command.to_string()],
        }
    }
}

/// 判断可执行文件是否在 PATH 上。
fn which(exe: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(exe).is_file())
}

async fn run(
    command: &str,
    shell: Shell,
    timeout: Duration,
    ctx: &ToolCtx,
) -> Result<ToolOutcome, ToolError> {
    let started = Instant::now();

    let mut cmd = Command::new(shell.program());
    cmd.args(shell.args(command))
        .current_dir(&ctx.workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // group_spawn 而非 spawn：把整棵进程树纳入一个 Job Object（Windows）
    // 或进程组（Unix），取消时才能连同孙进程一起回收
    let mut child = cmd
        .group_spawn()
        .map_err(|e| ToolError::recoverable(format!("无法启动 {}：{e}", shell.program())))?;

    let stdout = child.inner().stdout.take();
    let stderr = child.inner().stderr.take();

    // 输出写进共享缓冲而不是靠 future 的返回值：超时和取消时那些 future
    // 根本不会完成，返回值拿不到 —— 但用户已经看到的输出，模型也该看到。
    let out_buffer = Arc::new(Mutex::new(String::new()));
    let err_buffer = Arc::new(Mutex::new(String::new()));

    let wait = {
        let out_buffer = out_buffer.clone();
        let err_buffer = err_buffer.clone();
        async {
            // stdout 与 stderr 必须并发读。只读一个会在另一个的管道缓冲写满时
            // 死锁 —— 这是子进程执行里最经典的挂起原因。
            tokio::join!(
                read_stream(stdout, "stdout", ctx, out_buffer),
                read_stream(stderr, "stderr", ctx, err_buffer),
            );
            child.wait().await
        }
    };

    let outcome = tokio::select! {
        biased;
        _ = ctx.cancel.cancelled() => Finish::Cancelled,
        _ = tokio::time::sleep(timeout) => Finish::TimedOut,
        status = wait => Finish::Exited(status.ok().and_then(|s| s.code())),
    };

    ctx.progress.flush("stdout");

    // 超时或取消：关掉整个 Job / 进程组，孙进程一并回收
    if !matches!(outcome, Finish::Exited(_)) {
        if let Err(e) = child.kill().await {
            tracing::warn!("终止进程树失败: {e}");
        }
        let _ = child.wait().await;
    }

    let mut collector = Output::new();
    collector.absorb(take(&out_buffer), take(&err_buffer));

    Ok(render(command, collector, outcome, started.elapsed()))
}

fn take(buffer: &Arc<Mutex<String>>) -> String {
    std::mem::take(&mut *buffer.lock().expect("输出缓冲锁被毒化"))
}

enum Finish {
    Exited(Option<i32>),
    TimedOut,
    Cancelled,
}

/// 读取一个输出流，边读边上报进度，同时累积到共享缓冲。
async fn read_stream<R>(
    stream: Option<R>,
    name: &'static str,
    ctx: &ToolCtx,
    buffer: Arc<Mutex<String>>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(stream) = stream else {
        return;
    };

    let mut reader = BufReader::new(stream);
    let mut chunk = [0u8; 8192];
    // 半个多字节字符可能跨越两次读取，留到下次拼接
    let mut carry: Vec<u8> = Vec::new();

    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };

        carry.extend_from_slice(&chunk[..read]);
        let (text, rest) = decode(&carry);
        carry = rest;

        if text.is_empty() {
            continue;
        }

        ctx.progress.push(name, &text);
        buffer.lock().expect("输出缓冲锁被毒化").push_str(&text);
    }

    if !carry.is_empty() {
        let (text, _) = decode_lossy(&carry);
        ctx.progress.push(name, &text);
        buffer.lock().expect("输出缓冲锁被毒化").push_str(&text);
    }
}

/// 解码一段输出，返回 (已解码文本, 需要留到下次的尾部字节)。
///
/// 优先按 UTF-8 解；失败则认为是本地代码页（中文 Windows 上通常是 GBK）。
fn decode(bytes: &[u8]) -> (String, Vec<u8>) {
    match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_string(), Vec::new()),
        Err(error) => {
            let valid = error.valid_up_to();
            // 尾部不完整的多字节序列留到下次
            if error.error_len().is_none() {
                let text = std::str::from_utf8(&bytes[..valid])
                    .unwrap_or_default()
                    .to_string();
                return (text, bytes[valid..].to_vec());
            }
            decode_lossy(bytes)
        }
    }
}

/// UTF-8 解不动就按 GBK 试，再不行才用替换字符。
fn decode_lossy(bytes: &[u8]) -> (String, Vec<u8>) {
    let (text, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if had_errors {
        (String::from_utf8_lossy(bytes).into_owned(), Vec::new())
    } else {
        (text.into_owned(), Vec::new())
    }
}

/// 累积的输出。
struct Output {
    text: String,
}

impl Output {
    fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    fn absorb(&mut self, stdout: String, stderr: String) {
        self.text.push_str(&stdout);
        if !stderr.is_empty() {
            if !self.text.is_empty() && !self.text.ends_with('\n') {
                self.text.push('\n');
            }
            self.text.push_str(&stderr);
        }
    }
}

fn render(command: &str, output: Output, finish: Finish, elapsed: Duration) -> ToolOutcome {
    let (text, truncated) = clip_tail(&output.text);

    let (exit_code, timed_out, killed) = match finish {
        Finish::Exited(code) => (code, false, false),
        Finish::TimedOut => (None, true, true),
        Finish::Cancelled => (None, false, true),
    };

    let mut llm_text = String::new();

    match finish {
        Finish::TimedOut => llm_text.push_str(&format!(
            "命令超时（{:.0} 秒后被终止）。\n",
            elapsed.as_secs_f64()
        )),
        Finish::Cancelled => llm_text.push_str("命令被用户中止。\n"),
        Finish::Exited(Some(0)) => {}
        Finish::Exited(Some(code)) => {
            llm_text.push_str(&format!("命令退出码 {code}（非零表示失败）。\n"))
        }
        Finish::Exited(None) => llm_text.push_str("命令被信号终止。\n"),
    }

    if text.trim().is_empty() {
        llm_text.push_str("（无输出）");
    } else {
        if truncated {
            llm_text.push_str("[输出过长，仅保留末尾部分]\n");
        }
        llm_text.push_str(&text);
    }

    ToolOutcome {
        llm_text,
        ui: ToolResultBody::Exec {
            command: command.to_string(),
            exit_code,
            output: text,
            truncated,
            timed_out,
            killed,
        },
    }
}

/// 超长输出保留**末尾** —— 命令的结论（错误信息、汇总）通常在最后。
fn clip_tail(text: &str) -> (String, bool) {
    let count = text.chars().count();
    if count <= MAX_OUTPUT_CHARS {
        return (text.to_string(), false);
    }
    let skip = count - MAX_OUTPUT_CHARS;
    (text.chars().skip(skip).collect(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_shell() {
        assert!(Shell::detect().is_some(), "本机应当至少有一个可用 shell");
    }

    #[test]
    fn powershell_forces_utf8_output() {
        let args = Shell::PowerShell.args("dir");
        let script = args.last().expect("应当有脚本参数");
        assert!(
            script.contains("OutputEncoding"),
            "没有强制 UTF-8：{script}"
        );
        assert!(script.ends_with("dir"));
    }

    #[test]
    fn bash_uses_dash_c() {
        assert_eq!(
            Shell::Bash.args("ls"),
            vec!["-c".to_string(), "ls".to_string()]
        );
    }

    #[test]
    fn keeps_short_output_intact() {
        let (text, truncated) = clip_tail("hello");
        assert_eq!(text, "hello");
        assert!(!truncated);
    }

    /// 截断保留末尾：命令的结论通常在最后几行。
    #[test]
    fn clips_from_the_head() {
        let long: String = (0..MAX_OUTPUT_CHARS + 500).map(|_| 'x').collect();
        let tail = format!("{long}END");
        let (text, truncated) = clip_tail(&tail);

        assert!(truncated);
        assert!(text.ends_with("END"), "应当保留末尾");
        assert_eq!(text.chars().count(), MAX_OUTPUT_CHARS);
    }

    /// 多字节字符不该在截断时被切坏。
    #[test]
    fn clips_on_character_boundaries() {
        let long: String = (0..MAX_OUTPUT_CHARS + 100).map(|_| '中').collect();
        let (text, _) = clip_tail(&long);
        assert_eq!(text.chars().count(), MAX_OUTPUT_CHARS);
        assert!(text.chars().all(|c| c == '中'));
    }

    #[test]
    fn decodes_utf8() {
        let (text, rest) = decode("中文 ok\n".as_bytes());
        assert_eq!(text, "中文 ok\n");
        assert!(rest.is_empty());
    }

    /// 尾部截断的多字节序列要留到下次，不能当成乱码。
    #[test]
    fn holds_back_incomplete_utf8_tail() {
        let full = "中文".as_bytes();
        let (text, rest) = decode(&full[..4]); // "中" 完整 + "文" 的第一个字节
        assert_eq!(text, "中");
        assert_eq!(rest.len(), 1);
    }

    /// 中文 Windows 上的 GBK 输出要能识别。
    #[test]
    fn falls_back_to_gbk() {
        let (gbk, _, _) = encoding_rs::GBK.encode("中文");
        let (text, _) = decode(&gbk);
        assert_eq!(text, "中文", "GBK 输出没有被正确解码");
    }

    #[test]
    fn reports_timeout_to_model() {
        let outcome = render(
            "sleep 100",
            Output {
                text: String::new(),
            },
            Finish::TimedOut,
            Duration::from_secs(5),
        );
        assert!(outcome.llm_text.contains("超时"), "{}", outcome.llm_text);

        match outcome.ui {
            ToolResultBody::Exec {
                timed_out, killed, ..
            } => {
                assert!(timed_out);
                assert!(killed);
            }
            _ => panic!("Bash 的 UI 结果应当是 Exec"),
        }
    }

    #[test]
    fn reports_non_zero_exit() {
        let outcome = render(
            "false",
            Output {
                text: "boom".into(),
            },
            Finish::Exited(Some(1)),
            Duration::from_millis(10),
        );
        assert!(
            outcome.llm_text.contains("退出码 1"),
            "{}",
            outcome.llm_text
        );
        assert!(outcome.llm_text.contains("boom"));
    }

    /// 成功且有输出时不该有多余的前缀说明。
    #[test]
    fn success_output_has_no_preamble() {
        let outcome = render(
            "echo hi",
            Output {
                text: "hi\n".into(),
            },
            Finish::Exited(Some(0)),
            Duration::from_millis(5),
        );
        assert_eq!(outcome.llm_text, "hi\n");
    }

    #[test]
    fn empty_output_is_explicit() {
        let outcome = render(
            "true",
            Output {
                text: String::new(),
            },
            Finish::Exited(Some(0)),
            Duration::from_millis(5),
        );
        assert!(outcome.llm_text.contains("无输出"));
    }

    #[test]
    fn merges_stderr_after_stdout() {
        let mut output = Output::new();
        output.absorb("out".into(), "err".into());
        assert_eq!(output.text, "out\nerr");
    }

    // ---------- 真实进程 ----------

    use agent_core::ProgressReporter;
    use tokio_util::sync::CancellationToken;

    fn ctx_with(cancel: CancellationToken) -> ToolCtx {
        ToolCtx {
            workspace_root: std::env::temp_dir(),
            cancel,
            progress: ProgressReporter::null(),
        }
    }

    async fn exec(command: &str, timeout_secs: u64) -> ToolOutcome {
        let ctx = ctx_with(CancellationToken::new());
        BashTool
            .execute(json!({ "command": command, "timeout": timeout_secs }), &ctx)
            .await
            .expect("命令执行不该返回工具错误")
    }

    /// 跨平台的「打印一行」命令。
    fn echo_cmd(text: &str) -> String {
        if cfg!(windows) {
            format!("Write-Output '{text}'")
        } else {
            format!("echo '{text}'")
        }
    }

    /// 跨平台的「睡 N 秒」命令。
    fn sleep_cmd(secs: u32) -> String {
        if cfg!(windows) {
            format!("Start-Sleep -Seconds {secs}")
        } else {
            format!("sleep {secs}")
        }
    }

    #[tokio::test]
    async fn captures_stdout() {
        let outcome = exec(&echo_cmd("hello-from-test"), 30).await;
        assert!(
            outcome.llm_text.contains("hello-from-test"),
            "没拿到输出：{}",
            outcome.llm_text
        );
    }

    /// 中文输出不能乱码 —— 中文 Windows 上这是最容易踩的坑。
    #[tokio::test]
    async fn handles_chinese_output() {
        let outcome = exec(&echo_cmd("中文输出测试"), 30).await;
        assert!(
            outcome.llm_text.contains("中文输出测试"),
            "中文乱码了：{}",
            outcome.llm_text
        );
    }

    /// 非零退出码要如实报告，否则模型会以为命令成功了。
    #[tokio::test]
    async fn reports_failure_exit_code() {
        // `exit 3` 在 PowerShell 和 bash 里语义一致，无需按平台分支
        let outcome = exec("exit 3", 30).await;

        match outcome.ui {
            ToolResultBody::Exec { exit_code, .. } => {
                assert_eq!(exit_code, Some(3), "退出码不对");
            }
            _ => panic!("应当是 Exec 结果"),
        }
        assert!(
            outcome.llm_text.contains("退出码 3"),
            "{}",
            outcome.llm_text
        );
    }

    /// 超时必须真的把命令掐掉，而不是一直挂着。
    #[tokio::test]
    async fn times_out_long_commands() {
        let started = Instant::now();
        let outcome = exec(&sleep_cmd(30), 2).await;
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(15),
            "超时没生效，耗时 {elapsed:?}"
        );

        match outcome.ui {
            ToolResultBody::Exec {
                timed_out, killed, ..
            } => {
                assert!(timed_out, "应当标记为超时");
                assert!(killed, "应当标记为被终止");
            }
            _ => panic!("应当是 Exec 结果"),
        }
    }

    /// 取消要立刻生效。
    #[tokio::test]
    async fn cancellation_stops_the_command() {
        let cancel = CancellationToken::new();
        let ctx = ctx_with(cancel.clone());

        let token = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            token.cancel();
        });

        let started = Instant::now();
        let outcome = BashTool
            .execute(json!({ "command": sleep_cmd(30), "timeout": 60 }), &ctx)
            .await
            .expect("取消不该返回工具错误");

        assert!(
            started.elapsed() < Duration::from_secs(15),
            "取消没有及时生效"
        );
        assert!(outcome.llm_text.contains("中止"), "{}", outcome.llm_text);
    }

    /// 取消时已经产生的输出必须保留 —— 用户看见了，模型也该看见。
    #[tokio::test]
    async fn keeps_output_produced_before_cancellation() {
        let cancel = CancellationToken::new();
        let ctx = ctx_with(cancel.clone());

        let token = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            token.cancel();
        });

        // 先打印一行，再长时间睡眠
        let command = format!("{}; {}", echo_cmd("marker-before-cancel"), sleep_cmd(30));
        let outcome = BashTool
            .execute(json!({ "command": command, "timeout": 60 }), &ctx)
            .await
            .expect("取消不该返回工具错误");

        assert!(
            outcome.llm_text.contains("marker-before-cancel"),
            "取消前的输出丢了：{}",
            outcome.llm_text
        );
    }

    /// 进程树回收：被取消的命令派生出的孙进程不能存活。
    ///
    /// Windows 上 `child.kill()` 只杀 shell 本身，孙进程会变成孤儿继续跑 ——
    /// 这正是 Job Object 要解决的问题。
    #[tokio::test]
    async fn kills_the_whole_process_tree() {
        if !cfg!(windows) {
            return;
        }

        let cancel = CancellationToken::new();
        let ctx = ctx_with(cancel.clone());

        // 派生一个后台子进程并打印它的 PID，然后父进程长睡
        let command = "$p = Start-Process -PassThru -WindowStyle Hidden powershell \
                       -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 120'; \
                       Write-Output \"CHILD_PID=$($p.Id)\"; Start-Sleep -Seconds 120";

        let token = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            token.cancel();
        });

        let outcome = BashTool
            .execute(json!({ "command": command, "timeout": 60 }), &ctx)
            .await
            .expect("执行不该返回工具错误");

        let Some(pid) = outcome
            .llm_text
            .lines()
            .find_map(|l| l.trim().strip_prefix("CHILD_PID="))
            .and_then(|p| p.trim().parse::<u32>().ok())
        else {
            // 拿不到 PID 说明环境不支持这个探测，不作为失败
            eprintln!("跳过：未能取得子进程 PID\n{}", outcome.llm_text);
            return;
        };

        // 给系统一点回收时间
        tokio::time::sleep(Duration::from_millis(800)).await;

        let alive = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false);

        assert!(!alive, "孙进程 {pid} 在取消后仍然存活，进程树没有被回收");
    }
}
