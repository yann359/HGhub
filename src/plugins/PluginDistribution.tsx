import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { pluginCommand, pluginError, type InstalledPlugin } from "./client";
import type { PluginFailure } from "./protocol";
import { usePluginRequests } from "./usePluginRequests";
import { useHost } from "./HostContext";
import { platform } from "../environment";

interface ImportItem {
  key: string; id: string; name: string; description: string; version: string; source: string;
  installedVersion?: string; error?: PluginFailure;
}
interface Preview { token: string; name: string; description: string; items: ImportItem[]; updatesOnly: boolean; subscriptions: { location: string; name: string; description: string }[] }
interface Outcome { key: string; id: string; plugin?: InstalledPlugin; error?: PluginFailure }
interface UpdateStatus { id: string; status: "current" | "available" | "failed" | "noSource"; error?: PluginFailure }
const newer = (next: string, current?: string) => {
  if (!current) return true;
  const a = next.split(".").map(Number), b = current.split(".").map(Number);
  for (let i = 0; i < 3; i++) if ((a[i] || 0) !== (b[i] || 0)) return (a[i] || 0) > (b[i] || 0);
  return false;
};

export default function PluginDistribution({ plugins, reload }: { plugins: InstalledPlugin[]; reload: () => Promise<void> }) {
  const request = usePluginRequests();
  const host = useHost();
  const [location, setLocation] = useState(""), [code, setCode] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null), [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [downgrade, setDowngrade] = useState(false), [installOnly, setInstallOnly] = useState(false);
  const [report, setReport] = useState<Outcome[]>([]), [updates, setUpdates] = useState<UpdateStatus[]>([]);
  const [directoryFiles, setDirectoryFiles] = useState<{ name: string; location: string }[]>([]);
  const token = useRef<string | null>(null);
  useEffect(() => () => { if (token.current) void pluginCommand("distribution.discard", { token: token.current }).catch(() => {}); }, []);
  const replacePreview = (next: Preview | null, changedOnly = false) => {
    if (token.current && token.current !== next?.token) void pluginCommand("distribution.discard", { token: token.current }).catch(() => {});
    token.current = next?.token || null; setPreview(next); setSelected(next?.items.filter(i => !i.error && (!changedOnly || newer(i.version, i.installedVersion))).map(i => i.key) || []);
  };
  const run = async (work: () => Promise<void>) => {
    setBusy(true); setError("");
    try { await work(); } catch (e) { setError(pluginError(e)); }
    finally { setBusy(false); }
  };
  const readLocations = async (locations: string[]) => {
    if (!locations.length) return;
    setReport([]); setUpdates([]);
    replacePreview(await request<Preview>("distribution.preview", { locations }));
  };
  const apply = () => run(async () => {
    if (!preview) return;
    const result = await request<{ items: Outcome[] }>("distribution.apply", { token: preview.token, selected, allowDowngrade: downgrade, installOnly });
    token.current = null; setPreview(null); setReport(result.items); setUpdates([]); await reload();
  });
  return <div className="plugin-distribution-grid">
    <section className="hpp-subscriptions" aria-label="插件订阅"><div className="hpp-section-title"><h2>插件订阅</h2><span>{host.updates.subscriptions.length} 个列表</span></div>
      {host.updates.subscriptions.length ? host.updates.subscriptions.map(subscription => <div className="hpp-subscription" key={subscription.id}><div><strong>{subscription.name}</strong><small>{subscription.location}</small>{subscription.description && <p>{subscription.description}</p>}</div>
        <button disabled={busy} onClick={() => void run(async () => { setUpdates([]); setReport([]); replacePreview(await request<Preview>("subscriptions.refresh", { id: subscription.id }), true); })}>刷新订阅 · {subscription.name}</button>
        <button disabled={busy} onClick={() => void run(async () => { await request("subscriptions.remove", { id: subscription.id }); await host.refreshUpdates(); })}>取消订阅 · {subscription.name}</button></div>) : <p className="hpp-muted">读取远程插件列表后，可将其保存为订阅。刷新后勾选新增插件或更新。</p>}
    </section>
    {host.updates.items.length>0&&<section className="hpp-update-queue" aria-label="插件更新队列"><h2>插件更新</h2>{host.updates.items.map(item => <div className="hpp-subscription" key={item.id}><div><strong>{item.name} · v{item.version}</strong><small>{item.error?.message || ({available:"有新版本",deferred:"等待播放结束后安装",installed:"已更新",current:"已是此版本或更高版本",failed:"更新失败，可重新检查或恢复上一版本"})[item.status]}</small></div>
      {["available","deferred"].includes(item.status)&&<button disabled={busy} onClick={() => void run(async () => { await request("updates.install", { ids: [item.id] }); await host.refreshUpdates(); await reload(); })}>安装更新 · {item.name}</button>}
      <button disabled={busy} onClick={() => void run(async () => { await request("updates.dismiss", { id: item.id }); await host.refreshUpdates(); })}>忽略更新 · {item.name}</button>
    </div>)}</section>}
    <section className="hpp-import"><div><h2>从网址导入插件或订阅列表</h2></div>
      <form className="hpp-distribution-location" onSubmit={e => { e.preventDefault(); void run(() => readLocations([location.trim()])); }}>
        <input aria-label="插件或列表地址" placeholder="HTTPS 地址或本地 manifest.json 完整路径" value={location} onChange={e => setLocation(e.target.value)} />
        <button disabled={busy || !location.trim()}>读取列表或插件</button>
      </form>
      <div className="hpp-distribution-buttons"><label className="hpp-check"><input type="checkbox" checked={downgrade} disabled={busy} onChange={e => setDowngrade(e.target.checked)} />允许导入更低版本</label>
        <label className="hpp-check"><input type="checkbox" checked={installOnly} disabled={busy} onChange={e => setInstallOnly(e.target.checked)} />仅安装，暂不启用</label></div>
      {busy && <button onClick={() => request.cancelAll()}>取消当前操作</button>}
    </section>
    <section className="plugin-local-import"><h2>本地文件与开发调试</h2>      <div className="hpp-distribution-buttons">
        <button className="primary" disabled={busy} onClick={() => void run(async () => readLocations(await invoke<string[]>("pick_plugin_files")))}>选择插件文件</button>
        {platform === "android" && <button disabled={busy} onClick={() => void run(async () => setDirectoryFiles(await invoke<{ name: string; location: string }[]>("pick_plugin_directory")))}>选择列表所在目录</button>}
        <button disabled={busy || !plugins.length} onClick={() => void run(async () => {
          setReport([]); const result = await request<{ preview: Preview; statuses: UpdateStatus[] }>("updates.check", { ids: plugins.map(p => p.meta.id) });
          replacePreview(result.preview.items.length ? result.preview : null); if (!result.preview.items.length) void pluginCommand("distribution.discard", { token: result.preview.token });
          setUpdates(result.statuses);
        })}>检查插件更新</button>
      </div>
      {platform === "ios" && <p className="hpp-muted">导入含相对路径的列表时，请一次选中列表和同级的插件文件。</p>}
      {directoryFiles.length > 0 && <div className="hpp-distribution-buttons" aria-label="目录中的插件列表">{directoryFiles.map(file => <button key={file.location} disabled={busy} onClick={() => void run(async () => { await readLocations([file.location]); setDirectoryFiles([]); })}>{file.name}</button>)}<button disabled={busy} onClick={() => setDirectoryFiles([])}>关闭目录</button></div>}
      <details><summary>粘贴代码导入</summary><textarea aria-label="插件代码" placeholder="export const meta = …" spellCheck={false} value={code} onChange={e => setCode(e.target.value)} />
        <button disabled={busy || !code.trim()} onClick={() => void run(async () => {
          const plugin = await request<InstalledPlugin>("import", { code, allowDowngrade: downgrade, installOnly });
          replacePreview(null); setReport([{ key: plugin.meta.id, id: plugin.meta.id, plugin }]); await reload();
        })}>导入代码</button>
      </details>
</section>
    {error && <div className="hpp-error" role="alert">{error}</div>}
    {preview && <section className="hpp-distribution-preview" aria-label="插件导入预览"><div className="hpp-section-title"><h2>{preview.name}</h2><span>{preview.items.length} 个条目</span></div><p className="hpp-muted">{preview.description}</p>
      {preview.subscriptions?.map(subscription => <button key={subscription.location} disabled={busy || host.updates.subscriptions.some(s => s.location === subscription.location)} onClick={() => void run(async () => { await request("subscriptions.save", { token: preview.token, location: subscription.location }); await host.refreshUpdates(); })}>{host.updates.subscriptions.some(s => s.location === subscription.location) ? "已保存订阅" : `保存订阅 · ${subscription.name}`}</button>)}
      <label className="hpp-check"><input type="checkbox" disabled={busy} checked={preview.items.some(i => !i.error) && preview.items.filter(i => !i.error).every(i => selected.includes(i.key))} onChange={e => setSelected(e.target.checked ? preview.items.filter(i => !i.error).map(i => i.key) : [])} />全选可安装条目</label>
      <div className="hpp-distribution-items">{preview.items.map(item => <label key={item.key} className="hpp-distribution-item">
        <input type="checkbox" aria-label={`选择 ${item.name}`} checked={selected.includes(item.key)} disabled={busy || !!item.error} onChange={e => setSelected(old => e.target.checked ? [...old, item.key] : old.filter(k => k !== item.key))} />
        <span><strong>{item.name}</strong><small>{item.id} {item.version && `· v${item.version}`}{item.installedVersion && ` · 已安装 v${item.installedVersion}`}</small>{item.description && <small>{item.description}</small>}<small>{item.source}</small>{item.error && <small className="hpp-danger">{item.error.message}</small>}</span>
      </label>)}</div>
      <div className="hpp-distribution-buttons"><button className="primary" disabled={busy || !selected.length} onClick={() => void apply()}>{preview.updatesOnly ? "安装所选更新" : "导入所选插件"}（{selected.length}）</button><button disabled={busy} onClick={() => replacePreview(null)}>关闭预览</button></div>
    </section>}
    {!!report.length && <section className="hpp-distribution-results" aria-label="插件导入结果"><h3>导入结果</h3>{report.map(item => <p key={item.key} className={item.error || item.plugin?.error ? "hpp-danger" : "hpp-muted"}>
      {item.plugin?.meta.name || item.id || "文件"} · {item.error ? item.error.message : item.plugin?.error ? `已安装，初始化失败：${item.plugin.error.message}` : item.plugin?.enabled ? "已安装并启用" : "已安装，未启用"}
    </p>)}</section>}
    {!!updates.length && <section className="hpp-distribution-results" aria-label="插件更新检查结果"><h3>更新检查</h3>{updates.map((item, i) => <p className={item.status === "failed" ? "hpp-danger" : "hpp-muted"} key={item.id + i}>
      {plugins.find(p => p.meta.id === item.id)?.meta.name || item.id} · {item.error?.message || (item.status === "available" ? "有新版本，可在上方选择安装" : item.status === "noSource" ? "没有更新地址" : "已是最新版本")}
    </p>)}</section>}
  </div>;
}
