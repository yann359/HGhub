import { useEffect, useId, useMemo, useRef, useState } from "react";
import { onPluginAuthFailure, openPluginLink, pluginCommand, pluginError, releaseMedia, type InstalledPlugin } from "./client";
import type { AuthMethod, AuthResult, Form, ImageRef, PluginFailure, UserProfile } from "./protocol";
import { usePluginRequests } from "./usePluginRequests";

type Action = { id: string; label: string; description?: string; form: Form };
type Target = { kind: "auth" | "ui"; id: string; label: string; form?: Form };
type ActionResult = { status: "ok"; message?: string } | { status: "next"; form: Form };
type Methods = { methods: AuthMethod[]; registerUrl?: string };

function initialValues(form: Form | undefined, previous: Record<string, string> = {}) {
  // Explicit next-step defaults can rotate hidden challenges. Other prior fields survive.
  return Object.fromEntries([
    ...Object.entries(previous),
    ...(form?.fields || []).map(f => [f.key, f.value ?? (Object.prototype.hasOwnProperty.call(previous, f.key) ? previous[f.key] : "")]),
  ]);
}
function useFormImages(id: string, refs: ImageRef[]) {
  const [urls, setUrls] = useState<string[]>([]);
  useEffect(() => {
    let disposed = false, sessionId = "";
    setUrls([]);
    if (refs.length) void pluginCommand<{ sessionId: string; urls: string[] }>("assets.prepare", { id, assets: refs }).then(r => {
      sessionId = r.sessionId;
      if (disposed) releaseMedia(sessionId); else setUrls(r.urls);
    }).catch(() => {});
    return () => { disposed = true; if (sessionId) releaseMedia(sessionId); };
  }, [id, refs]);
  return urls;
}

