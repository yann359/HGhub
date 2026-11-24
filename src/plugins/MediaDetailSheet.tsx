import { useEffect, useRef, useState, type ReactNode } from "react";
import { IClose, IPlayF, IStar, IStarF, Logo } from "../icons";
import { registerBackHandler } from "../back";
import type { MediaDetail, MediaItem } from "./protocol";

/** The same decision surface is used by discovery, search and the library. */
export default function MediaDetailSheet({ item, poster, source, favorite, onFavorite, onClose, onPlay, playLabel, busy, error, onRetry, children }: {
  item: MediaDetail | MediaItem | null; poster?: string; source: string; favorite: boolean;
  onFavorite: () => void; onClose: () => void; onPlay: () => void; playLabel: string;
  busy: boolean; error: string; onRetry: () => void; children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null), close = useRef(onClose);
  close.current = onClose;
  const [expanded, setExpanded] = useState(false);
  useEffect(() => {
    const dialog = ref.current!;
    dialog.showModal();
    const unregister = registerBackHandler(() => { close.current(); return true; });
    return () => { unregister(); dialog.close(); };
  }, []);
  return <dialog ref={ref} className="hg-detail-sheet" aria-labelledby="hg-detail-title"
    onCancel={e => { e.preventDefault(); onClose(); }} onClick={e => { if (e.target === e.currentTarget) onClose(); }}>
    <div className="hg-detail-frame">
      <div className="hg-sheet-handle"/>
      <header className="hg-detail-heading"><h2 id="hg-detail-title">作品详情</h2><div>
        <button aria-label={favorite ? "取消本地收藏" : "本地收藏"} aria-pressed={favorite} onClick={onFavorite} disabled={!item}>{favorite ? <IStarF size={23}/> : <IStar size={23}/>}</button>
        <button aria-label="关闭详情" onClick={onClose} autoFocus><IClose size={22}/></button>
      </div></header>
      <div className="hg-detail-scroll">
        <div className="hg-detail-hero"><div className="hg-detail-cover">{poster ? <img src={poster} alt=""/> : <Logo size={36}/>}</div>
          <div><h1>{item?.title || "正在读取作品…"}</h1><p>{[item?.year, item?.episodeCount ? `共 ${item.episodeCount} 集` : item?.badge, source].filter(Boolean).join(" · ")}</p>
            {item?.score !== undefined && <strong className="hg-detail-score">★ {item.score.toFixed(1)} <small>分</small></strong>}
            <div className="hg-detail-tags">{item?.tags?.map(tag => <span key={tag}>{tag}</span>)}</div>
          </div>
        </div>
        <div className="hg-synopsis"><p className={expanded ? "" : "is-clamped"}>{item?.description || "暂无简介"}</p>{!!item?.description && item.description.length > 80 && <button aria-expanded={expanded} onClick={() => setExpanded(v => !v)}>{expanded ? "收起" : "展开简介"}</button>}</div>
        {error && <div role="alert" className="hg-detail-error"><p>{error}</p><button onClick={onRetry}>重新加载</button></div>}
        {children}
      </div>
      <footer><button className="hg-watch-action" onClick={onPlay} disabled={busy}><IPlayF size={19}/>{playLabel}</button></footer>
    </div>
  </dialog>;
}
