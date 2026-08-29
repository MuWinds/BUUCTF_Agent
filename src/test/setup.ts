/**
 * 测试环境补齐浏览器 API。
 *
 * `requestAnimationFrame` 在 node 环境不存在，这里换成同步队列：
 * 帧不再由渲染器驱动，而是测试调用 `flushFrames()` 时才推进，
 * 于是「缓冲了几帧」「取消是否真的丢弃了增量」都能确定性断言。
 */

let queue: FrameRequestCallback[] = [];
let nextHandle = 1;
const pending = new Map<number, FrameRequestCallback>();

globalThis.requestAnimationFrame = (cb: FrameRequestCallback): number => {
  const handle = nextHandle++;
  pending.set(handle, cb);
  queue.push(cb);
  return handle;
};

globalThis.cancelAnimationFrame = (handle: number): void => {
  const cb = pending.get(handle);
  if (!cb) return;
  pending.delete(handle);
  queue = queue.filter((q) => q !== cb);
};

/** 执行所有已排队的帧回调。回调里再排新帧的，留到下次调用。 */
export function flushFrames(): void {
  const due = queue;
  queue = [];
  pending.clear();
  for (const cb of due) cb(0);
}

/** 清空未执行的帧，避免用例之间互相污染。 */
export function resetFrames(): void {
  queue = [];
  pending.clear();
}

// `import.meta.env.DEV` 在 vitest 下为真，会把每个事件都打到 stdout 淹没测试报告。
// 只静音 debug，warn/error 仍然可见。
console.debug = () => {};
