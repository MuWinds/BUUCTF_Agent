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
      // Rust 侧已清空会话，前端也得跟上，否则界面还留着旧工作区的对话
      useSession.setState({ messages: [], usage: null });
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
