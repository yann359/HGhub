import type { PreparedMedia } from './client';

type Entry = {
  key: string; requestId: string; result?: PreparedMedia; expires: number;
  cancelled: boolean; claimed: boolean; promise: Promise<PreparedMedia | undefined>;
  params: Record<string, unknown>; warmAbort?: AbortController; warming?: Promise<void>;
  warmAttempts: number; warmAfter: number; warmed: boolean;
};
export interface PreparationTransport {
  prepare: (params: Record<string, unknown>, requestId: string) => Promise<PreparedMedia>;
  cancel: (requestId: string) => void;
  release: (sessionId: string) => void;
  warm: (media: PreparedMedia, params: Record<string, unknown>, signal: AbortSignal) => Promise<boolean>;
  stopWarm: (sessionId: string) => void;
  activate: (sessionId: string) => Promise<unknown>;
}

/** One speculative episode, with ownership transferred (never copied) on selection. */
export class NextEpisode {
  private next?: Entry;
  private owned = new Set<Entry>();
  constructor(private transport: PreparationTransport, private now = Date.now) {}
  offer(key: string, params: Record<string, unknown>, nearEnd = false) {
    if (this.next?.key === key && (!nearEnd || this.next.expires > this.now() + 5000)) {
      this.warm(this.next); return;
    }
    this.clear();
    const entry: Entry = { key, requestId: crypto.randomUUID(), expires: this.now() + 90_000,
      cancelled: false, claimed: false, params, warmAttempts: 0, warmAfter: 0, warmed: false,
      promise: Promise.resolve(undefined) };
    this.next = entry; this.owned.add(entry);
    entry.promise = this.transport.prepare(params, entry.requestId).then(media => {
      if (entry.cancelled) { this.transport.release(media.sessionId); return undefined; }
      entry.result = media;
      entry.expires = Math.min(this.now() + 90_000, media.playback.expiresAt ?? Infinity);
      this.warm(entry);
      return media;
    }, () => undefined); // Speculative failures never open auth/selection UI.
  }
  private warm(entry: Entry) {
    if (!entry.result || entry.cancelled || entry.claimed || entry.warmed || entry.warming ||
      entry.warmAttempts >= 3 || entry.warmAfter > this.now() || entry.expires <= this.now() + 5000) return;
    const abort = new AbortController(); entry.warmAbort = abort; entry.warmAttempts++;
    entry.warming = this.transport.warm(entry.result, entry.params, abort.signal).then(ok => {
      if (!abort.signal.aborted) entry.warmed = ok;
    }, () => {}).finally(() => { entry.warming = undefined; entry.warmAfter = this.now() + 10_000; });
  }
  async take(key: string): Promise<PreparedMedia | undefined> {
    const entry = this.next;
    if (!entry || entry.key !== key) { this.clear(); return undefined; }
    this.next = undefined; entry.claimed = true;
    const result = await entry.promise;
    if (!result || entry.cancelled) { this.owned.delete(entry); return undefined; }
    entry.warmAbort?.abort(); this.transport.stopWarm(result.sessionId);
    if (entry.expires <= this.now() + 5000) { this.owned.delete(entry); this.transport.release(result.sessionId); return undefined; }
    try { await this.transport.activate(result.sessionId); }
    catch { this.owned.delete(entry); this.transport.release(result.sessionId); return undefined; }
    this.owned.delete(entry);
    if (entry.cancelled) { this.transport.release(result.sessionId); return undefined; }
    return result;
  }
  stopWarm() {
    this.next?.warmAbort?.abort();
    if (this.next?.result) this.transport.stopWarm(this.next.result.sessionId);
  }
  clear() {
    this.next = undefined;
    for (const entry of this.owned) {
      entry.cancelled = true;
      entry.warmAbort?.abort();
      this.transport.cancel(entry.requestId);
      if (entry.result) this.transport.release(entry.result.sessionId);
    }
    this.owned.clear();
  }
}
