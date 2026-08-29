//! 端到端测试。
//!
//! 覆盖的是「真协议 + 真工具 + 真循环」这条完整链路：
//! 请求打到 `scripts/fake-llm` 的假服务端，工具在沙箱副本上真实读写文件，
//! 轮次循环原样跑，断言落在推给 UI 的 [`AgentEvent`] 序列上。
//!
//! 之所以断言事件而不是断言返回值：事件流才是前端唯一能看到的东西。
//! 返回值对了但事件序列错了，界面照样是坏的。
//!
//! 场景数据在 `scripts/fake-llm/fixtures/`，工作区数据在 `scripts/fake-llm/sandbox/`。
//! 两者被自动化测试和手动 GUI 测试共用 —— 数据只有一份，才不会出现
//! 「测试通过但手动点出来是另一回事」。

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_core::{
    AgentEvent, EventSink, LlmClient, LlmConfig, Session, ThrottledSink, ToolEnv, ToolResultBody,
};
use coding_agent_lib::tools::{registry, ReadRegistry};
use command_group::{CommandGroup, GroupChild};
use tokio_util::sync::CancellationToken;

const SERVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../scripts/fake-llm/server.mjs"
);
const SANDBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../scripts/fake-llm/sandbox");

// ---------- 假服务端 ----------

/// 一个独占的 fake-llm 进程。
///
/// 每个用例起一个而不是全局共享：`--port 0` 让内核分配端口，用例之间
/// 天然隔离，也就不必关心执行顺序和并行度。代价是每次约 200ms 启动开销。
struct FakeLlm {
    child: GroupChild,
    base_url: String,
}

impl FakeLlm {
    fn start() -> Self {
        let mut child = Command::new("node")
            .arg(SERVER)
            .args(["--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // group_spawn 而非 spawn：Windows 上 kill() 只杀 node 本身，
            // 纳入 Job Object 才能保证退出时不留孤儿进程。
            .group_spawn()
            .expect("启动 fake-llm 失败，请确认 node 在 PATH 上");

        let stdout = child.inner().stdout.take().expect("stdout 已被取走");
        let mut line = String::new();
        BufReader::new(stdout)
            .read_line(&mut line)
            .expect("读取 fake-llm 就绪行失败");

        let base_url = line
            .strip_prefix("FAKE_LLM_READY ")
            .unwrap_or_else(|| panic!("fake-llm 未按约定输出就绪行，实际收到：{line:?}"))
            .trim()
            .to_string();

        Self { child, base_url }
    }
}

impl Drop for FakeLlm {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------- 事件收集 ----------

/// 把事件收进 Vec 的 sink。core 只认 [`EventSink`] trait，测试因此不需要任何 GUI。
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<AgentEvent>>>);

impl EventSink for Recorder {
    fn emit(&self, event: AgentEvent) {
        self.0.lock().expect("Recorder 锁被毒化").push(event);
    }
}

impl Recorder {
    fn events(&self) -> Vec<AgentEvent> {
        self.0.lock().expect("Recorder 锁被毒化").clone()
    }
}

// ---------- 工作区 ----------

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("创建目录失败");
    for entry in std::fs::read_dir(from).expect("读取沙箱目录失败") {
        let entry = entry.expect("读取目录项失败");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("获取文件类型失败").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("复制文件失败");
        }
    }
}

/// 把沙箱复制到临时目录。工具会真的写文件，不能污染仓库里的那一份。
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("创建临时目录失败");
    copy_dir(Path::new(SANDBOX), dir.path());
    dir
}

// ---------- 驱动 ----------

struct Run {
    events: Vec<AgentEvent>,
    finish_reason: String,
    workspace: PathBuf,
    // 持有 TempDir 直到断言结束，否则目录会被提前删掉
    _dir: tempfile::TempDir,
}

