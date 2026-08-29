import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useState } from 'react';
import { Minus, Square, Copy, X } from 'lucide-react';

/**
 * 自绘标题栏。
 *
 * 窗口在 tauri.conf.json 中设了 `decorations: false`，所以拖拽、最小化、
 * 最大化、关闭全部要自己实现。拖拽区靠 `data-tauri-drag-region` 属性，
 * 由 WebView 层直接处理，不经过 JS，因此拖动不会掉帧。
 */
export function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    void win.isMaximized().then(setMaximized);
    // onResized 会在最大化/还原时触发，用它同步图标状态
    const unlisten = win.onResized(() => void win.isMaximized().then(setMaximized));
    return () => void unlisten.then((f) => f());
  }, []);

  const win = getCurrentWindow();

  return (
    <div
      data-tauri-drag-region
      className="flex h-9 shrink-0 items-center justify-between border-b border-(--border) bg-(--bg-elevated) pl-3 select-none"
    >
      <div data-tauri-drag-region className="flex items-center gap-2 text-xs">
        <div className="size-2 rounded-full bg-accent" />
        <span className="font-medium text-(--fg-muted)">Coding Agent</span>
      </div>

      <div className="flex h-full">
        <TitleBarButton onClick={() => void win.minimize()} label="最小化">
          <Minus className="size-3.5" />
        </TitleBarButton>
        <TitleBarButton onClick={() => void win.toggleMaximize()} label="最大化">
          {maximized ? <Copy className="size-3" /> : <Square className="size-3" />}
        </TitleBarButton>
        <TitleBarButton onClick={() => void win.close()} label="关闭" danger>
          <X className="size-3.5" />
        </TitleBarButton>
      </div>
    </div>
  );
}

function TitleBarButton({
  children,
  onClick,
  label,
  danger,
}: {
  children: React.ReactNode;
  onClick: () => void;
  label: string;
  danger?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className={[
        'flex w-11 items-center justify-center text-(--fg-muted) transition-colors',
        danger
          ? 'hover:bg-(--color-danger) hover:text-white'
          : 'hover:bg-(--bg-inset) hover:text-(--fg)',
      ].join(' ')}
    >
      {children}
    </button>
  );
}
