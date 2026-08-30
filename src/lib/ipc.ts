import { Channel, invoke } from '@tauri-apps/api/core';
import type { AgentEvent } from './events';
import type { Session, SessionList } from './session';

export interface LlmConfig {
  base_url: string;
  api_key: string;
  model: string;
  temperature: number | null;
  context_limit: number;
  /** 自动压缩触发阈值（0~1），默认 0.7。 */
  compact_threshold: number;
  /** 请求失败后的自动重试次数；null = 无限重试，0 = 不重试。 */
  max_retries: number | null;
}

/**
 * 发送消息并驱动一个轮次。
 *
 * 返回的 Promise 直到轮次结束才 resolve；期间输出全部走 `onEvent`。
 * 用 Tauri v2 的 Channel 而非全局事件：无需按事件名路由，也不必管理 unlisten。
 */
export function sendMessage(text: string, onEvent: (e: AgentEvent) => void) {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke<void>('send_message', { text, onEvent: channel });
}

export const cancelTurn = () => invoke<void>('cancel_turn');
/** 通知后端「插队」：指定轮次在当前 tool call 结束后停下，把位置让给新消息。 */
export const preemptTurn = (turnId: string) => invoke<void>('preempt_turn', { turnId });
/** 读取已保存的会话，用于启动时还原界面。 */
export const getSession = () => invoke<Session>('get_session');
/** 列出当前工作区的全部会话摘要，供列表页渲染。 */
export const listSessions = () => invoke<SessionList>('list_sessions');
/** 切换到指定会话并载入其内容。 */
export const switchSession = (id: string) => invoke<void>('switch_session', { id });
/** 新建一段空会话并切换到它，旧会话保留在磁盘上。 */
export const newSession = () => invoke<void>('new_session');
/** 删除指定会话。删除当前会话时退回新会话。 */
export const deleteSession = (id: string) => invoke<void>('delete_session', { id });
/**
 * 回退/分叉：截断当前会话到第 entry_index 条消息，旧分支另存为新会话。
 * 返回最新会话列表（含新保存的旧分支）。
 */
export const rewindSession = (entryIndex: number) =>
  invoke<SessionList>('rewind_session', { entryIndex });

export interface SaveResult {
  /** 密钥是否成功存入系统凭据管理器。 */
  key_persisted: boolean;
}

export const getLlmConfig = () => invoke<LlmConfig>('get_llm_config');
export const setLlmConfig = (config: LlmConfig) => invoke<SaveResult>('set_llm_config', { config });
/** 从系统凭据管理器中删除已保存的密钥。 */
export const clearApiKey = () => invoke<void>('clear_api_key');
export const testConnection = (config: LlmConfig) => invoke<string>('test_connection', { config });

export const getWorkspace = () => invoke<string>('get_workspace');
/** 返回规范化后的绝对路径。切换工作区会清空会话历史。 */
export const setWorkspace = (path: string) => invoke<string>('set_workspace', { path });