/// 跑完整一个轮次。`scenario` 直接填进 model 字段 —— 假服务端据此选场景。
async fn run_scenario(scenario: &str, user_text: &str) -> Run {
    run_with_cancel(scenario, user_text, CancellationToken::new()).await
}

async fn run_with_cancel(scenario: &str, user_text: &str, cancel: CancellationToken) -> Run {
    let server = FakeLlm::start();
    let dir = workspace();
    let recorder = Recorder::default();

    let config = LlmConfig {
        base_url: server.base_url.clone(),
        api_key: String::new(),
        model: scenario.to_string(),
        temperature: None,
        context_limit: 128_000,
    };

    let mut session = Session::default();
    session.push_user(user_text);

    let read_registry = Arc::new(ReadRegistry::new());
    let tools = registry(read_registry);
    let env = ToolEnv {
        workspace_root: dir.path().to_path_buf(),
    };
    let mut sink = ThrottledSink::new(Arc::new(recorder.clone()), "turn-e2e");

    let outcome = agent_core::turn::run(
        &LlmClient::new().expect("创建 HTTP 客户端失败"),
        &config,
        &mut session,
        &tools,
        &env,
        &mut sink,
        cancel,
    )
    .await;

    Run {
        events: recorder.events(),
        finish_reason: outcome.finish_reason,
        workspace: dir.path().to_path_buf(),
        _dir: dir,
    }
}

// ---------- 断言辅助 ----------

impl Run {
    /// 拼接全部正文增量。逐帧断言没有意义 —— 帧边界由 33ms 节流决定，不稳定。
    fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::AssistantDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn reasoning(&self) -> String {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ReasoningDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// 工具调用就绪事件里的 (名称, 参数)。
    fn ready_calls(&self) -> Vec<(String, serde_json::Value)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolCallReady { name, args, .. } => Some((name.clone(), args.clone())),
                _ => None,
            })
            .collect()
    }

    fn results(&self) -> Vec<(bool, ToolResultBody)> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::ToolResult { ok, result, .. } => Some((*ok, result.clone())),
                _ => None,
            })
            .collect()
    }

    fn errors(&self) -> Vec<String> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Error { message, .. } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    fn usage(&self) -> Option<(u32, u32)> {
        self.events.iter().find_map(|e| match e {
            AgentEvent::Usage {
                total_tokens,
                context_used,
                ..
            } => Some((*total_tokens, *context_used)),
            _ => None,
        })
    }

    /// 事件类型名序列，用于断言整体形状。
    fn shape(&self) -> Vec<&'static str> {
        self.events
            .iter()
            .map(|e| match e {
                AgentEvent::TurnStart { .. } => "turn_start",
                AgentEvent::AssistantDelta { .. } => "assistant_delta",
                AgentEvent::ReasoningDelta { .. } => "reasoning_delta",
                AgentEvent::ToolCallStart { .. } => "tool_call_start",
                AgentEvent::ToolCallReady { .. } => "tool_call_ready",
                AgentEvent::ToolProgress { .. } => "tool_progress",
                AgentEvent::ToolResult { .. } => "tool_result",
                AgentEvent::Usage { .. } => "usage",
                AgentEvent::TurnEnd { .. } => "turn_end",
                AgentEvent::Error { .. } => "error",
            })
            .collect()
    }
}

// ---------- 用例 ----------

/// 最基本的一条：轮次必须以 turn_start 开头、turn_end 收尾，中间有正文。
#[tokio::test]
async fn plain_answer_streams_and_closes_the_turn() {
    let run = run_scenario("basic-chat", "介绍一下这个项目").await;

    assert_eq!(run.finish_reason, "stop");
    assert_eq!(run.shape().first(), Some(&"turn_start"));
    assert_eq!(run.shape().last(), Some(&"turn_end"));
    assert!(
        run.text().contains("通用 Coding Agent"),
        "正文：{}",
        run.text()
    );
    // 代码块必须原样穿过，前端 RichText 依赖它做流式渲染
    assert!(run.text().contains("```rust"));
    assert!(run.errors().is_empty());
}

