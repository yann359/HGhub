import { pluginCommand, type InstalledPlugin } from './client';

// Only provider metadata is cached. Playback URLs and sessions are always resolved live.
const TTL = 5 * 60_000, LIMIT = 128;
const methods = new Set(['media.getDetail', 'media.listEpisodes', 'catalog.getCategories', 'catalog.getFilters', 'catalog.list', 'home.getSections', 'home.listSection']);
type Entry = { expires: number; value?: unknown; pending?: Promise<unknown>; requestId?: string };
const entries = new Map<string, Entry>();
function key(plugin: InstalledPlugin, method: string, args: Record<string, unknown>) {
  return JSON.stringify([plugin.meta.id, plugin.revision, plugin.endpoint?.url, method, args]);
}
function trim() {
  while (entries.size > LIMIT) {
    const id = entries.keys().next().value!, entry = entries.get(id)!;
    if (entry.requestId) void pluginCommand('cancel', { requestId: entry.requestId }).catch(() => {});
    entries.delete(id);
  }
}
export function clearMetadataCache() {
  for (const entry of entries.values()) if (entry.requestId) void pluginCommand('cancel', { requestId: entry.requestId }).catch(() => {});
  entries.clear();
}
export function peekMetadata<T>(plugin: InstalledPlugin, method: string, args: Record<string, unknown>): T | undefined {
  const id = key(plugin, method, args), entry = entries.get(id);
  if (!entry || entry.expires <= Date.now()) { entries.delete(id); return undefined; }
  entries.delete(id); entries.set(id, entry);
  return entry.value as T | undefined;
}
export function storeMetadata(plugin: InstalledPlugin, method: string, args: Record<string, unknown>, value: unknown) {
  const id = key(plugin, method, args);
  entries.delete(id); entries.set(id, { value, expires: Date.now() + TTL }); trim();
}
export async function readMetadata<T>(plugin: InstalledPlugin, method: string, args: Record<string, unknown>, refresh = false, notifyAuth = true): Promise<T> {
  if (!methods.has(method)) throw new Error('This method cannot be cached: ' + method);
  const id = key(plugin, method, args);
  if (!refresh) {
    const value = peekMetadata<T>(plugin, method, args);
    if (value !== undefined) return value;
    const pending = entries.get(id)?.pending;
    if (pending) return pending as Promise<T>;
  }
  const entry: Entry = { expires: Date.now() + TTL, requestId: crypto.randomUUID() };
  // A shared read survives route changes; its result may only update a live consumer.
  entry.pending = pluginCommand<T>('call', { id: plugin.meta.id, method, args, requestId: entry.requestId }, { notifyAuth }).then(value => {
    if (entries.get(id) === entry) { entry.value = value; entry.expires = Date.now() + TTL; delete entry.pending; delete entry.requestId; }
    return value;
  }, error => { if (entries.get(id) === entry) entries.delete(id); throw error; });
  entries.set(id, entry); trim();
  return entry.pending as Promise<T>;
}
