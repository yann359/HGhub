import { useEffect, useRef, useState } from 'react';

/** One pointer gesture settles to one adjacent item, regardless of travel distance. */
export function useVerticalSwipe(enabled: boolean, previous?: () => void, next?: () => void) {
  const [motion, setMotion] = useState({ offset: 0, height: 1, settling: false });
  const timer = useRef<number | undefined>(undefined), locked = useRef(false);
  const latest = useRef({ enabled, previous, next }); latest.current = { enabled, previous, next };
  const cancel = () => { clearTimeout(timer.current); locked.current = false; setMotion(m => ({ ...m, offset: 0, settling: false })); };
  useEffect(() => { if (!enabled) cancel(); }, [enabled]);
  useEffect(() => () => clearTimeout(timer.current), []);
  const move = (distance: number, height: number) => {
    if (!latest.current.enabled || locked.current) return;
    const available = distance < 0 ? latest.current.next : latest.current.previous;
    setMotion({ offset: Math.max(-height, Math.min(height, distance * (available ? 1 : .2))), height, settling: false });
  };
  const finish = (distance: number, elapsed: number, height: number) => {
    if (!latest.current.enabled || locked.current) { cancel(); return; }
    const action = distance < 0 ? latest.current.next : latest.current.previous;
    const commit = !!action && (Math.abs(distance) >= Math.max(60, height * .16) || (Math.abs(distance) >= 32 && Math.abs(distance) / Math.max(1, elapsed) > .55));
    locked.current = true;
    setMotion({ offset: commit ? (distance < 0 ? -height : height) : 0, height, settling: true });
    timer.current = window.setTimeout(() => {
      if (commit && latest.current.enabled) action?.();
      locked.current = false; setMotion({ offset: 0, height, settling: false });
    }, 220);
  };
  return { ...motion, move, finish, cancel, locked };
}