export default function PluginForms({ plugin }: { plugin: InstalledPlugin }) {
  const request = usePluginRequests(), fieldId = useId(), generation = useRef(0), authEpoch = useRef(0);
  const [methods, setMethods] = useState<Methods>({ methods: [] }), [actions, setActions] = useState<Action[]>([]);
  const [profile, setProfile] = useState<UserProfile | null>(null), [target, setTarget] = useState<Target | null>(null);
  const [values, setValues] = useState<Record<string, string>>({}), [error, setError] = useState("");
  const [accountError, setAccountError] = useState(""), [actionsError, setActionsError] = useState("");
  const [notice, setNotice] = useState(""), [busy, setBusy] = useState(false), [loading, setLoading] = useState(false), [refresh, setRefresh] = useState(0);
  const hasAuth = plugin.methods.includes("auth.getMethods"), hasUi = plugin.methods.includes("ui.getActions");
  const ready = plugin.enabled && (plugin.status === "ready" || plugin.status === "unloaded");
  const imageRefs = useMemo(() => [
    ...(profile?.avatar ? [profile.avatar] : []),
    ...(target?.form?.fields.flatMap(f => f.image ? [f.image] : []) || []),
  ], [profile, target?.form]);
  const images = useFormImages(plugin.meta.id, imageRefs);
  const closeForm = () => { setTarget(null); setValues({}); setError(""); };
  useEffect(() => onPluginAuthFailure(f => {
    if (f.id !== plugin.meta.id) return;
    authEpoch.current++;
    setProfile(null);
    if (!f.silent) setAccountError(f.code === "AUTH_EXPIRED" ? "登录已失效，请重新登录" : "请先登录后重试该操作");
  }), [plugin.meta.id]);
  useEffect(() => {
    const n = ++generation.current;
    setTarget(null); setValues({}); setProfile(null); setMethods({ methods: [] }); setActions([]);
    setAccountError(""); setActionsError(""); setError(""); setNotice(""); setBusy(false);
    if (!ready || (!hasAuth && !hasUi)) return;
    setLoading(true);
    const epoch = authEpoch.current;
    const jobs: Promise<unknown>[] = [];
    if (hasAuth) {
      jobs.push(request<Methods>("form.actions", { id: plugin.meta.id, target: "auth" }).then(r => { if (n === generation.current) setMethods(r); }).catch(e => { if (n === generation.current) setAccountError(pluginError(e)); }));
      jobs.push(request<UserProfile | null>("call", { id: plugin.meta.id, method: "auth.getProfile", args: {} }).then(r => { if (n === generation.current && epoch === authEpoch.current) setProfile(r); }).catch(e => { if (n === generation.current) setAccountError(pluginError(e)); }));
    }
    if (hasUi) jobs.push(request<Action[]>("form.actions", { id: plugin.meta.id, target: "ui" }).then(r => { if (n === generation.current) setActions(r); }).catch(e => { if (n === generation.current) setActionsError(pluginError(e)); }));
    void Promise.allSettled(jobs).then(() => { if (n === generation.current) setLoading(false); });
    return () => { generation.current++; };
  }, [plugin, hasAuth, hasUi, ready, request, refresh]);

  const submit = async (chosen: Target, submitted: Record<string, string>) => {
    const n = generation.current;
    setBusy(true); setError(""); setNotice("");
    try {
      const result = await request<AuthResult | ActionResult>("form.submit", { id: plugin.meta.id, target: { kind: chosen.kind, id: chosen.id }, values: submitted });
      if (n !== generation.current) return;
      if (result.status === "next") {
        setTarget({ ...chosen, form: result.form });
        setValues(initialValues(result.form, submitted));
      } else {
        if ("user" in result) { setProfile(result.user); setAccountError(""); }
        closeForm(); setNotice("message" in result ? result.message || "已完成" : chosen.kind === "auth" ? "登录成功" : "已完成");
      }
    } catch (e) {
      if (n !== generation.current || (e as PluginFailure)?.code === "CANCELLED") return;
      setError(pluginError(e));
    } finally { if (n === generation.current) setBusy(false); }
  };
  const choose = (chosen: Target) => {
    setTarget(chosen); setValues(initialValues(chosen.form)); setError(""); setNotice("");
    if (!chosen.form) void submit(chosen, {});
  };
  const logout = async () => {
    const n = generation.current; setBusy(true); setAccountError(""); setNotice(""); closeForm();
    try {
      await request("call", { id: plugin.meta.id, method: "auth.logout", args: {} });
      if (n === generation.current) { setProfile(null); setNotice("已退出登录"); }
    } catch (e) {
      if (n !== generation.current) return;
      // Cleanup belongs to the plugin even when its upstream logout fails. Query its
      // resulting state instead of pretending logout succeeded or clearing its storage.
      setAccountError(pluginError(e));
      try {
        const user = await request<UserProfile | null>("call", { id: plugin.meta.id, method: "auth.getProfile", args: {} });
        if (n === generation.current) setProfile(user);
      } catch { if (n === generation.current) setProfile(null); }
    } finally { if (n === generation.current) setBusy(false); }
  };
  if (!ready || (!hasAuth && !hasUi)) return null;
  const missingRequired = target?.form?.fields.some(f => f.required && !values[f.key]);
  let imageIndex = profile?.avatar ? 1 : 0;
  return <section className="hpp-forms" aria-label="插件账号与操作">
    {hasAuth && <div className="hpp-account"><h3>插件账号</h3>
      {accountError && <p role="alert" className="hpp-error">{accountError}</p>}
      {profile ? <div className="hpp-profile">{images[0] && profile.avatar && <img src={images[0]} alt="账号头像" />}<div><strong>{profile.name}</strong>{profile.description && <p>{profile.description}</p>}{profile.stats && <p>{Object.entries(profile.stats).map(([label, count]) => `${label} ${count}`).join(" · ")}</p>}</div><button disabled={busy || loading} onClick={() => void logout()}>退出登录</button></div> : <div className="hpp-form-actions">{methods.methods.map(m => <button disabled={busy || loading} key={m.id} onClick={() => choose({ ...m, kind: "auth" })}>{m.label}</button>)}{!loading && !methods.methods.length && <p className="hpp-muted">暂无可用登录方式</p>}</div>}
      {methods.registerUrl && <button className="hpp-register" onClick={() => void openPluginLink(methods.registerUrl!).catch(e => setAccountError(pluginError(e)))}>注册账号</button>}
    </div>}
    {hasUi && <div className="hpp-actions"><h3>插件操作</h3>{actionsError && <p role="alert" className="hpp-error">{actionsError}</p>}<div className="hpp-action-list">{actions.map(a => <button key={a.id} aria-label={a.label} disabled={busy || loading} onClick={() => choose({ ...a, kind: "ui" })}><span>{a.label}{a.description && <small>{a.description}</small>}</span><span aria-hidden="true">›</span></button>)}</div></div>}
    {loading && <p role="status" className="hpp-muted">正在读取账号与操作…</p>}
    {(accountError || actionsError) && <button disabled={loading || busy} onClick={() => setRefresh(n => n + 1)}>重新读取账号与操作</button>}
    {target && <form className="hpp-form" aria-label={target.form?.title || target.label} onSubmit={e => { e.preventDefault(); if (!busy && !missingRequired) void submit(target, values); }}>
      <h3>{target.form?.title || target.label}</h3>{target.form?.description && <p className="hpp-muted">{target.form.description}</p>}
      {target.form?.fields.map((field, i) => {
        const id = `${fieldId}-${i}`, value = values[field.key] ?? "";
        const image = field.image ? images[imageIndex++] : undefined;
        if (field.type === "hidden") return <input key={field.key} type="hidden" value={value} readOnly />;
        return <div className="hpp-form-field" key={field.key}><label htmlFor={id}>{field.label || field.key}{field.required && <span aria-hidden="true"> *</span>}</label>
          {image && <img className="hpp-captcha" src={image} alt={field.label ? `${field.label}题面` : "验证码题面"} />}
          {field.type === "select" ? <select id={id} aria-label={field.label || field.key} disabled={busy} required={field.required} value={value} onChange={e => setValues(old => ({ ...old, [field.key]: e.target.value }))}><option value="">请选择</option>{field.options?.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}</select> : <input id={id} aria-label={field.label || field.key} disabled={busy} required={field.required} value={value} placeholder={field.placeholder} type={field.type === "password" || field.type === "email" || field.type === "number" ? field.type : "text"} step={field.type === "number" ? "any" : undefined} inputMode={field.type === "otp" ? "numeric" : undefined} autoComplete={field.type === "password" ? "current-password" : field.type === "otp" ? "one-time-code" : "off"} onChange={e => setValues(old => ({ ...old, [field.key]: e.target.value }))} />}
        </div>;
      })}
      {error && <p role="alert" className="hpp-error">{error}</p>}
      <div className="hpp-form-actions">{target.form && <button className="primary" type="submit" disabled={busy || missingRequired}>{busy ? "正在提交…" : target.form.submitLabel || "提交"}</button>}<button type="button" disabled={busy} onClick={closeForm}>取消</button></div>
    </form>}
    {notice && <p role="status" className="hpp-form-notice">{notice}</p>}
  </section>;
}
