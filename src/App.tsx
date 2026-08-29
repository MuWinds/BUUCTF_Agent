import { useEffect, useState } from 'react';
import { TitleBar } from '@/components/layout/TitleBar';
import { StatusPanel } from '@/components/layout/StatusPanel';
import { MessageList } from '@/components/chat/MessageList';
import { Composer } from '@/components/chat/Composer';
import { Settings } from '@/pages/Settings';
import { useConfig } from '@/store/config';
import { useSession } from '@/store/session';
import { useWorkspace } from '@/store/workspace';

export default function App() {
  const init = useConfig((s) => s.init);
  const loaded = useConfig((s) => s.loaded);
  const model = useConfig((s) => s.config.model);
  const initWorkspace = useWorkspace((s) => s.init);
  const restoreSession = useSession((s) => s.restore);
  const [settingsOpen, setSettingsOpen] = useState(false);

  useEffect(() => {
    void init();
    void initWorkspace();
    void restoreSession();
  }, [init, initWorkspace, restoreSession]);

  // 首次启动没有配置模型时，直接把设置页推到用户面前
  useEffect(() => {
    if (loaded && !model) setSettingsOpen(true);
  }, [loaded, model]);

  return (
    <div className="relative flex h-full flex-col">
      <TitleBar />
      <MessageList />
      <Composer />
      <StatusPanel onOpenSettings={() => setSettingsOpen(true)} />
      {settingsOpen && <Settings onClose={() => setSettingsOpen(false)} />}
    </div>
  );
}
