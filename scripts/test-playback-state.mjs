import assert from 'node:assert/strict';
import { build } from 'esbuild';

// Exercise the production surface's asynchronous callbacks with controllable media events.
// This is a component logic test, not browser decoding or visual acceptance.
const bundle = await build({ entryPoints: ['src/components/VideoSurface.tsx'], bundle: true, write: false,
  format: 'esm', platform: 'node', plugins: [{ name: 'media-fixture', setup(build) {
    build.onResolve({ filter: /^(react|hls\.js)|\/plugins\/(client|playbackTiming)$/ }, args => ({ path: args.path, namespace: 'fixture' }));
    build.onLoad({ filter: /.*/, namespace: 'fixture' }, ({ path }) => ({ contents:
      path === 'react' ? `export const forwardRef=f=>f;export const useRef=v=>globalThis.fixture.useRef(v);export const useEffect=(f,d)=>globalThis.fixture.useEffect(f,d);export const useImperativeHandle=(ref,f)=>{ref.current=f()};` :
      path === 'react/jsx-runtime' ? `export const jsx=(type,props)=>({type,props});export const jsxs=jsx;` :
      path.includes('worker') ? `export default 'worker.js';` :
      path === 'hls.js' ? `export default class Hls{static isSupported(){return false}}` :
      path.endsWith('client') ? `export const pluginCommand=async()=>{};` : `export const playbackTiming=()=>{};` }));
  } }] });
const { default: Surface } = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString('base64')}`);
const flush = async () => { await Promise.resolve(); await Promise.resolve(); };
const deferred = () => { let resolve, reject; const promise = new Promise((a,b) => { resolve=a; reject=b; }); return { promise, resolve, reject }; };
globalThis.document = Object.assign(new EventTarget(), { hidden: false });
globalThis.window = Object.assign(new EventTarget(), { setTimeout, clearTimeout });
let callbacks, tree, cursor, effects, refs, queued, events, props;
class Video extends EventTarget {
  paused=true; readyState=0; currentTime=0; duration=100; ended=false; requests=[];
  play() { this.paused=false; this.fire('play'); const d=deferred(); this.requests.push(d); return d.promise; }
  pause() { if(!this.paused){this.paused=true;this.fire('pause');} }
  removeAttribute() {} load() { this.paused=true;this.readyState=0; }
  fire(name) { this.dispatchEvent(new Event(name)); const prop={play:'onPlay',waiting:'onWaiting',seeking:'onSeeking',seeked:'onSeeked'}[name]; if(prop)callbacks[prop]?.(); }
}
let video, handle;
const cleanup = () => { for(const e of effects.values())e.cleanup?.(); };
function setup() {
  video=new Video(); handle={};refs=[];effects=new Map();events=[];
  globalThis.fixture={
    useRef(value){const i=cursor++;return refs[i]??=( {current:i===0?video:value} );},
    useEffect(run,deps){const i=cursor++, old=effects.get(i);if(!old||deps.some((v,n)=>v!==old.deps[n]))queued.push(()=>{old?.cleanup?.();effects.set(i,{deps,cleanup:run()});});},
  };
  props={media:{sessionId:'a',version:0,url:'fixture',format:'progressive',playback:{}},position:0,active:true,autoPlay:true,
    muted:true,volume:1,rate:1,fit:'contain',scale:1,panX:0,panY:0,isGesturing:false,
    onPlaying:()=>events.push('playing'),onPause:()=>events.push('paused'),onWaiting:()=>events.push('loading'),onEnded:()=>events.push('ended'),onError:()=>events.push('error'),
    onTime(){},onDuration(){},onSize(){},onBuffered(){},onClick(){}};
  render();
}
function render(update={}) {
  props={...props,...update};cursor=0;queued=[];tree=Surface(props,handle);callbacks=tree.props.children.props;
  for(const effect of queued)effect();
}

setup(); handle.current.play(); const old=video.requests.at(-1);
render({media:{...props.media,sessionId:'b'}});handle.current.play();
old.reject(new DOMException('old source','NotAllowedError'));await flush();
assert.equal(events.at(-1),'loading','a previous session cannot mark the new session paused');
video.readyState=4;video.fire('playing');assert.equal(events.at(-1),'playing');cleanup();

setup();handle.current.play();const beforeRefresh=video.requests.at(-1);
render({media:{...props.media,version:1,url:'refreshed'},position:42});
assert.equal(video.src,'refreshed','same-session refresh must reload the new resource version');
video.fire('loadedmetadata');assert.equal(video.currentTime,42,'refresh preserves current position');
beforeRefresh.reject(new DOMException('old version','NotAllowedError'));await flush();
assert.equal(events.at(-1),'loading','old-version play rejection cannot change refreshed state');cleanup();

setup();handle.current.play();const first=video.requests.at(-1);handle.current.play();
first.reject(new DOMException('superseded','NotAllowedError'));await flush();
assert.equal(events.at(-1),'loading','superseded play failures cannot override newer attempts');
video.requests.at(-1).reject(new DOMException('interrupted','AbortError'));await flush();
assert.equal(events.at(-1),'loading','load interruptions are not explicit pauses');cleanup();

setup();handle.current.play();video.requests.at(-1).reject(new DOMException('gesture required','NotAllowedError'));await flush();
assert.equal(events.at(-1),'paused','a real autoplay denial must expose the play action');cleanup();

setup();handle.current.play();const pending=video.requests.at(-1);handle.current.pause();
assert.equal(events.at(-1),'paused');video.fire('seeking');video.fire('seeked');
assert.equal(events.at(-1),'paused','seeking while explicitly paused must not show buffering');
video.fire('play');assert.equal(events.at(-1),'paused','a queued play event cannot undo a newer explicit pause');
const requests=video.requests.length;video.fire('loadedmetadata');assert.equal(video.requests.length,requests,'metadata must not undo an explicit pause');
pending.reject(new DOMException('pause','AbortError'));await flush();assert.equal(events.at(-1),'paused');cleanup();

setup();handle.current.play();video.readyState=4;video.fire('playing');video.currentTime=video.duration;video.fire('timeupdate');
assert.equal(events.filter(event=>event==='ended').length,1,'terminal timeupdate synthesizes a missing end');video.fire('ended');assert.equal(events.filter(event=>event==='ended').length,1,'native ended is deduplicated after the fallback');cleanup();

setup();handle.current.play();const disposed=video.requests.at(-1);cleanup();const before=events.length;
disposed.reject(new DOMException('gone','NotAllowedError'));await flush();assert.equal(events.length,before);
console.log('Playback state: stale/superseded play rejection, buffering cancellation, explicit pause/seek, autoplay denial, end fallback/deduplication and disposal passed.');
