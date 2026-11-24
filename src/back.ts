/** 全局返回拦截：面板等临时层优先消费，未消费则走导航栈。 */
type Handler = () => boolean;

let handler: Handler | null = null;
const layers: { handle: Handler; priority: number }[] = [];

export function registerBackHandler(h: Handler, priority = 0) {
  const entry = { handle: h, priority };
  layers.push(entry);
  return () => { const i = layers.indexOf(entry); if (i >= 0) layers.splice(i, 1); };
}

export function setBackHandler(h: Handler | null) {
  handler = h;
}

/** 返回 true 表示已被消费（不再退栈） */
export function runBack(minPriority = -Infinity): boolean {
  try {
    const top = [...layers].reverse().filter(entry => entry.priority >= minPriority).sort((a, b) => b.priority - a.priority)[0];
    return top?.handle() || (minPriority <= 0 && handler ? handler() : false);
  } catch {
    return false;
  }
}
