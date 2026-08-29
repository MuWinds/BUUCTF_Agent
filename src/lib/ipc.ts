import { Channel, invoke } from '@tauri-apps/api/core';
import type { AgentEvent } from './events';
import type { Session } from './session';

export interface LlmConfig {
  base_url: string;
  api_key: string;
  model: string;
  temperature: number | null;
  context_limit: number;
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
export const clearHistory = () => invoke<void>('clear_history');
/** 读取已保存的会话，用于启动时还原界面。 */
export const getSession = () => invoke<Session>('get_session');

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
