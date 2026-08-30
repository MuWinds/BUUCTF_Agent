import { create } from 'zustand';
import { load, type Store } from '@tauri-apps/plugin-store';
import { getLlmConfig, setLlmConfig, testConnection, clearApiKey, type LlmConfig } from '@/lib/ipc';
import { errorMessage } from '@/lib/events';

const STORE_FILE = 'settings.json';
const KEY = 'llm';

/** 落盘的配置不含 api_key —— 密钥在 Rust 侧走系统凭据管理器。 */
type StoredConfig = Omit<LlmConfig, 'api_key'>;

interface ConfigState {
  config: LlmConfig;
  loaded: boolean;
  saving: boolean;
  testing: boolean;
  /** 连通性测试结果，null 表示尚未测试。 */
  testResult: { ok: boolean; message: string } | null;
  /** 密钥未能存入系统凭据管理器时的提示。 */
  keyWarning: string | null;
  init: () => Promise<void>;
  update: (patch: Partial<LlmConfig>) => void;
  save: () => Promise<boolean>;
  test: () => Promise<void>;
  /** 从系统凭据管理器中删除密钥。 */
  clearKey: () => Promise<void>;
}

let store: Store | null = null;
async function getStore() {
  store ??= await load(STORE_FILE, { autoSave: false });
  return store;
}

export const useConfig = create<ConfigState>((set, get) => ({
  config: {
    base_url: 'https://api.openai.com/v1',
    api_key: '',
    model: '',
    temperature: null,
    context_limit: 128000,
    compact_threshold: 0.7,
    max_retries: 2,
  },
  loaded: false,
  saving: false,
  testing: false,
  testResult: null,
  keyWarning: null,

  /** 从磁盘和凭据管理器恢复配置。启动时调用一次。 */
  async init() {
    // Rust 侧启动时已从凭据管理器读出了 api_key
    let config = await getLlmConfig();

    try {
      const s = await getStore();
      const saved = await s.get<StoredConfig & { api_key?: string }>(KEY);

      if (saved) {
        const { api_key: legacyKey, ...rest } = saved;
        config = { ...config, ...rest };

        // 迁移：早期版本把密钥明文存在 settings.json 里。
        // 凭据管理器为空而磁盘上还有旧密钥时，搬过去并从磁盘抹掉。
        if (!config.api_key && legacyKey) {
          config.api_key = legacyKey;
          await s.set(KEY, rest);
          await s.save();
          console.info('已把 API Key 迁移到系统凭据管理器');
        }

        // 磁盘配置未必有效（比如模型名被清空），无效就不推给 Rust，
        // 让用户在设置页里看到问题而不是发消息时才炸
        await setLlmConfig(config).catch(() => undefined);
      }
    } catch (e) {
      console.warn('读取本地配置失败，使用默认值', e);
    }

    set({ config, loaded: true });
  },

  update(patch) {
    set((s) => ({ config: { ...s.config, ...patch }, testResult: null }));
  },

  async save() {
    set({ saving: true });
    try {
      const { config } = get();
      const { key_persisted } = await setLlmConfig(config);

      // 解构剥掉 api_key，只把其余字段落盘
      const { api_key: omittedKey, ...persistable } = config;
      const s = await getStore();
      await s.set(KEY, persistable);
      await s.save();

      set({
        keyWarning: key_persisted
          ? null
          : '系统凭据管理器不可用，API Key 只保存在内存中，重启后需要重新填写。',
      });
      return true;
    } catch (e) {
      set({ testResult: { ok: false, message: errorMessage(e) } });
      return false;
    } finally {
      set({ saving: false });
    }
  },

  async test() {
    set({ testing: true, testResult: null });
    try {
      const message = await testConnection(get().config);
      set({ testResult: { ok: true, message } });
    } catch (e) {
      set({ testResult: { ok: false, message: errorMessage(e) } });
    } finally {
      set({ testing: false });
    }
  },

  async clearKey() {
    await clearApiKey();
    set((s) => ({
      config: { ...s.config, api_key: '' },
      testResult: null,
      keyWarning: null,
    }));
  },
}));