/// 思维链走独立事件，不能混进正文 —— 前端要把它折叠灰显。
#[tokio::test]
async fn reasoning_is_reported_separately_from_the_answer() {
    let run = run_scenario("reasoning", "想一想").await;

    assert!(
        run.reasoning().contains("跨进程序列化"),
        "思维链：{}",
        run.reasoning()
    );
    assert!(run.text().contains("产生端"));
    assert!(
        !run.text().contains("跨进程序列化"),
        "思维链不该出现在正文里"
    );
}

/// 服务端返回 usage 时必须转成事件，且 context_used 取最后一次请求的 prompt。
#[tokio::test]
async fn usage_is_accumulated_across_tool_rounds() {
    let run = run_scenario("tool-read", "读一下 README").await;

    let (total, context_used) = run.usage().expect("应当有 usage 事件");
    // 两轮请求：848 + 1222
    assert_eq!(total, 2070, "总量应当累加而非只报最后一次");
    // 上下文占用只能是最后一次的 prompt，累加会算出荒谬的占比
    assert_eq!(context_used, 1180);
}

/// 单个工具调用的完整链路：start → ready → result，且真的读到了文件内容。
#[tokio::test]
async fn single_tool_call_runs_against_the_real_filesystem() {
    let run = run_scenario("tool-read", "读一下 README").await;

    let calls = run.ready_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "Read");
    assert_eq!(calls[0].1["path"], "README.md");

    let results = run.results();
    assert_eq!(results.len(), 1);
    assert!(results[0].0, "Read 应当成功");
    match &results[0].1 {
        ToolResultBody::Text { content, .. } => {
            assert!(content.contains("沙箱工作区"), "读到的内容：{content}");
        }
        other => panic!("Read 的结果应当是 Text，实际是 {other:?}"),
    }

    // 卡片必须先于结果出现，否则 UI 会先画结果再画卡片头
    let shape = run.shape();
    let start = shape.iter().position(|s| *s == "tool_call_start").unwrap();
    let result = shape.iter().position(|s| *s == "tool_result").unwrap();
    assert!(start < result);
}

/// 一轮里的多个调用要各自独立上报，不能被合并成一个。
#[tokio::test]
async fn parallel_tool_calls_are_reported_independently() {
    let run = run_scenario("tool-parallel", "找一下 TODO").await;

    let names: Vec<_> = run.ready_calls().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["Glob", "Grep"]);
    assert_eq!(run.results().len(), 2);
    assert!(run.results().iter().all(|(ok, _)| *ok));
}

/// Bash 的增量输出要在命令结束前就推出去。
#[tokio::test]
async fn bash_streams_progress_before_the_command_finishes() {
    let run = run_scenario("tool-bash", "跑一下").await;

    let results = run.results();
    assert_eq!(results.len(), 1);
    match &results[0].1 {
        ToolResultBody::Exec {
            exit_code, output, ..
        } => {
            assert_eq!(*exit_code, Some(0));
            assert!(output.contains("第一行"), "输出：{output}");
        }
        other => panic!("Bash 的结果应当是 Exec，实际是 {other:?}"),
    }

    let shape = run.shape();
    let progress = shape.iter().position(|s| *s == "tool_progress");
    let result = shape.iter().position(|s| *s == "tool_result").unwrap();
    assert!(
        progress.is_some_and(|p| p < result),
        "进度事件必须早于结果事件，实际形状：{shape:?}"
    );
}

