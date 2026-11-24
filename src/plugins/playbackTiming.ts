type Timing = { sessionId: string; phase: string; at: number; elapsedMs?: number };
const timings: Timing[] = [];
/** Local bounded diagnostics; opaque session IDs only, never URLs or credentials. */
export function playbackTiming(sessionId: string, phase: string, started?: number) {
  const at = performance.now();
  const value = { sessionId, phase, at, ...(started === undefined ? {} : { elapsedMs: Math.round(at - started) }) };
  timings.push(value);
  if (timings.length > 200) timings.shift();
  if (import.meta.env.DEV) console.debug('[playback]', value);
}
export function readPlaybackTimings() { return timings.slice(); }
