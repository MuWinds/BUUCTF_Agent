//! 事件出口与节流。
//!
//! 逐 token 推送到 UI 是性能陷阱：100 tok/s 就是 100 次/秒的跨进程序列化
//! 加 100 次前端状态更新，在 WebView 上会肉眼可见地掉帧。
//!
//! 这里把文本增量按帧（33ms ≈ 30fps）聚合。人眼分辨不出 33ms 内的差别，
//! 但推送频率从 O(tokens) 降到 ≤30/s。
//!
//! 实现用"惰性检查"而非独立的 interval task：每次 push 后判断是否该 flush，
//! 流结束时强制 flush。少一个 task、少一层 select，行为等价 —— 因为 token
//! 停止流入时本来也没有新内容需要显示。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::events::AgentEvent;

/// 事件出口。
///
/// core 不关心事件最终去了哪里 —— GUI 走 IPC channel，CLI 直接打印，
/// 测试收集到 Vec 里。实现方需自行保证线程安全。
pub trait EventSink: Send + Sync {
    fn emit(&self, event: AgentEvent);
}

/// 一帧的时长。
const FRAME: Duration = Duration::from_millis(33);

/// 文本缓冲上限。超过就立即 flush，避免大段文本被压在缓冲里显得卡顿。
const MAX_BUFFER: usize = 2048;

/// 带帧聚合的事件发送器。文本类增量走缓冲，其余事件立即发出。
pub struct ThrottledSink {
    inner: Arc<dyn EventSink>,
    turn_id: String,
    text: Buffer,
    reasoning: Buffer,
}

#[derive(Default)]
struct Buffer {
    pending: String,
    last_flush: Option<Instant>,
}

impl Buffer {
    /// 是否到了该发出去的时候。
    fn should_flush(&self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        if self.pending.len() >= MAX_BUFFER {
            return true;
        }
        self.last_flush.is_none_or(|t| t.elapsed() >= FRAME)
    }

    fn take(&mut self) -> String {
        self.last_flush = Some(Instant::now());
        std::mem::take(&mut self.pending)
    }
}

impl ThrottledSink {
    pub fn new(inner: Arc<dyn EventSink>, turn_id: impl Into<String>) -> Self {
        Self {
            inner,
            turn_id: turn_id.into(),
            text: Buffer::default(),
            reasoning: Buffer::default(),
        }
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    /// 拿到底层出口的共享句柄，用于派生工具进度上报器。
    pub fn raw(&self) -> Arc<dyn EventSink> {
        self.inner.clone()
    }

    /// 立即推送一个事件，不经过缓冲。
    pub fn emit(&self, event: AgentEvent) {
        self.inner.emit(event);
    }

    pub fn push_text(&mut self, delta: &str) {
        self.text.pending.push_str(delta);
        if self.text.should_flush() {
            self.flush_text();
        }
    }

    pub fn push_reasoning(&mut self, delta: &str) {
        self.reasoning.pending.push_str(delta);
        if self.reasoning.should_flush() {
            self.flush_reasoning();
        }
    }

    /// 把两个缓冲都清空。流结束或出错前必须调用，否则末尾的文本会丢。
    pub fn flush(&mut self) {
        self.flush_reasoning();
        self.flush_text();
    }

    fn flush_text(&mut self) {
        if self.text.pending.is_empty() {
            return;
        }
        let text = self.text.take();
        self.emit(AgentEvent::AssistantDelta {
            turn_id: self.turn_id.clone(),
            text,
        });
    }

    fn flush_reasoning(&mut self) {
        if self.reasoning.pending.is_empty() {
            return;
        }
        let text = self.reasoning.take();
        self.emit(AgentEvent::ReasoningDelta {
            turn_id: self.turn_id.clone(),
            text,
        });
    }
}

/// 单次工具调用的进度上报器。
///
/// 工具拿到的是不可变引用，所以节流状态放在 `Mutex` 里。
/// 竞争极低：一次调用最多两个读取任务（stdout / stderr）在推。
pub struct ProgressReporter {
    sink: Arc<dyn EventSink>,
    turn_id: String,
    call_id: String,
    state: Mutex<ProgressState>,
}

#[derive(Default)]
struct ProgressState {
    pending: String,
    last_flush: Option<Instant>,
}

/// 单次推送的输出上限。`npm install` 那种刷屏输出会瞬间打死前端。
const MAX_PROGRESS_CHUNK: usize = 8 * 1024;

impl ProgressReporter {
    pub fn new(
        sink: Arc<dyn EventSink>,
        turn_id: impl Into<String>,
        call_id: impl Into<String>,
    ) -> Self {
        Self {
            sink,
            turn_id: turn_id.into(),
            call_id: call_id.into(),
            state: Mutex::new(ProgressState::default()),
        }
    }

    /// 丢弃一切进度的上报器。
    ///
    /// 给测试，以及不关心中间输出的宿主（比如批处理场景）用。
    pub fn null() -> Self {
        Self::new(Arc::new(NullSink), "", "")
    }

    /// 推送一段输出。
    ///
    /// 按帧节流，且**只在行边界切分** —— 把半行发给 UI 会让终端视图
    /// 出现闪烁的残缺行。
    pub fn push(&self, stream: &'static str, chunk: &str) {
        let ready = {
            let mut state = self.state.lock().expect("进度锁被毒化");
            state.pending.push_str(chunk);

            let due = state.pending.len() >= MAX_PROGRESS_CHUNK
                || state.last_flush.is_none_or(|t| t.elapsed() >= FRAME);
            if !due {
                return;
            }

            // 切到最后一个换行处，剩下的半行留到下次
            match state.pending.rfind('\n') {
                Some(index) => {
                    let rest = state.pending.split_off(index + 1);
                    let ready = std::mem::replace(&mut state.pending, rest);
                    state.last_flush = Some(Instant::now());
                    ready
                }
                // 一整段都没有换行：超限就强制发出，否则继续攒
                None if state.pending.len() >= MAX_PROGRESS_CHUNK => {
                    state.last_flush = Some(Instant::now());
                    state.take_pending()
                }
                None => return,
            }
        };

        if !ready.is_empty() {
            self.emit(stream, ready);
        }
    }

    /// 把残留的半行发出去。命令结束时必须调用。
    pub fn flush(&self, stream: &'static str) {
        let ready = {
            let mut state = self.state.lock().expect("进度锁被毒化");
            state.take_pending()
        };
        if !ready.is_empty() {
            self.emit(stream, ready);
        }
    }

    fn emit(&self, stream: &'static str, chunk: String) {
        self.sink.emit(AgentEvent::ToolProgress {
            turn_id: self.turn_id.clone(),
            call_id: self.call_id.clone(),
            stream,
            chunk,
        });
    }
}

impl ProgressState {
    fn take_pending(&mut self) -> String {
        self.last_flush = Some(Instant::now());
        std::mem::take(&mut self.pending)
    }
}

/// 丢弃一切事件的出口。
struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: AgentEvent) {}
}
