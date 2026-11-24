use crate::{
    model::{self, Meta},
    native::Logs,
    network::Network,
    runtime::{self, Worker},
    store::{atomic_write, Store},
    wire::Transfer,
    PluginError, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

#[path = "distribution.rs"]
mod distribution;
#[path = "endpoints.rs"]
mod endpoints;
#[path = "updater.rs"]
mod updater;
pub use distribution::{DocumentReader, ImportPreview, ImportReport, PluginOrigin, UpdateCheck};
pub use updater::{Subscription, UpdatePolicy, UpdateState};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    meta: Meta,
    enabled: bool,
    source: Option<String>,
    #[serde(default)]
    origin: Option<PluginOrigin>,
    #[serde(default)]
    usable: bool,
    #[serde(default)]
    settings: BTreeMap<String, Value>,
    #[serde(default)]
    endpoint: Value,
}
struct Installed {
    entry: IndexEntry,
    worker: Option<Worker>,
    network: Arc<Network>,
    store: Arc<Store>,
    logs: Logs,
    error: Option<PluginError>,
    revision: String,
    retired: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub meta: Meta,
    pub enabled: bool,
    pub source: Option<String>,
    pub origin: Option<PluginOrigin>,
    pub can_restore: bool,
    pub status: String,
    pub methods: Vec<String>,
    pub error: Option<PluginError>,
    pub endpoint: Value,
    pub revision: String,
}
pub struct PluginManager {
    media_capabilities: Mutex<crate::capabilities::MediaCapabilities>,
    root: PathBuf,
    document_reader: Option<Arc<dyn DocumentReader>>,
    web_login: Option<Arc<dyn crate::web_login::WebLoginProvider>>,
    plugins: tokio::sync::RwLock<BTreeMap<String, Arc<tokio::sync::Mutex<Installed>>>>,
    mutation: tokio::sync::Mutex<()>,
    strict: Option<bool>,
    cooldown: Arc<Mutex<HashMap<String, Instant>>>,
    distribution: distribution::Distribution,
    updater: updater::Updater,
    pub(crate) playback_gate: tokio::sync::RwLock<()>,
    pub(crate) resolver_preferences: crate::resolver_preferences::ResolverPreferences,
    pub sessions: crate::media::MediaSessions,
    pub library: crate::library::Library,
}
impl PluginManager {
    pub fn open(root: PathBuf) -> Result<Self> {
        Self::with_mode(root, None)
    }
    pub fn with_mode(root: PathBuf, strict: Option<bool>) -> Result<Self> {
        std::fs::create_dir_all(&root)?;
        let cooldown: Arc<Mutex<HashMap<String, Instant>>> = Default::default();
        let mut plugins = BTreeMap::new();
        let path = root.join("index.json");
        let index: Vec<IndexEntry> = if path.exists() {
            serde_json::from_slice(&std::fs::read(path)?)
                .map_err(|_| PluginError::internal("插件索引损坏，原文件已保留"))?
        } else {
            vec![]
        };
        for mut entry in index {
            let indexed = Meta::parse(serde_json::to_value(&entry.meta).unwrap())?;
            let recovered = std::fs::read_to_string(root.join(&indexed.id).join("plugin.js"))
                .map_err(PluginError::from)
                .and_then(|code| runtime::inspect(&code));
            let mut error = None;
            match recovered {
                Ok(meta) if meta.id == indexed.id => {
                    if serde_json::to_value(&meta).unwrap()
                        != serde_json::to_value(&indexed).unwrap()
                    {
                        entry.usable = false;
                    }
                    entry.meta = meta;
                }
                Ok(_) => error = Some(PluginError::invalid("插件文件与索引身份不一致")),
                Err(e) => error = Some(e),
            }
            let mut installed =
                Self::installed(root.join(&indexed.id), entry, strict, cooldown.clone())?;
            installed.error = error;
            plugins.insert(indexed.id, Arc::new(tokio::sync::Mutex::new(installed)));
        }
        let library = crate::library::Library::open(root.join("library.json"))?;
        let updater = updater::Updater::open(root.clone())?;
        let resolver_preferences = crate::resolver_preferences::ResolverPreferences::open(
            root.join("resolver-preferences.json"),
        )?;
        Ok(Self {
            media_capabilities: Default::default(),
            root,
            document_reader: None,
            web_login: None,
            plugins: tokio::sync::RwLock::new(plugins),
            mutation: tokio::sync::Mutex::new(()),
            strict,
            cooldown,
            distribution: Default::default(),
            updater,
            playback_gate: Default::default(),
            resolver_preferences,
            sessions: Default::default(),
            library,
        })
    }
    pub fn with_web_login(mut self, provider: Arc<dyn crate::web_login::WebLoginProvider>) -> Self {
        self.web_login = Some(provider);
        self
    }
    pub fn with_document_reader(mut self, reader: Arc<dyn DocumentReader>) -> Self {
        self.document_reader = Some(reader);
        self
    }
    pub async fn configure_media(
        &self,
        media: crate::capabilities::MediaCapabilities,
    ) -> Result<bool> {
        media.validate()?;
        let _playback = self.playback_gate.write().await;
        let _mutation = self.mutation.lock().await;
        {
            let mut current = self.media_capabilities.lock().unwrap();
            if *current == media {
                return Ok(false);
            }
            *current = media;
        }
        // Environment is a load-time snapshot: cancel old workers and sessions;
        // the next list/call initializes using the new capabilities.
        let plugins = self
            .plugins
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for plugin in plugins {
            let mut p = plugin.lock().await;
            if let Some(worker) = p.worker.take() {
                worker.stop();
            }
            self.sessions.release_plugin(&p.entry.meta.id);
            p.revision = uuid::Uuid::new_v4().to_string();
            p.error = None;
        }
        Ok(true)
    }
    fn installed(
        dir: PathBuf,
        entry: IndexEntry,
        strict: Option<bool>,
        cooldown: Arc<Mutex<HashMap<String, Instant>>>,
    ) -> Result<Installed> {
        std::fs::create_dir_all(&dir)?;
        Ok(Installed {
            network: Arc::new(Network::new(
                entry.meta.clone(),
                dir.clone(),
                strict,
                cooldown,
            )?),
            store: Arc::new(Store::open(dir.join("storage.json"))?),
            entry,
            worker: None,
            logs: Default::default(),
            error: None,
            revision: uuid::Uuid::new_v4().to_string(),
            retired: false,
        })
    }
    async fn save(&self) -> Result<()> {
        let list = self
            .plugins
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut entries = vec![];
        for p in list {
            entries.push(p.lock().await.entry.clone());
        }
        atomic_write(
            &self.root.join("index.json"),
            &serde_json::to_vec_pretty(&entries)
                .map_err(|_| PluginError::internal("无法保存插件索引"))?,
        )
    }
    async fn get(&self, id: &str) -> Result<Arc<tokio::sync::Mutex<Installed>>> {
        self.plugins
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| PluginError::new("NOT_FOUND", "插件未安装"))
    }
    fn settings(entry: &IndexEntry) -> Value {
        let mut v = serde_json::Map::new();
        for s in &entry.meta.settings {
            if let Some(k) = s["key"].as_str() {
                if let Some(val) = entry.settings.get(k).or_else(|| s.get("default")) {
                    v.insert(k.into(), val.clone());
                }
            }
        }
        Value::Object(v)
    }
    fn requirements(&self, meta: &Meta) -> Result<()> {
        for r in &meta.requires {
            let supported = match r.r#type.as_str() {
                "feature" if r.name == "webview" => self.web_login.is_some(),
                "api" if r.name == "webview.open" => self.web_login.is_some(),
                "feature" => r.name == "charset.legacy",
                "api" => [
                    "http.fetch",
                    "storage.get",
                    "storage.set",
                    "storage.delete",
                    "storage.clear",
                    "storage.keys",
                    "cookies.get",
                    "cookies.set",
                    "cookies.clear",
                    "html.parse",
                    "crypto.digest",
                    "crypto.hmac",
                    "crypto.aesEncrypt",
                    "crypto.aesDecrypt",
                    "crypto.rsaEncrypt",
                    "crypto.randomBytes",
                    "crypto.randomUUID",
                    "codec.base64Encode",
                    "codec.base64Decode",
                    "codec.hexEncode",
                    "codec.hexDecode",
                    "codec.utf8Encode",
                    "codec.utf8Decode",
                    "codec.decodeText",
                    "settings.get",
                    "settings.getAll",
                    "endpoint.url",
                ]
                .contains(&r.name.as_str()),
                _ => false,
            };
            if !supported {
                return Err(PluginError::new(
                    "UNSUPPORTED",
                    format!("缺少必需能力 {}:{}", r.r#type, r.name),
                ));
            }
        }
        Ok(())
    }
    async fn initialize(&self, p: &mut Installed, full: bool) -> Result<Worker> {
        if p.retired {
            return Err(PluginError::cancelled());
        }
        if !p.entry.enabled {
            return Err(PluginError::new("UNSUPPORTED", "插件已停用"));
        }
        if !full {
            if let Some(w) = &p.worker {
                if let Some(error) = w.fault() {
                    p.error = Some(error.clone());
                    return Err(error);
                }
                if !w.closed() {
                    return Ok(w.clone());
                }
                if p.error.is_some() {
                    return Err(p.error.clone().unwrap());
                }
            }
        }
        if full {
            self.sessions.release_plugin(&p.entry.meta.id);
            p.revision = uuid::Uuid::new_v4().to_string();
        }
        self.requirements(&p.entry.meta)?;
        if let Some(w) = p.worker.take() {
            w.stop();
        }
        let source = std::fs::read_to_string(self.root.join(&p.entry.meta.id).join("plugin.js"))?;
        let spawn = |endpoint: Value| {
            Worker::spawn(
                source.clone(),
                p.entry.meta.clone(),
                Self::settings(&p.entry),
                endpoint,
                p.store.clone(),
                p.network.clone(),
                p.logs.clone(),
                self.media_capabilities.lock().unwrap().clone(),
                self.web_login.clone(),
            )
        };
        let mut w = spawn(p.entry.endpoint.clone()).await?;
        if full && w.methods.contains(&"endpoints.list".into()) {
            let result = w
                .call(
                    uuid::Uuid::new_v4().to_string(),
                    "endpoints.list".into(),
                    Transfer::from_json(json!({"refresh":false})),
                    p.entry.meta.budget("endpoints.list"),
                )
                .await?;
            let endpoints = model::validate_result(
                "endpoints.list",
                result.json(),
                p.entry.meta.allow_insecure_http,
            )?;
            let array = endpoints.as_array().unwrap();
            if array.is_empty() {
                w.stop();
                return Err(PluginError::internal("线路列表为空"));
            }
            let selected = array
                .iter()
                .find(|e| e["id"] == p.entry.endpoint["id"])
                .unwrap_or(&array[0])
                .clone();
            if selected != p.entry.endpoint {
                w.stop();
                w = spawn(selected.clone()).await?;
                p.entry.endpoint = selected;
            }
        }
        p.worker = Some(w.clone());
        p.error = None;
        p.entry.usable = true;
        Ok(w)
    }
    fn info(p: &Installed) -> PluginInfo {
        let error = p
            .error
            .clone()
            .or_else(|| p.worker.as_ref().and_then(Worker::fault));
        let methods = p
            .worker
            .as_ref()
            .map(|w| w.methods.clone())
            .unwrap_or_default();
        let status = if !p.entry.enabled {
            "disabled"
        } else if error.as_ref().is_some_and(|e| e.code == "UNSUPPORTED") {
            "incompatible"
        } else if error.is_some() {
            "failed"
        } else if p.worker.as_ref().is_some_and(|w| !w.closed()) {
            "ready"
        } else {
            "unloaded"
        };
        PluginInfo {
            meta: p.entry.meta.clone(),
            enabled: p.entry.enabled,
            source: p.entry.source.clone(),
            origin: p.entry.origin.clone(),
            can_restore: false,
            status: status.into(),
            methods,
            error,
            endpoint: p.entry.endpoint.clone(),
            revision: p.revision.clone(),
        }
    }
    pub async fn list(&self) -> Vec<PluginInfo> {
        let all = self
            .plugins
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut out = vec![];
        for p in all {
            let mut p = p.lock().await;
            if p.entry.enabled && p.worker.is_none() && p.error.is_none() {
                if let Err(error) = self.initialize(&mut p, true).await {
                    p.error = Some(error);
                }
            }
            let mut info = Self::info(&p);
            info.can_restore = self
                .root
                .join(&p.entry.meta.id)
                .join("plugin.previous.json")
                .exists();
            out.push(info);
        }
        out
    }
    pub async fn initialize_enabled(&self) {
        let _mutation = self.mutation.lock().await;
        let plugins = self
            .plugins
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for plugin in plugins {
            let mut p = plugin.lock().await;
            if p.entry.enabled && p.worker.is_none() && p.error.is_none() {
                if let Err(error) = self.initialize(&mut p, true).await {
                    p.error = Some(error);
                }
            }
        }
        let _ = self.save().await;
    }
    pub async fn import_code(
        &self,
        code: String,
        source: Option<String>,
        allow_downgrade: bool,
    ) -> Result<PluginInfo> {
        self.import_code_mode(code, source, allow_downgrade, false)
            .await
    }
    pub async fn import_code_mode(
        &self,
        code: String,
        source: Option<String>,
        allow_downgrade: bool,
        install_only: bool,
    ) -> Result<PluginInfo> {
        self.install_code(
            code,
            source,
            None,
            allow_downgrade,
            Some(!install_only),
            None,
            false,
            None,
        )
        .await
    }
    async fn install_code(
        &self,
        code: String,
        source: Option<String>,
        origin: Option<PluginOrigin>,
        allow_downgrade: bool,
        enabled: Option<bool>,
        expected_revision: Option<&str>,
        restoring: bool,
        cancel: Option<&tokio_util::sync::CancellationToken>,
    ) -> Result<PluginInfo> {
        let cancel = cancel.cloned().unwrap_or_default();
        let _mutation = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            guard = self.mutation.lock() => guard,
        };
        let copy = code.clone();
        let meta = tokio::task::spawn_blocking(move || runtime::inspect(&copy))
            .await
            .map_err(|_| PluginError::internal("插件检查失败"))??;
        if cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        let existing = self.plugins.read().await.get(&meta.id).cloned();
        let mut settings = BTreeMap::new();
        let mut endpoint = Value::Null;
        let mut was_enabled = true;
        let mut previous = None;
        if let Some(p) = &existing {
            let p = p.lock().await;
            if expected_revision.is_some_and(|r| r != p.revision) {
                return Err(PluginError::invalid("插件已变化，请重新检查更新"));
            }
            if model::version(&meta.version)? < model::version(&p.entry.meta.version)?
                && !allow_downgrade
            {
                return Err(PluginError::invalid("该版本更低；确认降级后再导入"));
            }
            settings = p.entry.settings.clone();
            endpoint = p.entry.endpoint.clone();
            was_enabled = p.entry.enabled;
            if !restoring && p.entry.usable {
                previous = Some(distribution::Snapshot {
                    code: std::fs::read_to_string(self.root.join(&meta.id).join("plugin.js"))?,
                    entry: p.entry.clone(),
                });
            }
        } else if expected_revision.is_some() {
            return Err(PluginError::invalid("插件已移除，请重新导入"));
        }
        if restoring {
            let snapshot: distribution::Snapshot = serde_json::from_slice(&std::fs::read(
                self.root.join(&meta.id).join("plugin.previous.json"),
            )?)
            .map_err(|_| PluginError::invalid("恢复文件损坏"))?;
            let current = settings;
            settings = snapshot.entry.settings;
            settings.extend(current);
        }
        settings.retain(|key, value| {
            meta.settings
                .iter()
                .find(|s| s["key"].as_str() == Some(key))
                .is_some_and(|s| model::validate_setting(s, value).is_ok())
        });
        if cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        let dir = self.root.join(&meta.id);
        std::fs::create_dir_all(&dir)?;
        let file = dir.join("plugin.js");
        if let Some(previous) = previous {
            atomic_write(
                &dir.join("plugin.previous.json"),
                &serde_json::to_vec(&previous)
                    .map_err(|_| PluginError::internal("无法保存恢复文件"))?,
            )?;
        }
        // Keep stale references from reinitializing against a replaced file. Wait for the old
        // thread to finish before reopening storage/cookies, so final writes are visible.
        let old_code = std::fs::read(&file).ok();
        let entry = IndexEntry {
            meta: meta.clone(),
            enabled: enabled.unwrap_or(was_enabled),
            source,
            origin,
            usable: false,
            settings,
            endpoint,
        };
        let mut old_guard = match &existing {
            Some(old) => Some(old.lock().await),
            None => None,
        };
        if let Some(old) = old_guard.as_mut() {
            if let Some(w) = old.worker.take() {
                w.stop_and_wait().await?;
            }
        }
        let mut p = Self::installed(dir.clone(), entry, self.strict, self.cooldown.clone())?;
        if let Some(old) = old_guard.as_ref() {
            p.store = old.store.clone();
        }
        if cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        atomic_write(&file, code.as_bytes())?;
        if let Some(old) = old_guard.as_mut() {
            old.retired = true;
        }
        drop(old_guard);
        self.sessions.release_plugin(&meta.id);
        if p.entry.enabled {
            if let Err(e) = self.initialize(&mut p, true).await {
                p.error = Some(e);
            }
        }
        let mut info = Self::info(&p);
        info.can_restore = dir.join("plugin.previous.json").exists();
        self.plugins
            .write()
            .await
            .insert(meta.id.clone(), Arc::new(tokio::sync::Mutex::new(p)));
        if let Err(e) = self.save().await {
            if let Some(new) = self.plugins.write().await.remove(&meta.id) {
                if let Some(w) = new.lock().await.worker.take() {
                    w.stop();
                }
            }
            if let Some(bytes) = old_code {
                atomic_write(&file, &bytes)?;
            } else {
                let _ = std::fs::remove_file(&file);
            }
            if let Some(old) = existing {
                old.lock().await.retired = false;
                self.plugins.write().await.insert(meta.id, old);
            }
            return Err(e);
        }
        Ok(info)
    }
    pub async fn retry(&self, id: &str) -> Result<PluginInfo> {
        let _mutation = self.mutation.lock().await;
        let p = self.get(id).await?;
        let info = {
            let mut p = p.lock().await;
            if let Err(e) = self.initialize(&mut p, true).await {
                p.error = Some(e);
            }
            Self::info(&p)
        };
        self.save().await?;
        Ok(info)
    }
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<PluginInfo> {
        let _mutation = self.mutation.lock().await;
        let p = self.get(id).await?;
        let info = {
            let mut p = p.lock().await;
            p.entry.enabled = enabled;
            if let Some(w) = p.worker.take() {
                w.stop();
            }
            self.sessions.release_plugin(id);
            p.error = None;
            if enabled {
                if let Err(e) = self.initialize(&mut p, true).await {
                    p.error = Some(e);
                }
            }
            Self::info(&p)
        };
        self.save().await?;
        Ok(info)
    }
    pub async fn remove(&self, id: &str) -> Result<()> {
        let _mutation = self.mutation.lock().await;
        let p = self.get(id).await?;
        {
            let p = p.lock().await;
            if let Some(w) = &p.worker {
                w.stop();
            }
            self.sessions.release_plugin(id);
        }
        self.plugins.write().await.remove(id);
        self.save().await?;
        // The ID was validated on import/load; resolved target must remain an immediate child.
        let target = self.root.join(id).canonicalize()?;
        let root = self.root.canonicalize()?;
        if target.parent() != Some(root.as_path()) {
            return Err(PluginError::invalid("插件目录越界"));
        }
        std::fs::remove_dir_all(target)?;
        Ok(())
    }
    pub async fn call_transfer(
        &self,
        id: &str,
        method: &str,
        params: Transfer,
        request_id: String,
    ) -> Result<Transfer> {
        self.call_scoped(id, method, params, request_id, false).await
    }
    pub(crate) async fn call_scoped(
        &self,
        id: &str,
        method: &str,
        mut params: Transfer,
        request_id: String,
        user_auth: bool,
    ) -> Result<Transfer> {
        if request_id.is_empty() || request_id.len() > 128 {
            return Err(PluginError::invalid("requestId 无效"));
        }
        crate::forms::validate_params(method, &params.json())?;
        let plugin = self.get(id).await?;
        let (w, meta, logs) = {
            let mut p = plugin.lock().await;
            let full = p.worker.is_none();
            let w = self.initialize(&mut p, full).await?;
            if !w.methods.iter().any(|m| m == method) {
                return Err(PluginError::new("UNSUPPORTED", "插件未实现该方法"));
            }
            if matches!(
                method,
                "media.resolvePlayback" | "live.resolve" | "resolver.resolve"
            ) {
                let Transfer::Object(ref mut fields) = params else {
                    return Err(PluginError::invalid("播放参数必须是对象"));
                };
                fields.insert("codecs".into(), Transfer::from_json(json!(w.media.codecs)));
            }
            (w, p.entry.meta.clone(), p.logs.clone())
        };
        let result = w
            .call_scoped(
                request_id.clone(),
                method.into(),
                params,
                meta.budget(method),
                user_auth && method == "auth.submit",
            )
            .await
            .map_err(|mut e| {
                if matches!(e.code.as_str(), "AUTH_REQUIRED" | "AUTH_EXPIRED") {
                    e.plugin_id = Some(id.into());
                }
                e
            })?;
        if method == "transformResource" {
            result.validate(8 * 1024 * 1024)?;
            return Ok(result);
        }
        let mut warnings = Vec::new();
        let validated = model::validate_result_with_warnings(
            method,
            result.json(),
            meta.allow_insecure_http,
            &mut warnings,
        );
        for warning in warnings {
            crate::native::validation_log(&logs, "warn", &warning, &request_id);
        }
        if let Err(e) = &validated {
            crate::native::validation_log(&logs, "error", &e.message, &request_id);
        }
        validated.map(Transfer::from_json)
    }
    pub async fn call(
        &self,
        id: &str,
        method: &str,
        params: Value,
        request_id: String,
    ) -> Result<Value> {
        self.call_transfer(id, method, Transfer::from_json(params), request_id)
            .await
            .map(|v| v.json())
    }
    pub async fn cancel(&self, request_id: &str) {
        self.sessions.cancel_refresh_waiter(request_id);
        self.distribution.cancel(request_id);
        let list = self
            .plugins
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for p in list {
            if let Some(w) = &p.lock().await.worker {
                w.cancel(request_id);
            }
        }
    }
    pub async fn logs(&self, id: &str) -> Result<Vec<Value>> {
        let p = self.get(id).await?;
        let p = p.lock().await;
        let logs = p.logs.lock().unwrap().snapshot();
        Ok(logs)
    }
    pub async fn settings_get(&self, id: &str) -> Result<Value> {
        Ok(Self::settings(&self.get(id).await?.lock().await.entry))
    }
    pub async fn settings_set(&self, id: &str, key: String, value: Value) -> Result<PluginInfo> {
        let _mutation = self.mutation.lock().await;
        let plugin = self.get(id).await?;
        let info = {
            let mut p = plugin.lock().await;
            let s = p
                .entry
                .meta
                .settings
                .iter()
                .find(|s| s["key"] == key)
                .ok_or_else(|| PluginError::invalid("未知设置 key"))?;
            model::validate_setting(s, &value)?;
            p.entry.settings.insert(key, value);
            self.sessions.release_plugin(id);
            if let Some(w) = p.worker.take() {
                w.stop();
            }
            if let Err(e) = self.initialize(&mut p, true).await {
                p.error = Some(e);
            }
            Self::info(&p)
        };
        self.save().await?;
        Ok(info)
    }
    pub async fn endpoint_set(&self, id: &str, endpoint_id: &str) -> Result<PluginInfo> {
        let entries = self
            .call(
                id,
                "endpoints.list",
                json!({"refresh":false}),
                uuid::Uuid::new_v4().to_string(),
            )
            .await?;
        let selected = entries
            .as_array()
            .and_then(|a| a.iter().find(|e| e["id"] == endpoint_id))
            .ok_or_else(|| PluginError::invalid("线路 ID 不存在"))?
            .clone();
        let _mutation = self.mutation.lock().await;
        let plugin = self.get(id).await?;
        let info = {
            let mut p = plugin.lock().await;
            p.entry.endpoint = selected;
            p.revision = uuid::Uuid::new_v4().to_string();
            self.sessions.release_plugin(id);
            if let Some(w) = p.worker.take() {
                w.stop();
            }
            if let Err(e) = self.initialize(&mut p, false).await {
                p.error = Some(e);
            }
            Self::info(&p)
        };
        self.save().await?;
        Ok(info)
    }
    pub(crate) async fn network(&self, id: &str) -> Result<Arc<Network>> {
        let p = self.get(id).await?;
        let p = p.lock().await;
        if !p.entry.enabled {
            return Err(PluginError::new("UNSUPPORTED", "插件已停用"));
        }
        Ok(p.network.clone())
    }
    pub async fn meta(&self, id: &str) -> Result<Meta> {
        Ok(self.get(id).await?.lock().await.entry.meta.clone())
    }
    pub(crate) async fn revision(&self, id: &str) -> Result<String> {
        let p = self.get(id).await?;
        let p = p.lock().await;
        if !p.entry.enabled {
            return Err(PluginError::cancelled());
        }
        Ok(p.revision.clone())
    }
}
impl Drop for PluginManager {
    fn drop(&mut self) {
        if let Ok(all) = self.plugins.try_read() {
            for p in all.values() {
                if let Ok(p) = p.try_lock() {
                    if let Some(w) = &p.worker {
                        w.stop();
                    }
                }
            }
        }
    }
}
