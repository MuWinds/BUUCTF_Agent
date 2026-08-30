import { create } from 'zustand';
import { open } from '@tauri-apps/plugin-dialog';
import { getWorkspace, setWorkspace } from '@/lib/ipc';
import { errorMessage } from '@/lib/events';
import { useSession } from './session';

interface WorkspaceState {
  path: string;
  error: string | null;
  init: () => Promise<void>;
  /** 弹出目录选择框并切换工作区。用户取消时什么都不做。 */
  pick: () => Promise<void>;
}

export const useWorkspace = create<WorkspaceState>((set) => ({
  path: '',
  error: null,

  async init() {
    try {
      set({ path: await getWorkspace() });
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },

  async pick() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected !== 'string') return;

    try {
      const path = await setWorkspace(selected);
      set({ path, error: null });
      // Rust 侧已载入新工作区最近一次的会话（没有历史则是空会话），
      // 前端先清掉旧工作区的消息再重新拉取，并同步会话列表。
      useSession.setState({
        messages: [],
        usage: null,
        streaming: false,
        queue: [],
        activeAssistantId: null,
        activeTurnId: null,
      });
      await useSession.getState().restore();
      await useSession.getState().refreshSessions();
    } catch (e) {
      set({ error: errorMessage(e) });
    }
  },
}));

/** 取路径最后一段用于紧凑显示。 */
export function basename(path: string): string {
  const parts = path.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
