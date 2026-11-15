import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { platform } from "./environment";
import type { SourceStart } from "./plugins/sourceMatching";
export type Tab = "home" | "category" | "search" | "profile";
export type Theme = "dark" | "light";
export type ThemeMode = Theme | "system";
export type PageInput = { kind: "plugins"; focusId?: string } | { kind: "settings" } | { kind: "media"; pluginId: string; mediaId: string; start?: SourceStart } | { kind: "section"; pluginId: string; sectionId: string; title?: string } | { kind: "live"; pluginId: string } | { kind: "articles"; pluginId: string } | { kind: "article"; pluginId: string; articleId: string } | { kind: "feed"; pluginId: string } | { kind: "recommendations"; pluginId: string };
export type Page = PageInput & { key: string };
function read<T>(key: string, fallback: T): T { try { return JSON.parse(localStorage.getItem(key) || "null") ?? fallback; } catch { return fallback; } }
function useApplication() {
  const [themeMode, setTheme] = useState<ThemeMode>(() => { const v = read<string>("hpp.theme", "system"); return v === "light" || v === "dark" ? v : "system"; });
  const [systemDark, setSystemDark] = useState(() => matchMedia("(prefers-color-scheme: dark)").matches);
  const theme: Theme = themeMode === "system" ? (systemDark ? "dark" : "light") : themeMode;
  useEffect(() => {
    const query = matchMedia("(prefers-color-scheme: dark)");
    const change = () => setSystemDark(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  const [tab, setTabRaw] = useState<Tab>("home"), [stack, setStack] = useState<Page[]>([]);
  const [muteOnStartup, setMuteOnStartup] = useState(() => read<boolean>("hpp.startMuted", true) !== false);
  const [volume, setVolume] = useState(() => { const v = read("hpp.volume", 0.7); return typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0.7; });
  const [muted, setMuted] = useState(muteOnStartup);
  const [theater, setTheater] = useState(false), [fs, setFs] = useState(false), [clean, setClean] = useState(false);
  const [strictSecurity, setStrict] = useState(true), [strictSecurityReady, setSecurityReady] = useState(false), [securityPending, setPending] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    document.documentElement.classList.toggle("theme-light", theme === "light"); document.documentElement.style.colorScheme = theme;
    try { localStorage.setItem("hpp.theme", JSON.stringify(themeMode)); localStorage.setItem("hpp.startMuted", JSON.stringify(muteOnStartup)); localStorage.setItem("hpp.volume", JSON.stringify(volume)); } catch { setError("无法保存应用偏好"); }
  }, [theme, themeMode, muteOnStartup, volume]);
  useEffect(() => { void invoke<{strict_security:boolean}>("app_boot").then(v => { setStrict(v.strict_security); setSecurityReady(true); }).catch(() => setError("无法读取网络设置")); }, []);
  const setStrictSecurity = useCallback(async (strict: boolean) => { setPending(true); try { setStrict(await invoke<boolean>("security_set", { strict })); } finally { setPending(false); } }, []);
  const setTab = useCallback((tab: Tab) => { setTabRaw(tab); setStack([]); }, []);
  const push = useCallback((p: PageInput) => setStack(old => [...old, { ...p, key: crypto.randomUUID() }]), []);
  const replace = useCallback((p: PageInput) => setStack(old => [...old.slice(0, -1), { ...p, key: crypto.randomUUID() }]), []);
  const pop = useCallback(() => setStack(old => old.slice(0, -1)), []);
  return { platform, theme, themeMode, setTheme, tab, setTab, stack, push, replace, pop, muteOnStartup, setMuteOnStartup, volume, setVolume, muted, setMuted, theater, setTheater, fs, setFs, clean, setClean, strictSecurity, strictSecurityReady, securityPending, setStrictSecurity, error };
}
const Context = createContext<ReturnType<typeof useApplication>>(null!);
export const useStore = () => useContext(Context);
export function AppProvider({children}: {children: React.ReactNode}) { const value = useApplication(); return <Context.Provider value={value}>{children}</Context.Provider>; }