/// Edit 要产出结构化 diff（而不是纯文本），并真的改到磁盘上的文件。
#[tokio::test]
async fn edit_produces_a_structured_diff_and_writes_to_disk() {
    let run = run_scenario("tool-edit", "改一下问候语").await;

    let results = run.results();
    assert_eq!(results.len(), 2, "先 Read 再 Edit");
    let (ok, diff) = &results[1];
    assert!(ok, "Edit 应当成功");

    match diff {
        ToolResultBody::Diff {
            path,
            hunks,
            added,
            removed,
        } => {
            assert!(path.contains("app.ts"));
            assert_eq!((*added, *removed), (1, 1));
            assert!(!hunks.is_empty(), "必须有 hunk，UI 靠它上色");
        }
        other => panic!("Edit 的结果应当是 Diff，实际是 {other:?}"),
    }

    let content =
        std::fs::read_to_string(run.workspace.join("src/app.ts")).expect("读取被改文件失败");
    assert!(content.contains("你好，世界"), "文件实际内容：{content}");
    assert!(!content.contains("hello world"));
}

/// 工具失败不能终止轮次 —— 结果要回灌给模型，让它自己纠正。
#[tokio::test]
async fn a_failing_tool_lets_the_model_correct_itself() {
    let run = run_scenario("tool-failure", "读一个不存在的文件").await;

    let results = run.results();
    assert_eq!(results.len(), 2, "失败一次，纠正后再成功一次");
    assert!(!results[0].0, "第一次 Read 应当失败");
    assert!(results[1].0, "纠正后应当成功");

    // 失败是工具级的，不该升级成整轮 Error
    assert!(run.errors().is_empty(), "错误事件：{:?}", run.errors());
    assert_eq!(run.finish_reason, "stop");
}

/// 缺 index、逐字符分片、夹杂非法帧，参数仍要被正确还原。
#[tokio::test]
async fn malformed_stream_fragments_are_tolerated() {
    let run = run_scenario("malformed-toolcall", "畸形分片").await;

    let calls = run.ready_calls();
    assert_eq!(calls.len(), 1, "逐字符分片不能被拆成多个调用");
    assert_eq!(calls[0].1["path"], "README.md", "参数应当被完整拼回");
    assert!(run.errors().is_empty(), "非法帧应当被跳过而非终止整条流");
    assert_eq!(run.finish_reason, "stop");
}

/// HTTP 错误要变成 Error 事件（而非 panic 或静默），且带上服务端给的原因。
#[tokio::test]
async fn http_errors_surface_as_error_events() {
    for (scenario, needle) in [
        ("error-401", "Incorrect API key"),
        ("error-429", "Rate limit"),
        ("error-500", "server had an error"),
    ] {
        let run = run_scenario(scenario, "触发错误").await;
        let errors = run.errors();
        assert_eq!(errors.len(), 1, "{scenario} 应当恰好一个错误事件");
        assert!(
            errors[0].contains(needle),
            "{scenario} 的消息里应当保留服务端原因，实际：{}",
            errors[0]
        );
        // 错误也要正常收尾，否则前端会永远停在「思考中」
        assert_eq!(run.shape().last(), Some(&"turn_end"));
    }
}

/// 一个内容帧都没有时，轮次仍要干净收尾。
#[tokio::test]
async fn an_empty_stream_still_closes_the_turn() {
    let run = run_scenario("empty-stream", "空流").await;

    assert_eq!(run.finish_reason, "stop");
    assert!(run.text().is_empty());
    assert_eq!(run.shape().last(), Some(&"turn_end"));
}

/// 取消后必须尽快停下，且已经吐出的内容要保留。
#[tokio::test]
async fn cancelling_mid_stream_keeps_what_was_already_said() {
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        trigger.cancel();
    });

    let run = run_with_cancel("cancellable", "慢慢说", cancel).await;

    assert_eq!(run.finish_reason, "cancelled");
    assert!(!run.text().is_empty(), "取消前已到达的内容不该被丢弃");
    // fixture 有 60 句，1.5 秒按 400ms/句只可能吐出个位数
    assert!(run.text().contains("第 1 句"));
    assert!(!run.text().contains("第 60 句"), "取消后不该继续接收");
    assert_eq!(run.shape().last(), Some(&"turn_end"));
}
