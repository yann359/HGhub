use crate::{
    model::Meta,
    native::{Logs, Native},
    network::Network,
    store::{now, Store},
    web_login::{LoginCompletion, LoginRequest, WebLoginProvider},
    wire::Transfer,
    PluginError, Result,
};
use rquickjs::{
    promise::PromiseHookType, Context, Function, Module, Object, Persistent, Runtime,
    Value as JsValue,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

const PRELUDE: &str = include_str!("prelude.js");
type Tokens = Arc<Mutex<HashMap<String, CancellationToken>>>;
struct Engine {
    api: Persistent<Object<'static>>,
    factory: RefCell<Option<Persistent<Function<'static>>>>,
    context: Context,
    runtime: Runtime,
    native: Rc<RefCell<Native>>,
    outgoing: Rc<RefCell<Vec<Value>>>,
    finished: Rc<RefCell<Vec<(String, String)>>>,
    slice: Rc<RefCell<Instant>>,
    initialization_deadline: Rc<RefCell<Option<Instant>>>,
    shutdown: Arc<AtomicBool>,
    initialized: Cell<bool>,
    disposing: Rc<Cell<bool>>,
    owner: Rc<RefCell<String>>,
    interrupted: Arc<AtomicBool>,
}
impl Engine {
    fn new(logs: Logs, tokens: Tokens, shutdown: Arc<AtomicBool>) -> Result<Self> {
        let runtime = Runtime::new().map_err(|e| PluginError::internal(e.to_string()))?;
        runtime.set_memory_limit(64 * 1024 * 1024);
        runtime.set_max_stack_size(1024 * 1024);
        let context = Context::full(&runtime).map_err(|e| PluginError::internal(e.to_string()))?;
        let native = Rc::new(RefCell::new(Native::new(logs)));
        let outgoing = Rc::new(RefCell::new(Vec::new()));
        let finished = Rc::new(RefCell::new(Vec::new()));
        let slice = Rc::new(RefCell::new(Instant::now()));
        let initialization_deadline = Rc::new(RefCell::new(None::<Instant>));
        let disposing = Rc::new(Cell::new(false));
        let owner = Rc::new(RefCell::new(String::new()));
        let interrupted = Arc::new(AtomicBool::new(false));
        {
            let slice = slice.clone();
            let initialization_deadline = initialization_deadline.clone();
            let shutdown = shutdown.clone();
            let disposing = disposing.clone();
            let owner = owner.clone();
            let interrupted = interrupted.clone();
            runtime.set_interrupt_handler(Some(Box::new(move || {
                let stop = slice.borrow().elapsed() > Duration::from_secs(1)
                    || !disposing.get()
                        && (shutdown.load(Ordering::Relaxed)
                            || initialization_deadline
                                .borrow()
                                .is_some_and(|deadline| Instant::now() >= deadline)
                            || tokens
                                .lock()
                                .unwrap()
                                .get(&*owner.borrow())
                                .is_some_and(CancellationToken::is_cancelled));
                if stop {
                    interrupted.store(true, Ordering::Relaxed);
                }
                stop
            })));
        }
        let api = context
            .with(|ctx| -> rquickjs::Result<Persistent<Object<'static>>> {
                let n = native.clone();
                ctx.globals().set(
                    "__hppSync",
                    Function::new(
                        ctx.clone(),
                        move |op: String, raw: String, scope: String| -> String {
                            let result = serde_json::from_str::<Transfer>(&raw)
                                .map_err(|_| PluginError::invalid("宿主桥接参数无效"))
                                .and_then(|p| {
                                    p.validate(8 * 1024 * 1024)?;
                                    n.borrow_mut().sync(&op, p, &scope)
                                });
                            response(result)
                        },
                    )?,
                )?;
                let q = outgoing.clone();
                ctx.globals().set(
                    "__hppSend",
                    Function::new(ctx.clone(), move |s: String| {
                        if let Ok(v) = serde_json::from_str(&s) {
                            q.borrow_mut().push(v);
                        }
                    })?,
                )?;
                let q = finished.clone();
                ctx.globals().set(
                    "__hppFinish",
                    Function::new(ctx.clone(), move |id: String, s: String| {
                        q.borrow_mut().push((id, s));
                    })?,
                )?;
                let api: Object = ctx.eval(PRELUDE)?;
                for name in ["__hppSync", "__hppSend", "__hppFinish"] {
                    ctx.globals().remove(name)?;
                }
                Ok(Persistent::save(&ctx, api))
            })
            .map_err(|e| PluginError::internal(format!("宿主前导脚本加载失败：{e}")))?;
        let active = owner.clone();
        let hook_interrupted = interrupted.clone();
        let hook_api = api.clone();
        runtime.set_promise_hook(Some(Box::new(move |ctx, kind, p, parent| {
            let k = match kind {
                PromiseHookType::Init => 0,
                PromiseHookType::Before => 1,
                PromiseHookType::After => 2,
                PromiseHookType::Resolve => 3,
            };
            let result = (|| -> rquickjs::Result<String> {
                let api = hook_api.clone().restore(&ctx)?;
                let f: Function = api.get("promiseHook")?;
                f.call((k, p, parent))
            })();
            if let Ok(owner) = result {
                if !hook_interrupted.load(Ordering::Relaxed) {
                    *active.borrow_mut() = owner;
                }
            } else {
                ctx.catch();
            }
        })));
        Ok(Self {
            api,
            factory: RefCell::new(None),
            context,
            runtime,
            native,
            outgoing,
            finished,
            slice,
            initialization_deadline,
            shutdown,
            initialized: Cell::new(false),
            disposing,
            owner,
            interrupted,
        })
    }
    fn error(&self, e: rquickjs::Error) -> PluginError {
        if !self.disposing.get() && self.shutdown.load(Ordering::Relaxed) {
            return PluginError::cancelled();
        }
        if self.interrupted.load(Ordering::Relaxed) {
            return PluginError::timeout();
        }
        self.context.with(|ctx| {
            let detail = if e.is_exception() {
                let v = ctx.catch();
                format!("{v:?}")
            } else {
                e.to_string()
            };
            let mut err = PluginError::internal("插件执行失败");
            err.details = Some(json!({"engine":detail}));
            err
        })
    }
    fn inspect(&self, source: &str) -> Result<Meta> {
        let deadline = Instant::now() + Duration::from_secs(5);
        *self.initialization_deadline.borrow_mut() = Some(deadline);
        *self.slice.borrow_mut() = Instant::now();
        let result = (|| {
            let (namespace, promise) = self
                .context
                .with(|ctx| -> rquickjs::Result<_> {
                    let (module, promise) =
                        Module::declare(ctx.clone(), "plugin", source)?.eval()?;
                    Ok((
                        Persistent::save(&ctx, module.namespace()?),
                        Persistent::save(&ctx, promise),
                    ))
                })
                .map_err(|e| self.error(e))?;
            self.wait_initialization::<()>(promise, deadline)?;
            *self.slice.borrow_mut() = Instant::now();
            let raw = self
                .context
                .with(|ctx| -> rquickjs::Result<Value> {
                    let ns = namespace.restore(&ctx)?;
                    let keys = ns.keys::<String>().collect::<rquickjs::Result<Vec<_>>>()?;
                    if keys.len() != 2
                        || !keys.iter().any(|k| k == "meta")
                        || !keys.iter().any(|k| k == "default")
                    {
                        return Err(rquickjs::Error::new_from_js_message(
                            "module",
                            "HPP",
                            "只允许 meta 与 default 两个导出",
                        ));
                    }
                    let factory: Function = ns.get("default")?;
                    let meta: JsValue = ns.get("meta")?;
                    let raw = ctx
                        .json_stringify(meta)?
                        .ok_or(rquickjs::Error::Unknown)?
                        .to_string()?;
                    *self.factory.borrow_mut() = Some(Persistent::save(&ctx, factory));
                    Ok(serde_json::from_str(&raw).unwrap_or(Value::Null))
                })
                .map_err(|e| self.error(e))?;
            let meta = Meta::parse(raw)?;
            if Instant::now() >= deadline {
                return Err(PluginError::timeout());
            }
            Ok(meta)
        })();
        *self.initialization_deadline.borrow_mut() = None;
        result
    }
    fn wait_initialization<T: for<'js> rquickjs::FromJs<'js>>(
        &self,
        promise: Persistent<rquickjs::Promise<'static>>,
        deadline: Instant,
    ) -> Result<T> {
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                return Err(PluginError::cancelled());
            }
            if self.interrupted.load(Ordering::Relaxed) || Instant::now() >= deadline {
                return Err(PluginError::timeout());
            }
            let value = self
                .context
                .with(|ctx| promise.clone().restore(&ctx)?.result::<T>().transpose())
                .map_err(|e| self.error(e))?;
            if let Some(value) = value {
                return Ok(value);
            }
            self.pump().map_err(|error| {
                if self.shutdown.load(Ordering::Relaxed) {
                    PluginError::cancelled()
                } else {
                    error
                }
            })?;
            if !self.runtime.is_job_pending() {
                // Initialization cannot perform I/O or use timers. Pending pure
                // Promises wait for the deadline without spinning the thread.
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
    fn install(
        &self,
        meta: &Meta,
        settings: &Value,
        endpoint: &Value,
        media: &crate::capabilities::MediaCapabilities,
        web_login: bool,
    ) -> Result<Vec<String>> {
        let deadline = Instant::now() + Duration::from_secs(5);
        *self.initialization_deadline.borrow_mut() = Some(deadline);
        *self.slice.borrow_mut() = Instant::now();
        let result = (|| {
            let promise = self.context
            .with(|ctx| -> rquickjs::Result<Persistent<rquickjs::Promise<'static>>> {
                let api = self.api.clone().restore(&ctx)?;
                let install: Function = api.get("install")?;
                let factory = self
                    .factory
                    .borrow()
                    .clone()
                    .ok_or(rquickjs::Error::Unknown)?
                    .restore(&ctx)?;
                let p: rquickjs::Promise = install.call((
                    factory,
                    json!({"meta":meta,"settings":settings,"endpoint":endpoint,"media":media,"webLogin":web_login})
                        .to_string(),
                ))?;
                Ok(Persistent::save(&ctx, p))
            })
            .map_err(|e| self.error(e))?;
            let value = self.wait_initialization::<String>(promise, deadline)?;
            serde_json::from_str(&value).map_err(|_| PluginError::internal("能力发现失败"))
        })();
        *self.initialization_deadline.borrow_mut() = None;
        self.initialized.set(result.is_ok());
        result
    }
    fn invoke(
        &self,
        name: &str,
        args: impl for<'js> rquickjs::function::IntoArgs<'js>,
    ) -> Result<()> {
        *self.slice.borrow_mut() = Instant::now();
        self.context
            .with(|ctx| {
                let api = self.api.clone().restore(&ctx)?;
                let f: Function = api.get(name)?;
                f.call::<_, ()>(args)
            })
            .map_err(|e| self.error(e))
    }
    fn pump(&self) -> Result<()> {
        for _ in 0..256 {
            if !self.runtime.is_job_pending() {
                break;
            }
            *self.slice.borrow_mut() = Instant::now();
            self.runtime.execute_pending_job().map_err(|e| {
                self.initialized.set(false);
                e.0.with(|ctx| {
                    let ex = ctx.catch();
                    let mut error = if self.interrupted.load(Ordering::Relaxed) {
                        PluginError::timeout()
                    } else {
                        PluginError::internal("插件异步任务失败")
                    };
                    error.details = Some(json!({"engine":format!("{ex:?}")}));
                    error
                })
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod initialization_tests;
#[cfg(test)]
mod lifecycle_tests;
impl Drop for Engine {
    fn drop(&mut self) {
        if self.initialized.get() && !self.interrupted.load(Ordering::Relaxed) {
            self.disposing.set(true);
            if let Err(error) = self.invoke("dispose", ()) {
                self.native.borrow().logs.lock().unwrap().push(
                    "error",
                    "插件卸载通知失败",
                    "",
                    Some(&json!({"code":error.code,"details":error.details})),
                );
            }
        }
        // Release the hook's persistent JS reference before the runtime is dropped.
        self.runtime.set_promise_hook(None);
    }
}
fn response(r: Result<Transfer>) -> String {
    match r {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error}),
    }
    .to_string()
}

pub fn inspect(source: &str) -> Result<Meta> {
    if source.len() > 2 * 1024 * 1024 {
        return Err(PluginError::invalid("插件文件超过 2 MiB"));
    }
    let e = Engine::new(Default::default(), Default::default(), Default::default())?;
    e.inspect(source)
}
struct Call {
    id: String,
    method: String,
    params: Transfer,
    deadline: Instant,
    user_auth: bool,
    web_wait: Option<Instant>,
    cancel: CancellationToken,
    reply: oneshot::Sender<Result<Transfer>>,
    _permit: OwnedSemaphorePermit,
}
enum HostReply {
    Operation(Result<Transfer>),
    Login(Arc<LoginRequest>, Result<LoginCompletion>),
}
enum Command {
    Call(Call),
}
#[derive(Clone)]
pub struct Worker {
    sender: mpsc::Sender<Command>,
    tokens: Tokens,
    shutdown: Arc<AtomicBool>,
    slots: Arc<Semaphore>,
    fault: Arc<Mutex<Option<PluginError>>>,
    pub methods: Vec<String>,
    pub media: crate::capabilities::MediaCapabilities,
}
impl Worker {
    pub async fn spawn(
        source: String,
        meta: Meta,
        settings: Value,
        endpoint: Value,
        store: Arc<Store>,
        network: Arc<Network>,
        logs: Logs,
        media: crate::capabilities::MediaCapabilities,
        web_login: Option<Arc<dyn WebLoginProvider>>,
    ) -> Result<Self> {
        let (sender, mut receiver) = mpsc::channel::<Command>(40);
        let (ready_tx, ready_rx) = oneshot::channel();
        let tokens: Tokens = Default::default();
        let shutdown = Arc::new(AtomicBool::new(false));
        struct StopInitialization(Option<Arc<AtomicBool>>);
        impl Drop for StopInitialization {
            fn drop(&mut self) {
                if let Some(stop) = &self.0 {
                    stop.store(true, Ordering::Relaxed);
                }
            }
        }
        let mut initialization = StopInitialization(Some(shutdown.clone()));
        let token_map = tokens.clone();
        let stop = shutdown.clone();
        let fault = Arc::new(Mutex::new(None));
        let thread_fault = fault.clone();
        let thread_name = format!("hpp:{}", meta.id);
        let thread_media = media.clone();
        let run_worker = move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = ready_tx.send(Err(PluginError::internal(e.to_string())));
                    return;
                }
            };
            let run = async move {
                let make = || {
                    let e = Engine::new(logs.clone(), token_map.clone(), stop.clone())?;
                    e.inspect(&source)?;
                    let methods = e.install(
                        &meta,
                        &settings,
                        &endpoint,
                        &thread_media,
                        web_login.is_some(),
                    )?;
                    Ok::<_, PluginError>((e, methods))
                };
                let (mut engine, methods) = match make() {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(methods));
                let mut calls: HashMap<String, Call> = HashMap::new();
                let mut queue: VecDeque<Call> = VecDeque::new();
                let mut tasks: HashMap<String, (String, CancellationToken)> = HashMap::new();
                let (done_tx, mut done_rx) = mpsc::unbounded_channel::<(u64, String, HostReply)>();
                let mut generation = 0u64;
                let mut failures = 0;
                let mut idle = Instant::now();
                let mut ticker = tokio::time::interval(Duration::from_millis(2));
                loop {
                    tokio::select! {
                        command=receiver.recv()=>match command{Some(Command::Call(c))=>{queue.push_back(c);idle=Instant::now();},None=>break},
                        Some((epoch,id,result))=done_rx.recv()=>{
                            if epoch == generation {
                                if let Some((owner, token)) = tasks.remove(&id) {
                                    if let Some(call) = calls.get_mut(&owner) {
                                        if !token.is_cancelled() && !stop.load(Ordering::Relaxed) {
                                            let result = match result {
                                                HostReply::Operation(result) => result,
                                                HostReply::Login(request, result) => {
                                                    if let Some(started) = call.web_wait.take() {
                                                        call.deadline += started.elapsed();
                                                        let unix = now() + call.deadline.saturating_duration_since(Instant::now()).as_millis() as u64;
                                                        let _ = engine.invoke("setDeadline", (owner, unix));
                                                    }
                                                    result.and_then(|result| request.commit(result, &network))
                                                        .map(|url| Transfer::from_json(json!({"finalUrl":url})))
                                                }
                                            };
                                            let _ = engine.invoke("deliver", (id, response(result)));
                                        }
                                    }
                                }
                            }
                        },
                        _=ticker.tick()=>{}
                    }
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let expired = calls
                        .iter()
                        .filter(|(_, c)| {
                            c.cancel.is_cancelled()
                                || c.web_wait.is_none() && Instant::now() >= c.deadline
                        })
                        .map(|(id, _)| id.clone())
                        .collect::<Vec<_>>();
                    for id in expired {
                        if let Some(c) = calls.remove(&id) {
                            let e = if c.cancel.is_cancelled() {
                                PluginError::cancelled()
                            } else {
                                PluginError::timeout()
                            };
                            c.cancel.cancel();
                            let _ = engine.invoke("cancel", (id.clone(),));
                            engine.native.borrow_mut().clear(&id);
                            token_map.lock().unwrap().remove(&id);
                            let _ = c.reply.send(Err(e));
                        }
                    }
                    while calls.len() < 8 {
                        let Some(c) = queue.pop_front() else {
                            break;
                        };
                        if c.cancel.is_cancelled() || Instant::now() >= c.deadline {
                            token_map.lock().unwrap().remove(&c.id);
                            let e = if c.cancel.is_cancelled() {
                                PluginError::cancelled()
                            } else {
                                PluginError::timeout()
                            };
                            let _ = c.reply.send(Err(e));
                            continue;
                        }
                        let id = c.id.clone();
                        let result = engine.invoke(
                            "start",
                            (
                                id.clone(),
                                c.method.clone(),
                                serde_json::to_string(&c.params).unwrap(),
                                now()
                                    + c.deadline
                                        .saturating_duration_since(Instant::now())
                                        .as_millis() as u64,
                            ),
                        );
                        if let Err(e) = result {
                            token_map.lock().unwrap().remove(&id);
                            let _ = c.reply.send(Err(e));
                        } else {
                            calls.insert(id, c);
                        }
                    }
                    let pump = engine.pump();
                    if pump.is_err() || engine.interrupted.load(Ordering::Relaxed) {
                        let owner = engine.owner.borrow().clone();
                        let reason = pump.err().unwrap_or_else(PluginError::timeout);
                        for (id, c) in calls.drain() {
                            let e = if id == owner {
                                if c.cancel.is_cancelled() {
                                    PluginError::cancelled()
                                } else {
                                    reason.clone()
                                }
                            } else {
                                PluginError::internal("插件运行时已重建")
                            };
                            c.cancel.cancel();
                            token_map.lock().unwrap().remove(&id);
                            let _ = c.reply.send(Err(e));
                        }
                        for (_, (_, t)) in tasks.drain() {
                            t.cancel();
                        }
                        generation += 1;
                        failures += 1;
                        if failures >= 3 {
                            let error = PluginError::internal(
                                "运行时连续故障，已停止；请重试或重新启用插件",
                            );
                            logs.lock().unwrap().push(
                                "error",
                                &error.message,
                                "",
                                error.details.as_ref(),
                            );
                            *thread_fault.lock().unwrap() = Some(error);
                            break;
                        }
                        drop(engine);
                        engine = match make() {
                            Ok((e, _)) => e,
                            Err(e) => {
                                *thread_fault.lock().unwrap() = Some(e);
                                break;
                            }
                        };
                        continue;
                    }
                    let finished = std::mem::take(&mut *engine.finished.borrow_mut());
                    for (id, raw) in finished {
                        if let Some(c) = calls.remove(&id) {
                            c.cancel.cancel();
                            engine.native.borrow_mut().clear(&id);
                            token_map.lock().unwrap().remove(&id);
                            let result = (|| {
                                let v: Value = serde_json::from_str(&raw)
                                    .map_err(|_| PluginError::internal("无效插件结果"))?;
                                if v["ok"] == true {
                                    let value: Transfer =
                                        serde_json::from_value(v["value"].clone())
                                            .map_err(|_| PluginError::internal("无效插件数据"))?;
                                    value.validate(8 * 1024 * 1024)?;
                                    Ok(value)
                                } else {
                                    Err(serde_json::from_value(v["error"].clone())
                                        .unwrap_or_else(|_| PluginError::internal("插件执行失败")))
                                }
                            })();
                            if result.is_ok() {
                                failures = 0;
                            } else if let Err(e) = &result {
                                logs.lock().unwrap().push(
                                    "error",
                                    &e.message,
                                    &id,
                                    e.details.as_ref(),
                                );
                            }
                            let _ = c.reply.send(result);
                        }
                    }
                    let messages = std::mem::take(&mut *engine.outgoing.borrow_mut());
                    for msg in messages {
                        let id = msg["id"].as_str().unwrap_or("").to_string();
                        let scope = msg["scope"].as_str().unwrap_or("").to_string();
                        if msg["kind"] == "abort" {
                            if let Some((_, t)) = tasks.remove(&id) {
                                t.cancel();
                            }
                            continue;
                        }
                        if msg["op"] == "webview.open" {
                            let open = (|| {
                                let provider = web_login
                                    .clone()
                                    .filter(|_| {
                                        calls.get(&scope).is_some_and(|c| {
                                            c.user_auth && !c.cancel.is_cancelled()
                                        })
                                    })
                                    .ok_or_else(|| {
                                        PluginError::new("UNSUPPORTED", "请从账号登录按钮打开网页")
                                    })?;
                                if calls.values().any(|c| c.web_wait.is_some()) {
                                    return Err(PluginError::new(
                                        "RATE_LIMITED",
                                        "该插件已有登录窗口",
                                    ));
                                }
                                let params: Transfer =
                                    serde_json::from_value(msg["params"].clone())
                                        .map_err(|_| PluginError::invalid("无效网页登录参数"))?;
                                params.validate(8 * 1024 * 1024)?;
                                Ok((
                                    provider,
                                    Arc::new(LoginRequest::parse(params.json(), &network)?),
                                ))
                            })();
                            match open {
                                Err(error) => {
                                    let _ = engine.invoke("deliver", (id, response(Err(error))));
                                }
                                Ok((provider, request)) => {
                                    let call = calls.get_mut(&scope).unwrap();
                                    call.web_wait = Some(Instant::now());
                                    let _ = engine
                                        .invoke("setDeadline", (scope.clone(), rquickjs::Null));
                                    let cancel = call.cancel.child_token();
                                    tasks.insert(id.clone(), (scope, cancel.clone()));
                                    let (done, epoch, plugin) =
                                        (done_tx.clone(), generation, meta.id.clone());
                                    tokio::spawn(async move {
                                        let close = cancel.child_token();
                                        let future =
                                            provider.open(plugin, request.clone(), close.clone());
                                        let result = tokio::select! {biased;
                                            _ = cancel.cancelled() => Err(PluginError::cancelled()),
                                            v = tokio::time::timeout(Duration::from_secs(request.timeout_sec), future) => v.unwrap_or_else(|_| Err(PluginError::timeout()))
                                        };
                                        close.cancel();
                                        let _ = done.send((
                                            epoch,
                                            id,
                                            HostReply::Login(request, result),
                                        ));
                                    });
                                }
                            }
                            continue;
                        }
                        let Some(c) = calls.get(&scope) else {
                            continue;
                        };
                        let cancel = c.cancel.child_token();
                        tasks.insert(id.clone(), (scope, cancel.clone()));
                        let store = store.clone();
                        let network = network.clone();
                        let done = done_tx.clone();
                        let epoch = generation;
                        tokio::spawn(async move {
                            let result = async {
                                let p: Transfer = serde_json::from_value(msg["params"].clone())
                                    .map_err(|_| PluginError::invalid("无效宿主参数"))?;
                                p.validate(8 * 1024 * 1024)?;
                                let op = msg["op"].as_str().unwrap_or("");
                                if op == "http" {
                                    network.fetch(p, cancel.clone()).await
                                } else if let Some(op) = op.strip_prefix("storage.") {
                                    store.operation(op, &p)
                                } else if let Some(op) = op.strip_prefix("cookies.") {
                                    network.cookies(op, p.json()).map(Transfer::from_json)
                                } else if op == "sleep" {
                                    tokio::time::sleep(Duration::from_millis(
                                        p.json()["ms"].as_u64().unwrap_or(0).min(60000),
                                    ))
                                    .await;
                                    Ok(Transfer::Null)
                                } else {
                                    Err(PluginError::new("UNSUPPORTED", "宿主方法未实现"))
                                }
                            };
                            let result = tokio::select! {biased; _=cancel.cancelled()=>Err(PluginError::cancelled()),r=result=>r};
                            let _ = done.send((epoch, id, HostReply::Operation(result)));
                        });
                    }
                    tasks.retain(|_, (_, t)| !t.is_cancelled());
                    if calls.is_empty()
                        && queue.is_empty()
                        && idle.elapsed() > Duration::from_secs(300)
                    {
                        break;
                    }
                }
                for (_, c) in calls.drain() {
                    c.cancel.cancel();
                    let _ = c.reply.send(Err(PluginError::cancelled()));
                }
                for c in queue {
                    c.cancel.cancel();
                    let _ = c.reply.send(Err(PluginError::cancelled()));
                }
                for token in token_map.lock().unwrap().values() {
                    token.cancel();
                }
            };
            rt.block_on(run);
        };
        std::thread::Builder::new()
            .name(thread_name)
            .spawn(run_worker)
            .map_err(|e| PluginError::internal(format!("无法创建插件线程：{e}")))?;
        let methods = ready_rx
            .await
            .map_err(|_| PluginError::internal("插件初始化线程退出"))??;
        initialization.0.take();
        Ok(Self {
            sender,
            tokens,
            shutdown,
            slots: Arc::new(Semaphore::new(40)),
            methods,
            media,
            fault,
        })
    }
    pub fn closed(&self) -> bool {
        self.sender.is_closed()
    }
    pub fn fault(&self) -> Option<PluginError> {
        self.fault.lock().unwrap().clone()
    }
    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        for t in self.tokens.lock().unwrap().values() {
            t.cancel();
        }
    }
    pub async fn stop_and_wait(&self) -> Result<()> {
        self.stop();
        tokio::time::timeout(Duration::from_secs(3), self.sender.closed())
            .await
            .map_err(|_| PluginError::internal("旧插件尚未停止，请重试更新"))
    }
    pub fn cancel(&self, id: &str) {
        if let Some(t) = self.tokens.lock().unwrap().get(id) {
            t.cancel();
        }
    }
    pub async fn call(
        &self,
        id: String,
        method: String,
        params: Transfer,
        budget: Duration,
    ) -> Result<Transfer> {
        self.call_scoped(id, method, params, budget, false).await
    }
    pub async fn call_scoped(
        &self,
        id: String,
        method: String,
        params: Transfer,
        budget: Duration,
        user_auth: bool,
    ) -> Result<Transfer> {
        params.validate(8 * 1024 * 1024)?;
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| PluginError::new("RATE_LIMITED", "插件调用队列已满"))?;
        let cancel = CancellationToken::new();
        // Dropping an IPC/refresh future must also cancel work owned by that call.
        struct CancelOnDrop(CancellationToken);
        impl Drop for CancelOnDrop {
            fn drop(&mut self) {
                self.0.cancel();
            }
        }
        let _cancel_on_drop = CancelOnDrop(cancel.clone());
        {
            let mut tokens = self.tokens.lock().unwrap();
            if tokens.contains_key(&id) {
                return Err(PluginError::invalid("requestId 重复"));
            }
            tokens.insert(id.clone(), cancel.clone());
        }
        let (reply, receive) = oneshot::channel();
        if self
            .sender
            .try_send(Command::Call(Call {
                id: id.clone(),
                method,
                params,
                deadline: Instant::now() + budget,
                user_auth,
                web_wait: None,
                cancel,
                reply,
                _permit: permit,
            }))
            .is_err()
        {
            self.tokens.lock().unwrap().remove(&id);
            return Err(PluginError::new("RATE_LIMITED", "插件调用不可用"));
        }
        receive
            .await
            .map_err(|_| PluginError::internal("插件运行时已停止"))?
    }
}
