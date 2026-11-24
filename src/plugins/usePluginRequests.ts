import { useCallback, useEffect, useRef } from "react";
import { pluginCommand, releaseMedia, type InstalledPlugin } from "./client";
import { readMetadata } from './metadataCache';

/** Own requests for the lifetime of a view, including requests omitting ctx.signal. */
export function usePluginRequests() {
  const mounted = useRef(true), pending = useRef(new Set<string>()), generation = useRef(0);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current++;
      for (const requestId of pending.current) void pluginCommand("cancel", { requestId }).catch(() => {});
    };
  }, []);
  const cancelAll = useCallback(() => {
    generation.current++;
    for (const requestId of pending.current) void pluginCommand("cancel", { requestId }).catch(() => {});
  }, []);
  const request = useCallback(async <T,>(action: string, params: Record<string, unknown> = {}): Promise<T> => {
    if (!mounted.current) throw { code: "CANCELLED", message: "页面已关闭" };
    const requestId = crypto.randomUUID(); pending.current.add(requestId);
    try {
      const result = await pluginCommand<T>(action, { ...params, requestId });
      if (!mounted.current) {
        if (action !== 'media.refresh' && result && typeof result === "object" && "sessionId" in result) releaseMedia(String(result.sessionId));
        const preview = action === "distribution.preview" ? result : action === "updates.check" ? (result as { preview?: unknown })?.preview : null;
        if (preview && typeof preview === "object" && "token" in preview) {
          void pluginCommand("distribution.discard", { token: String(preview.token) }).catch(() => {});
        }
        throw { code: "CANCELLED", message: "页面已关闭" };
      }
      return result;
    } finally { pending.current.delete(requestId); }
  }, []);
  const read = useCallback(async <T,>(plugin: InstalledPlugin, method: string, args: Record<string, unknown>, refresh = false): Promise<T> => {
    const epoch = generation.current;
    if (!mounted.current) throw { code: 'CANCELLED', message: '页面已关闭' };
    const value = await readMetadata<T>(plugin, method, args, refresh);
    if (!mounted.current || generation.current !== epoch) throw { code: 'CANCELLED', message: '请求已取消' };
    return value;
  }, []);
  return Object.assign(request, { cancelAll, read });
}
