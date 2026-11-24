// Host-owned HPP prelude. No site logic; evaluated in every isolated runtime.
(() => {
  "use strict";
  const syncNative = globalThis.__hppSync;
  const sendNative = globalThis.__hppSend;
  const finishNative = globalThis.__hppFinish;
  const scopes = new Map(), pending = new Map(), promiseScopes = new WeakMap(), stack = [];
  let current = null, serial = 0, plugin = null, factoryMode = false;
  const codes = new Set(["AUTH_REQUIRED","AUTH_INVALID","AUTH_EXPIRED","NOT_FOUND","CONTENT_LOCKED","RATE_LIMITED","UPSTREAM_ERROR","UPSTREAM_CHANGED","INTERACTION_REQUIRED","NETWORK","TIMEOUT","CANCELLED","INVALID_ARGUMENT","UNSUPPORTED","INTERNAL","STORAGE_FULL"]);
  const retryableCodes = new Set(["AUTH_REQUIRED","AUTH_EXPIRED","RATE_LIMITED","UPSTREAM_ERROR","INTERACTION_REQUIRED","NETWORK","TIMEOUT","STORAGE_FULL","INTERNAL"]);
  class PluginError extends Error {
    constructor(code, message = "插件出错", options = {}) {
      super(codes.has(code) ? message : "插件出错"); this.name = "PluginError"; this.code = codes.has(code) ? code : "INTERNAL";
      Object.assign(this, { retryable: options.retryable ?? retryableCodes.has(this.code), retryAfterSec: options.retryAfterSec, details: options.details });
    }
  }
  function deepFreeze(value) { if(value && typeof value==='object') { for(const child of Object.values(value))deepFreeze(child);Object.freeze(value); } return value; }
  function logText(args) { return args.map(value=>value instanceof Uint8Array ? 'Uint8Array('+value.length+')' : typeof value==='string' ? value : (()=>{try{return JSON.stringify(value);}catch{return String(value);}})()).join(' '); }
  function pack(value) {
    const seen = new Set();
    function walk(v, depth) {
      if (depth > 32) throw new PluginError("INVALID_ARGUMENT", "数据嵌套超过 32 层");
      if (v === null || v === undefined) return { t: "null" };
      if (typeof v === "boolean") return { t: "bool", v };
      if (typeof v === "string") return { t: "string", v };
      if (typeof v === "number" && Number.isFinite(v)) return { t: "number", v };
      if (v instanceof Uint8Array) return { t: "bytes", v: Array.from(v) };
      if (typeof v !== "object" || seen.has(v)) throw new PluginError("INVALID_ARGUMENT", "数据不可传递");
      seen.add(v);
      let result;
      if (Array.isArray(v)) {
        if (Array.from(v).some(x => x === undefined)) throw new PluginError("INVALID_ARGUMENT", "数组不能含 undefined 或空位");
        result = { t: "array", v: v.map(x => walk(x, depth + 1)) };
      } else {
        if (Object.getPrototypeOf(v) !== Object.prototype && Object.getPrototypeOf(v) !== null) throw new PluginError("INVALID_ARGUMENT", "只允许普通对象");
        result = { t: "object", v: Object.fromEntries(Object.entries(v).filter(([,v]) => v !== undefined).map(([k,v]) => [k,walk(v,depth+1)])) };
      }
      seen.delete(v); return result;
    }
    return walk(value, 0);
  }
  function unpack(v) {
    switch (v.t) {
      case "null": return null;
      case "bool": case "number": case "string": return v.v;
      case "bytes": return new Uint8Array(v.v);
      case "array": return v.v.map(unpack);
      case "object": return Object.fromEntries(Object.entries(v.v).map(([k,v]) => [k,unpack(v)]));
      default: throw new PluginError("INTERNAL", "无效桥接数据");
    }
  }
  function unwrap(s) { const v = JSON.parse(s); if (!v.ok) throw new PluginError(v.error.code,v.error.message,v.error); return unpack(v.value); }
  function sync(op, params) {
    if (current && !scopes.has(current)) throw new PluginError("CANCELLED", "调用已经结束");
    return unwrap(syncNative(op, JSON.stringify(pack(params)), current || ""));
  }
  function scope() {
    if (factoryMode) throw new PluginError("UNSUPPORTED", "工厂执行期间不可使用异步宿主模块");
    const s = scopes.get(current);
    if (!s || s.signal.aborted) throw new PluginError("CANCELLED", "调用已经结束");
    return s;
  }
  function host(op, params, signal) {
    const s = scope(), owner = current, id = String(++serial);
    let cleanup = () => {};
    const promise = new Promise((resolve,reject) => {
      if (signal?.aborted) { reject(new PluginError("CANCELLED", "请求已取消")); return; }
      const abort = () => {
        if (!pending.has(id)) return;
        pending.delete(id); sendNative(JSON.stringify({ kind:"abort", id, scope:owner }));
        cleanup(); reject(new PluginError("CANCELLED", "请求已取消"));
      };
      signal?.addEventListener("abort", abort);
      cleanup = () => signal?.removeEventListener("abort", abort);
      pending.set(id, { resolve,reject,owner,cleanup });
      sendNative(JSON.stringify({ kind:"request", id, scope:owner, op, params:pack(params) }));
    });
    promiseScopes.set(promise, owner); return promise;
  }
  class AbortSignal {
    constructor() { this.aborted=false;this.reason=undefined;this._listeners=new Set();this.onabort=null; }
    addEventListener(type, fn) { if (type === "abort") this._listeners.add(fn); }
    removeEventListener(type, fn) { if (type === "abort") this._listeners.delete(fn); }
    throwIfAborted() { if (this.aborted) throw this.reason; }
    static abort(reason) { const c=new AbortController();c.abort(reason);return c.signal; }
  }
  class AbortController {
    constructor() {this.signal=new AbortSignal();}
    abort(reason=new PluginError("CANCELLED","调用已取消")) {
      const s=this.signal;if(s.aborted)return;s.aborted=true;s.reason=reason;
      for(const fn of [...s._listeners]){try{fn.call(s,{type:"abort",target:s});}catch{}}
      try{s.onabort?.({type:"abort",target:s});}catch{}
    }
  }
  class TextEncoder { get encoding(){return "utf-8";} encode(s=""){return sync("codec.utf8Encode",{text:String(s)});} encodeInto(s,d){const b=this.encode(s);let n=Math.min(b.length,d.length);while(n&&n<b.length&&(b[n]&192)===128)n--;d.set(b.subarray(0,n));return {read:new TextDecoder().decode(b.subarray(0,n)).length,written:n};} }
  class TextDecoder { constructor(label="utf-8"){this.encoding=label.toLowerCase();} decode(data=new Uint8Array()){return sync("codec.decodeText",{data:data instanceof Uint8Array?data:new Uint8Array(data.buffer||data),charset:this.encoding});} }
  class URLSearchParams {
    constructor(value="", changed=()=>{}) {
      this._changed=changed;
      if(value instanceof URLSearchParams)this._pairs=value._pairs.map(x=>[...x]);
      else if(typeof value==="string")this._pairs=sync("url.queryParse",{text:value.replace(/^\?/,"")});
      else if(Array.isArray(value))this._pairs=value.map(([k,v])=>[String(k),String(v)]);
      else this._pairs=Object.entries(value).map(([k,v])=>[k,String(v)]);
    }
    append(k,v){this._pairs.push([String(k),String(v)]);this._changed();}
    set(k,v){k=String(k);const i=this._pairs.findIndex(x=>x[0]===k);this._pairs=this._pairs.filter(x=>x[0]!==k);this._pairs.splice(i<0?this._pairs.length:i,0,[k,String(v)]);this._changed();}
    delete(k,v){this._pairs=this._pairs.filter(x=>x[0]!==String(k)||(v!==undefined&&x[1]!==String(v)));this._changed();}
    get(k){return this._pairs.find(x=>x[0]===String(k))?.[1]??null;}
    getAll(k){return this._pairs.filter(x=>x[0]===String(k)).map(x=>x[1]);}
    has(k,v){return this._pairs.some(x=>x[0]===String(k)&&(v===undefined||x[1]===String(v)));}
    sort(){this._pairs.sort((a,b)=>a[0]<b[0]?-1:a[0]>b[0]?1:0);this._changed();}
    toString(){return sync("url.queryEncode",{pairs:this._pairs});}
    get size(){return this._pairs.length;}
    entries(){return this._pairs.map(x=>[...x])[Symbol.iterator]();} keys(){return this._pairs.map(x=>x[0])[Symbol.iterator]();} values(){return this._pairs.map(x=>x[1])[Symbol.iterator]();}
    [Symbol.iterator](){return this.entries();} forEach(fn,thisArg){for(const[k,v]of this._pairs)fn.call(thisArg,v,k,this);}
  }
  class URL {
    constructor(input,base){this._set(sync("url.parse",{input:String(input),base:base===undefined?undefined:String(base)}));}
    _set(v){this._parts=v;if(this._params)this._params._pairs=new URLSearchParams(v.search)._pairs;}
    get searchParams(){return this._params??=new URLSearchParams(this.search,()=>{this._set(sync("url.set",{url:this.href,key:"search",value:this._params.toString()}));});}
    toString(){return this.href;} toJSON(){return this.href;}
    static canParse(input,base){try{new URL(input,base);return true;}catch{return false;}}
  }
  for(const key of ["href","protocol","username","password","host","hostname","port","pathname","search","hash","origin"]){
    Object.defineProperty(URL.prototype,key,{get(){return this._parts[key];},...(key==="origin"?{}:{set(v){this._set(sync("url.set",{url:this.href,key,value:String(v)}));}})});
  }
  let timers=new Map();
  function setTimeout(fn,ms=0,...args){
    if(typeof fn!=="function")throw new TypeError("setTimeout requires a function");
    scope();if(timers.size>=64)throw new PluginError("RATE_LIMITED","定时器数量超限");
    const id=++serial,owner=current,c=new AbortController();timers.set(id,{owner,c});
    host("sleep",{ms:Math.min(60000,Math.max(0,Number(ms)||0))},c.signal).then(()=>{timers.delete(id);if(scopes.has(owner))fn(...args);},()=>{timers.delete(id);});return id;
  }
  function clearTimeout(id){timers.get(id)?.c.abort();timers.delete(id);}
  function clean(owner){
    for(const[id,t]of timers)if(t.owner===owner)clearTimeout(id);
    for(const[id,p]of pending)if(p.owner===owner){pending.delete(id);p.cleanup();p.reject(new PluginError("CANCELLED","调用已结束"));sendNative(JSON.stringify({kind:"abort",id,scope:owner}));}
    scopes.delete(owner);
  }
  function errorValue(e){return e instanceof PluginError?{code:e.code,message:e.message,retryable:e.retryable,retryAfterSec:e.retryAfterSec,details:e.details}:{code:"INTERNAL",message:"插件执行失败",details:{message:String(e?.stack||e)}};}
  const methods={media:["getDetail","resolvePlayback","listEpisodes","matchUrl"],live:["getGroups","listChannels","resolve","getProgrammes"],articles:["list","getDetail"],resolver:["resolve"],search:["search","getFilters","suggest","trending"],home:["getSections","listSection"],catalog:["getCategories","list","getFilters"],feed:["list"],endpoints:["list"],auth:["getMethods","submit","getProfile","logout"],ui:["getActions","submit"],library:["listMedia","listFolders","listFollowing"],progress:["report"],updates:["check"],comments:["list","send","like"],interact:["like","favorite","follow"]};
  const api={
    promiseHook(kind,p,parent){
      if(kind===0){const owner=current||promiseScopes.get(parent);if(owner)promiseScopes.set(p,owner);}
      else if(kind===1){stack.push(current);current=promiseScopes.get(p)||null;}
      else if(kind===2){current=stack.pop()||null;}
      return current || "";
    },
    async install(factory, config){
      const {meta,settings,endpoint,media,webLogin}=JSON.parse(config), log={};
      for(const level of ["debug","info","warn","error"])log[level]=(...args)=>sync("log",{level,text:args.map(v=>v instanceof Uint8Array?`Uint8Array(${v.length})`:typeof v==="string"?v:(()=>{try{return JSON.stringify(v);}catch{return String(v);}})()).join(" ")});
      const codec={
        base64Encode:(data,options={})=>sync("codec.base64Encode",{data,...options}),base64Decode:text=>sync("codec.base64Decode",{text}),
        hexEncode:data=>sync("codec.hexEncode",{data}),hexDecode:text=>sync("codec.hexDecode",{text}),
        utf8Encode:text=>sync("codec.utf8Encode",{text}),utf8Decode:data=>sync("codec.decodeText",{data,charset:"utf-8"}),decodeText:(data,charset)=>sync("codec.decodeText",{data,charset})
      };
      const crypto={digest:(algorithm,data)=>sync("crypto.digest",{algorithm,data}),hmac:(algorithm,key,data)=>sync("crypto.hmac",{algorithm,key,data}),aesEncrypt:(params,data)=>sync("crypto.aesEncrypt",{...params,data}),aesDecrypt:(params,data)=>sync("crypto.aesDecrypt",{...params,data}),rsaEncrypt:(params,data)=>sync("crypto.rsaEncrypt",{...params,data}),randomBytes:length=>sync("crypto.randomBytes",{length}),randomUUID:()=>sync("crypto.randomUUID",{})};
      const hg={PluginError,env:deepFreeze(configEnv(meta,media,webLogin)),endpoint:Object.freeze({current:deepFreeze(endpoint),url(path){if(!endpoint)throw new PluginError("UNSUPPORTED","当前尚无站点线路");const u=new URL(path,endpoint.url);if(!path.startsWith("/")||path.startsWith("//")||u.origin!==endpoint.url)throw new PluginError("INVALID_ARGUMENT","路径必须位于当前 origin");return u.href;}}),settings:Object.freeze({get:key=>Object.hasOwn(settings,key)?structuredClone(settings[key]):undefined,getAll:()=>structuredClone(settings)}),log,codec,crypto,
        http:{fetch(req){const{signal,...params}=req;return host("http",params,signal);}},
        ...(webLogin ? {webview:Object.freeze({open:params=>host("webview.open",params)})} : {}),
        cookies:Object.fromEntries(["get","set","clear"].map(op=>[op,params=>host("cookies."+op,params||{})])),
        storage:{async get(key){const r=await host("storage.get",{key});return r.found?r.value:undefined;},set:(key,value,options={})=>host("storage.set",{key,value,...options}),delete:key=>host("storage.delete",{key}),keys:(prefix="")=>host("storage.keys",{prefix}),clear:()=>host("storage.clear",{})},
        html:{parse(html){scope();return node(sync("html.parse",{html}));}}
      };
      function node(handle){return Object.freeze({get tag(){return sync("html.node",{handle,op:"tag"});},select:selector=>sync("html.node",{handle,op:"select",selector}).map(node),first(selector){return this.select(selector)[0]??null;},text:()=>sync("html.node",{handle,op:"text"}),ownText:()=>sync("html.node",{handle,op:"ownText"}),attr:name=>sync("html.node",{handle,op:"attr",name}),attrs:()=>sync("html.node",{handle,op:"attrs"}),html:()=>sync("html.node",{handle,op:"html"}),outerHtml:()=>sync("html.node",{handle,op:"outerHtml"}),parent(){const h=sync("html.node",{handle,op:"parent"});return h===null?null:node(h);},children:()=>sync("html.node",{handle,op:"children"}).map(node)});}
      factoryMode=true;try{plugin=await factory(hg);}finally{factoryMode=false;}
      if(!plugin||typeof plugin!=="object")throw new PluginError("INTERNAL","工厂必须返回插件对象");
      const available=[];
      for(const [space,names] of Object.entries({media:["getDetail","resolvePlayback"],live:["getGroups","listChannels","resolve"],articles:methods.articles,resolver:["resolve"],search:["search"],home:["getSections"],catalog:["getCategories","list"],feed:["list"],endpoints:["list"],auth:methods.auth,ui:methods.ui,library:["listMedia"],progress:["report"],updates:["check"],comments:["list"],interact:[]})) {
        if(plugin[space]!==undefined && (!plugin[space] || typeof plugin[space]!=="object" || names.some(name=>typeof plugin[space][name]!=="function")))
          throw new PluginError("INTERNAL",`${space} 缺少必需方法`);
      }
      for(const[space,names]of Object.entries(methods))for(const name of names)if(typeof plugin[space]?.[name]==="function")available.push(`${space}.${name}`);
      if(typeof plugin.transformResource==="function")available.push("transformResource");
      const required=meta.kind==="resolver"?["resolver.resolve"]:available.some(x=>x.startsWith("media."))?["media.getDetail","media.resolvePlayback"]:[];
      if(meta.kind!=="resolver"&&!required.length&&!available.includes("live.resolve"))throw new PluginError("INTERNAL","数据源至少实现 media 或 live");
      if(meta.kind!=="resolver"&&!available.includes("media.getDetail")&&["home.getSections","catalog.list","search.search","feed.list","library.listMedia"].some(m=>available.includes(m)))throw new PluginError("INTERNAL","返回点播条目的能力要求同时实现 media");
      if(required.some(x=>!available.includes(x)))throw new PluginError("INTERNAL","插件缺少必需方法");
      return JSON.stringify(available);
    },
    start(id,method,params,deadline){
      const before=current;current=id;const controller=new AbortController();const state={signal:controller.signal,controller,deadline};scopes.set(id,state);
      const ctx=Object.freeze({requestId:id,get deadline(){return state.deadline;},signal:controller.signal});
      Promise.resolve().then(()=>{
        const parts=method.split("."),target=parts.length===1?plugin:plugin[parts[0]],fn=target?.[parts.at(-1)];
        if(typeof fn!=="function")throw new PluginError("UNSUPPORTED","未实现该能力");
        return fn.call(target,unpack(JSON.parse(params)),ctx);
      }).then(value=>{
        try { finishNative(id,JSON.stringify({ok:true,value:pack(value)})); }
        catch(e) { finishNative(id,JSON.stringify({ok:false,error:{code:'INTERNAL',message:'插件返回了不可传递的数据'}})); }
        finally { clean(id); }
      },e=>{try{finishNative(id,JSON.stringify({ok:false,error:errorValue(e)}));}catch{finishNative(id,JSON.stringify({ok:false,error:{code:'INTERNAL',message:'插件执行失败'}}));}finally{clean(id);}});
      current=before;
    },
    setDeadline(id, value) {
      const state = scopes.get(id);
      if (state) state.deadline = value;
    },
    deliver(id,response){const p=pending.get(id);if(!p)return;pending.delete(id);p.cleanup();if(!scopes.has(p.owner))return;const before=current;current=p.owner;try{p.resolve(unwrap(response));}catch(e){p.reject(e);}finally{current=before;}},
    cancel(id){const s=scopes.get(id);if(s){s.controller.abort();clean(id);}},
    dispose(){for(const id of [...scopes.keys()])clean(id);current=null;if(typeof plugin?.dispose==="function")plugin.dispose();},
    pack,unpack
  };
  function configEnv(meta,media,webLogin){return {protocol:"1.0.0-dev",app:{name:"HGhub",version:"0.0.1"},plugin:{id:meta.id,version:meta.version},platform:sync("env",{}).platform,locale:"zh-CN",features:webLogin?["charset.legacy","webview"]:["charset.legacy"],media};}
  function structuredClone(v){return unpack(pack(v));}
  Object.assign(globalThis,{AbortSignal,AbortController,TextEncoder,TextDecoder,URL,URLSearchParams,setTimeout,clearTimeout,structuredClone,
    queueMicrotask:fn=>Promise.resolve().then(fn),
    atob:s=>{const bytes=sync("codec.base64Decode",{text:String(s)});let out="";for(let i=0;i<bytes.length;i+=8192)out+=String.fromCharCode.apply(null,bytes.subarray(i,i+8192));return out;},
    btoa:s=>{s=String(s);if([...s].some(c=>c.charCodeAt(0)>255))throw new TypeError("Invalid character");return sync("codec.base64Encode",{data:new Uint8Array([...s].map(c=>c.charCodeAt(0)))});},
    crypto:{getRandomValues(array){if(!ArrayBuffer.isView(array)||array instanceof DataView||array instanceof Float32Array||array instanceof Float64Array||array.byteLength>65536)throw new TypeError("Expected integer typed array, at most 65536 bytes");new Uint8Array(array.buffer,array.byteOffset,array.byteLength).set(sync("crypto.randomBytes",{length:array.byteLength}));return array;},randomUUID:()=>sync("crypto.randomUUID",{})},
    console:Object.fromEntries(["debug","log","info","warn","error"].map(level=>[level,(...args)=>sync("log",{level:level==='log'?'info':level,text:logText(args)})]))});
  return api;
})();
