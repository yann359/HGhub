import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { pluginError, type ExternalMedia } from './client';

export default function ExternalPlayback({ target }: { target: ExternalMedia }) {
  const [busy, setBusy] = useState(false), [message, setMessage] = useState('');
  const [dismissed, setDismissed] = useState(false);
  const open = async () => {
    if (target.expiresAt && target.expiresAt <= Date.now()) {
      setMessage('地址已过期，请刷新播放后再打开。'); return;
    }
    setBusy(true); setMessage('');
    try { await invoke('open_external_media', { url: target.url }); setMessage('已交给系统打开。'); }
    catch (error) { setMessage(pluginError(error)); }
    finally { setBusy(false); }
  };
  if (dismissed) return null;
  return <section className="hpp-notice max-h-[50vh] overflow-y-auto" aria-label="外部播放确认">
    <p>在外部应用打开{target.app ? ` · ${target.app}` : ''}</p>
    <p style={{ overflowWrap: 'anywhere' }}>{target.url}</p>
    <button disabled={busy} onClick={() => void open()}>{busy ? '正在打开…' : '确认打开'}</button>
    <button disabled={busy} onClick={() => setDismissed(true)}>取消</button>
    {message && <p role="status">{message}</p>}
  </section>;
}
