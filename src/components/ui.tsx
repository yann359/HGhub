import React, { useEffect, useState } from "react";
/* ——— 头像 ——— */
export function Avatar({ hue = 250, icon, name, size = 40, ring = false, className = "" }: {
  hue?: number; icon?: React.ReactNode; name?: string; size?: number; ring?: boolean; className?: string;
}) {
  const inner = (
    <div className="w-full h-full rounded-full flex items-center justify-center overflow-hidden font-semibold"
      style={{ background: `hsl(${hue} 22% 30%)`, color: `hsl(${hue} 70% 88%)` }}>
      {icon ?? <span style={{ fontSize: size * 0.42 }} className="leading-none">{(name || "H").replace(/^@/, "")[0].toUpperCase()}</span>}
    </div>
  );
  return (
    <div className={`shrink-0 ${className}`} style={{ width: size, height: size }}>
      {ring ? <div className="w-full h-full rounded-full p-[2px] bg-primary"><div className="w-full h-full rounded-full bg-bg p-[2px]">{inner}</div></div> : inner}
    </div>
  );
}

/* ——— 底部弹层 ——— */
export function Sheet({ open, onClose, children, z = "z-40" }: { open: boolean; onClose: () => void; children: React.ReactNode; z?: string }) {
  const [render, setRender] = useState(open);
  useEffect(() => {
    if (open) setRender(true);
    else { const t = window.setTimeout(() => setRender(false), 230); return () => window.clearTimeout(t); }
  }, [open]);
  if (!render) return null;
  return (
    <div className={`absolute inset-0 ${z}`}>
      <div className={`absolute inset-0 ${open ? "a-fade" : "opacity-0 transition-opacity duration-200"}`}
        style={{ background: "rgba(0,0,0,.55)" }} onClick={onClose} />
      <div role="dialog" aria-modal="true" aria-label="播放面板" className={`absolute bottom-0 inset-x-0 hg-sheet bg-s2 elev-3 overflow-hidden ${open ? "a-sheet" : "a-sheet-out"}`}
        style={{ paddingBottom: "var(--sab, 0px)", borderTopLeftRadius: "var(--r-lg)", borderTopRightRadius: "var(--r-lg)" }}>
        <div className="pt-3 pb-1 flex justify-center"><div className="w-8 h-1 rounded-full bg-dim/70" /></div>
        {children}
      </div>
    </div>
  );
}

/* ——— 居中弹窗 ——— */
export function Modal({ open, onClose, children, width = 340 }: { open: boolean; onClose: () => void; children: React.ReactNode; width?: number }) {
  const [render, setRender] = useState(open);
  useEffect(() => {
    if (open) setRender(true);
    else { const t = window.setTimeout(() => setRender(false), 200); return () => window.clearTimeout(t); }
  }, [open]);
  if (!render) return null;
  return (
    <div className="absolute inset-0 z-[55] flex items-center justify-center p-6">
      <div className={`absolute inset-0 bg-black/65 backdrop-blur-sm ${open ? "a-fade" : "opacity-0 transition-opacity"}`} onClick={onClose} />
      <div className={`relative bg-s3 rounded-(--r-lg) elev-3 ${open ? "a-scale-in" : "opacity-0 scale-90 transition-all"}`} style={{ width, maxWidth: "100%" }}>
        {children}
      </div>
    </div>
  );
}

/* ——— 开关 ——— */
export function Toggle({ on, onChange, label, disabled = false }: { on: boolean; onChange: (b: boolean) => void; label?: string; disabled?: boolean }) {
  return (
    <button disabled={disabled} onClick={() => onChange(!on)} aria-pressed={on} aria-label={label}
      className={`relative w-[46px] h-[26px] rounded-full transition-colors duration-200 disabled:opacity-50 disabled:cursor-wait ${on ? "bg-primary" : "bg-s4"}`}>
      <span className={`absolute top-[3px] w-5 h-5 rounded-full elev-1 transition-all duration-200 ${on ? "left-[23px] bg-on-primary" : "left-[3px] bg-mut"}`} />
    </button>
  );
}

