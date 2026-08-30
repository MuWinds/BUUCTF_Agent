import { useEffect, useState } from 'react';
import {
  X,
  Loader2,
  CheckCircle2,
  XCircle,
  Eye,
  EyeOff,
  AlertTriangle,
  Trash2,
} from 'lucide-react';
import { useConfig } from '@/store/config';

/** 设置面板。以覆盖层形式出现，而非独立路由 —— 应用只有一个主界面。 */
export function Settings({ onClose }: { onClose: () => void }) {
  const { config, update, save, test, saving, testing, testResult, keyWarning, clearKey } =
    useConfig();
  const [showKey, setShowKey] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const handleSave = async () => {
    if (await save()) onClose();
  };

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/50 p-6">
      <div className="w-full max-w-lg overflow-hidden rounded-(--radius-card) border border-(--border) bg-(--bg-elevated) shadow-2xl">
        <div className="flex items-center justify-between border-b border-(--border) px-5 py-3">
          <h2 className="text-sm font-medium">模型设置</h2>
          <button
            type="button"
            onClick={onClose}
            className="text-(--fg-subtle) transition-colors hover:text-(--fg)"
          >
            <X className="size-4" />
          </button>
        </div>

        <div className="space-y-4 p-5">
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
              value={config.compact_threshold}
              onChange={(e) => {
                const raw = e.target.value.replace(',', '.');
                const n = Number(raw);
                update({ compact_threshold: Number.isFinite(n) ? n : 0.7 });
              }}
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

          {keyWarning && (
            <div className="flex items-start gap-2 rounded-md bg-(--color-warn)/10 px-3 py-2 text-[13px] text-(--color-warn)">
              <AlertTriangle className="mt-px size-4 shrink-0" />
              <span>{keyWarning}</span>
            </div>
          )}

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
        </div>

        <div className="flex items-center justify-between border-t border-(--border) px-5 py-3">
          <button
            type="button"
            onClick={() => void test()}
            disabled={testing}
            className="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[13px] text-(--fg-muted) transition-colors hover:bg-(--bg-inset) hover:text-(--fg) disabled:opacity-50"
          >
            {testing && <Loader2 className="size-3.5 animate-spin" />}
            测试连接
          </button>

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
