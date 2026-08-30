import { useEffect, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  Cpu,
  Eye,
  EyeOff,
  Keyboard,
  Loader2,
  Plug,
  Trash2,
  X,
  XCircle,
} from 'lucide-react';
import {
  useConfig,
  KEY_ACTION_LABEL,
  KEY_COMBO_LABEL,
  type KeyAction,
  type KeyCombo,
} from '@/store/config';

const KEY_COMBOS: KeyCombo[] = ['enter', 'shift_enter', 'ctrl_enter'];
const KEY_ACTIONS: KeyAction[] = ['send', 'newline', 'queue', 'preempt'];

/** 左侧类别导航。每类设置独立成页，点击切换，避免单页纵向堆叠。 */
const SECTIONS = [
  { id: 'connection', label: '连接', icon: Plug },
  { id: 'model', label: '模型', icon: Cpu },
  { id: 'input', label: '输入', icon: Keyboard },
] as const;

type SectionId = (typeof SECTIONS)[number]['id'];

/** 设置面板。以覆盖层形式出现，而非独立路由 —— 应用只有一个主界面。 */
export function Settings({ onClose }: { onClose: () => void }) {
  const {
    config,
    update,
    save,
    test,
    saving,
    testing,
    testResult,
    keyWarning,
    clearKey,
    keybindings,
    setKeybinding,
  } = useConfig();
  const [showKey, setShowKey] = useState(false);
  // 压缩阈值是小数，受控 number 输入框会把「0.」这类中间态立即吞成 0，
  // 用户根本敲不出小数点。改成字符串本地态，保存时才解析成数字。
  const [thresholdText, setThresholdText] = useState(String(config.compact_threshold));
  const [active, setActive] = useState<SectionId>('connection');

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const handleSave = async () => {
    // 阈值输入框只在保存时才解析，避免编辑中途被规范化掉小数点
    const raw = thresholdText.replace(',', '.');
    const n = Number(raw);
    if (raw.trim() !== '' && Number.isFinite(n)) {
      update({ compact_threshold: n });
    }
    if (await save()) onClose();
  };

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/50 p-6">
      <div className="flex max-h-[calc(100vh-3rem)] w-full max-w-2xl flex-col overflow-hidden rounded-(--radius-card) border border-(--border) bg-(--bg-elevated) shadow-2xl">
        <div className="flex items-center justify-between border-b border-(--border) px-5 py-3">
          <h2 className="text-sm font-medium">设置</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-(--fg-subtle) transition-colors hover:text-(--fg)"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="flex min-h-0 flex-1">
          <nav className="w-40 shrink-0 space-y-1 overflow-y-auto border-r border-(--border) p-2">
            {SECTIONS.map((s) => {
              const Icon = s.icon;
              const selected = active === s.id;
              return (
                <button
                  key={s.id}
                  type="button"
                  onClick={() => setActive(s.id)}
                  className={`relative flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-[13px] transition-colors ${
                    selected
                      ? 'bg-(--bg-inset) text-(--fg)'
                      : 'text-(--fg-muted) hover:bg-(--bg-inset)/60 hover:text-(--fg)'
                  }`}
                >
                  <span
                    className={`absolute top-1/2 left-0 h-4 w-[3px] -translate-y-1/2 rounded-r-full bg-(--color-accent) transition-opacity ${
                      selected ? 'opacity-100' : 'opacity-0'
                    }`}
                  />
                  <Icon className="size-4" />
                  {s.label}
                </button>
              );
            })}
          </nav>

          <div className="min-h-0 flex-1 overflow-y-auto p-5">
            {active === 'connection' && (
              <Section title="连接">
                <Field
                  label="API 地址"
                  hint="兼容 OpenAI 的 /chat/completions 端点。不会自动补 /v1，请按服务商文档填写完整前缀。"
                >
                  <input
                    value={config.base_url}
                    onChange={(e) => update({ base_url: e.target.value })}
                    placeholder="https://api.openai.com/v1"
                    className={inputClass}
                    spellCheck={false}
                  />
                </Field>

                <Field
                  label="API Key"
                  hint="保存在系统凭据管理器中，不会明文落盘。留空保存不会清除已存的密钥。"
                >
                  <div className="relative">
                    <input
                      value={config.api_key}
                      onChange={(e) => update({ api_key: e.target.value })}
                      type={showKey ? 'text' : 'password'}
                      placeholder="sk-..."
                      className={`${inputClass} pr-16`}
                      spellCheck={false}
                    />
                    <div className="absolute top-1/2 right-2 flex -translate-y-1/2 items-center gap-1">
                      {config.api_key && (
                        <button
                          type="button"
                          onClick={() => void clearKey()}
                          title="从系统凭据管理器中删除密钥"
                          className="text-(--fg-subtle) transition-colors hover:text-(--color-danger)"
                        >
                          <Trash2 className="size-3.5" />
                        </button>
                      )}
                      <button
                        type="button"
                        onClick={() => setShowKey((v) => !v)}
                        className="text-(--fg-subtle) transition-colors hover:text-(--fg)"
                      >
                        {showKey ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                      </button>
                    </div>
                  </div>
                </Field>

                {keyWarning && (
                  <div className="flex items-start gap-2 rounded-md bg-(--color-warn)/10 px-3 py-2 text-[13px] text-(--color-warn)">
                    <AlertTriangle className="mt-px size-4 shrink-0" />
                    <span>{keyWarning}</span>
                  </div>
                )}

                <button
                  type="button"
                  onClick={() => void test()}
                  disabled={testing}
                  className="flex items-center gap-1.5 rounded-md border border-(--border) px-3 py-1.5 text-[13px] text-(--fg-muted) transition-colors hover:bg-(--bg-inset) hover:text-(--fg) disabled:opacity-50"
                >
                  {testing && <Loader2 className="size-3.5 animate-spin" />}
                  测试连接
                </button>

                {testResult && (
                  <div
                    className={`flex items-start gap-2 rounded-md px-3 py-2 text-[13px] ${
                      testResult.ok
                        ? 'bg-(--color-ok)/10 text-(--color-ok)'
                        : 'bg-(--color-danger)/10 text-(--color-danger)'
                    }`}
                  >
                    {testResult.ok ? (
                      <CheckCircle2 className="mt-px size-4 shrink-0" />
                    ) : (
                      <XCircle className="mt-px size-4 shrink-0" />
                    )}
                    <span className="selectable break-words">{testResult.message}</span>
                  </div>
                )}
              </Section>
            )}

            {active === 'model' && (
              <Section title="模型">
                <Field label="模型">
                  <input
                    value={config.model}
                    onChange={(e) => update({ model: e.target.value })}
                    placeholder="gpt-4o-mini"
                    className={inputClass}
                    spellCheck={false}
                  />
                </Field>

                <Field
                  label="上下文窗口"
                  hint="仅用于状态栏显示占用比例和自动压缩的阈值判断，填错不影响正常对话。常见值：128000、200000、1000000。"
                >
                  <input
                    value={config.context_limit}
                    onChange={(e) => {
                      const n = Number(e.target.value.replace(/\D/g, ''));
                      update({ context_limit: Number.isFinite(n) ? n : 0 });
                    }}
                    inputMode="numeric"
                    placeholder="128000"
                    className={inputClass}
                    spellCheck={false}
                  />
                </Field>

                <Field
                  label="自动压缩阈值"
                  hint="上下文占用超过窗口的该比例时，把最老的历史折叠成摘要。0.7 表示留 30% 余量给当前轮次；填 0.5 更早压缩、0.9 更晚。"
                >
                  <input
                    value={thresholdText}
                    onChange={(e) => setThresholdText(e.target.value.replace(',', '.'))}
                    inputMode="decimal"
                    placeholder="0.7"
                    className={inputClass}
                    spellCheck={false}
                  />
                </Field>

                <Field
                  label="重试次数"
                  hint="请求失败后自动重试的次数。填 0 表示不重试；填 n 表示最多重试 n 次；留空表示无限重试（直到成功或手动停止）。"
                >
                  <input
                    value={config.max_retries ?? ''}
                    onChange={(e) => {
                      const raw = e.target.value;
                      if (raw.trim() === '') {
                        update({ max_retries: null });
                        return;
                      }
                      const n = Number(raw.replace(/\D/g, ''));
                      update({ max_retries: Number.isFinite(n) ? Math.floor(n) : 0 });
                    }}
                    inputMode="numeric"
                    placeholder="留空 = 无限重试"
                    className={inputClass}
                    spellCheck={false}
                  />
                </Field>
              </Section>
            )}

            {active === 'input' && (
              <Section title="输入">
                <Field
                  label="发送快捷键"
                  hint="定义 Enter、Shift + Enter、Ctrl + Enter 的行为。排队发送会等当前回复结束后再发；插队发送会在当前工具调用结束后停下当前回复，立即处理你的消息。"
                >
                  <div className="space-y-2">
                    {KEY_COMBOS.map((combo) => (
                      <div key={combo} className="flex items-center gap-3">
                        <span className="w-28 shrink-0 font-mono text-[12px] text-(--fg-muted)">
                          {KEY_COMBO_LABEL[combo]}
                        </span>
                        <select
                          value={keybindings[combo]}
                          onChange={(e) => setKeybinding(combo, e.target.value as KeyAction)}
                          className={selectClass}
                        >
                          {KEY_ACTIONS.map((action) => (
                            <option key={action} value={action}>
                              {KEY_ACTION_LABEL[action]}
                            </option>
                          ))}
                        </select>
                      </div>
                    ))}
                  </div>
                </Field>
              </Section>
            )}
          </div>
        </div>

        <div className="flex items-center justify-end border-t border-(--border) px-5 py-3">
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={saving}
            className="rounded-md bg-(--color-accent) px-4 py-1.5 text-[13px] font-medium text-(--color-base-950) transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}

const inputClass =
  'selectable w-full rounded-md border border-(--border) bg-(--bg-inset) px-3 py-2 font-mono text-[13px] outline-none transition-colors focus:border-(--color-accent) placeholder:text-(--fg-subtle) placeholder:font-sans';

const selectClass =
  'selectable flex-1 rounded-md border border-(--border) bg-(--bg-inset) px-3 py-2 text-[13px] outline-none transition-colors focus:border-(--color-accent)';

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h3 className="mb-3 text-[11px] font-semibold tracking-widest text-(--fg-muted)">{title}</h3>
      <div className="space-y-4">{children}</div>
    </section>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="mb-1.5 block text-[13px] text-(--fg-muted)">{label}</label>
      {children}
      {hint && <p className="mt-1.5 text-[11px] leading-relaxed text-(--fg-subtle)">{hint}</p>}
    </div>
  );
}
