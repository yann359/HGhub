//! Distribution is a host operation. Downloads never inherit a plugin's credentials.
use super::*;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use url::Url;

const JS_LIMIT: usize = 2 * 1024 * 1024;
const MANIFEST_LIMIT: usize = 1024 * 1024;

/// A platform adapter for user-selected documents with opaque provider URIs.
/// Reads must stop at limit + 1 bytes. The core validates size and UTF-8.
pub trait DocumentReader: Send + Sync {
    fn read(&self, location: &str, limit: usize) -> Result<Vec<u8>>;
    fn resolve_relative(&self, _base: &str, _reference: &str) -> Result<String> {
        Err(PluginError::new(
            "UNSUPPORTED",
            "请先选择列表所在目录并授权，或分别选择插件文件",
        ))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOrigin {
    pub kind: String,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_name: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Snapshot {
    pub code: String,
    pub entry: IndexEntry,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub key: String,
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub source: String,
    pub installed_version: Option<String>,
    pub error: Option<PluginError>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub token: String,
    pub name: String,
    pub description: String,
    pub items: Vec<ImportItem>,
    pub updates_only: bool,
    pub subscriptions: Vec<SubscriptionPreview>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SubscriptionPreview {
    pub location: String,
    pub name: String,
    pub description: String,
}
#[derive(Serialize)]
pub struct ImportOutcome {
    pub key: String,
    pub id: String,
    pub plugin: Option<PluginInfo>,
    pub error: Option<PluginError>,
}
#[derive(Serialize)]
pub struct ImportReport {
    pub items: Vec<ImportOutcome>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub id: String,
    pub status: String,
    pub error: Option<PluginError>,
}
#[derive(Serialize)]
pub struct UpdateCheck {
    pub preview: ImportPreview,
    pub statuses: Vec<UpdateStatus>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Candidate {
    pub(super) info: ImportItem,
    pub(super) origin: PluginOrigin,
    pub(super) code: Option<String>,
    pub(super) expected: Option<(String, String)>,
    pub(super) revision: Option<String>,
}
#[derive(Clone)]
struct Pending {
    created: Instant,
    items: Vec<Candidate>,
    updates_only: bool,
    subscriptions: Vec<SubscriptionPreview>,
}
#[derive(Default)]
pub(super) struct Distribution {
    pending: Mutex<BTreeMap<String, Pending>>,
    active: Mutex<HashMap<String, CancellationToken>>,
}
pub(super) struct Operation<'a> {
    state: &'a Distribution,
    id: String,
    pub(super) cancel: CancellationToken,
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.state.active.lock().unwrap().remove(&self.id);
    }
}
impl Distribution {
    pub(super) fn begin(&self, id: &str) -> Result<Operation<'_>> {
        if id.is_empty() || id.len() > 128 {
            return Err(PluginError::invalid("requestId 无效"));
        }
        let mut active = self.active.lock().unwrap();
        if active.contains_key(id) {
            return Err(PluginError::invalid("requestId 已在使用"));
        }
        let cancel = CancellationToken::new();
        active.insert(id.into(), cancel.clone());
        Ok(Operation {
            state: self,
            id: id.into(),
            cancel,
        })
    }
    pub(super) fn cancel(&self, id: &str) {
        if let Some(token) = self.active.lock().unwrap().get(id) {
            token.cancel();
        }
    }
    fn stage(
        &self,
        name: String,
        description: String,
        items: Vec<Candidate>,
        updates_only: bool,
        subscriptions: Vec<SubscriptionPreview>,
    ) -> ImportPreview {
        let token = uuid::Uuid::new_v4().to_string();
        let preview = ImportPreview {
            token: token.clone(),
            name,
            description,
            items: items.iter().map(|c| c.info.clone()).collect(),
            updates_only,
            subscriptions: subscriptions.clone(),
        };
        let mut pending = self.pending.lock().unwrap();
        pending.retain(|_, p| p.created.elapsed() < Duration::from_secs(900));
        // Keep staged code bounded; lists contain references and download only selected entries.
        let new_bytes = items
            .iter()
            .map(|c| c.code.as_ref().map_or(0, String::len))
            .sum::<usize>();
        while pending.len() >= 16
            || pending
                .values()
                .flat_map(|p| &p.items)
                .map(|c| c.code.as_ref().map_or(0, String::len))
                .sum::<usize>()
                + new_bytes
                > 32 * JS_LIMIT
        {
            if let Some(oldest) = pending
                .iter()
                .min_by_key(|(_, p)| p.created)
                .map(|(k, _)| k.clone())
            {
                pending.remove(&oldest);
            } else {
                break;
            }
        }
        pending.insert(
            token,
            Pending {
                created: Instant::now(),
                items,
                updates_only,
                subscriptions,
            },
        );
        preview
    }
}

struct Document {
    text: String,
    final_location: String,
}
fn remote(location: &str) -> bool {
    location.split_once(':').is_some_and(|(scheme, _)| {
        scheme.eq_ignore_ascii_case("https") || scheme.eq_ignore_ascii_case("http")
    })
}
fn utf8(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes)
        .map(|s| s.trim_start_matches('\u{feff}').to_string())
        .map_err(|_| PluginError::invalid("插件文件必须为 UTF-8"))
}
fn manifest(text: &str) -> Result<Option<Value>> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => {
            let entries = v["plugins"]
                .as_array()
                .ok_or_else(|| PluginError::invalid("JSON 文件必须包含 plugins 数组"))?;
            if text.len() > MANIFEST_LIMIT || entries.len() > 500 {
                return Err(PluginError::invalid("插件列表超过 1 MiB 或 500 个条目"));
            }
            Ok(Some(v))
        }
        Err(e) if text.trim_start().starts_with('{') => {
            Err(PluginError::invalid(format!("插件列表 JSON 无效：{e}")))
        }
        Err(_) => Ok(None),
    }
}
fn resolve_entry(
    base: &str,
    reference: &str,
    reader: Option<&dyn DocumentReader>,
) -> Result<String> {
    if remote(base) {
        let url = Url::parse(base)
            .and_then(|u| u.join(reference))
            .map_err(|_| PluginError::invalid("列表条目地址无效"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(PluginError::invalid("远程列表只能引用 HTTP(S) 插件"));
        }
        return Ok(url.into());
    }
    if remote(reference) {
        return Ok(reference.into());
    }
    if base.starts_with("content://") {
        return reader
            .ok_or_else(|| PluginError::new("UNSUPPORTED", "当前宿主不支持目录授权"))?
            .resolve_relative(base, reference);
    }
    let base = Url::from_file_path(base).map_err(|_| PluginError::invalid("本地列表路径无效"))?;
    base.join(reference)
        .map_err(|_| PluginError::invalid("本地条目路径无效"))?
        .to_file_path()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| PluginError::invalid("本地列表条目只能使用文件或 HTTP(S) 地址"))
}

impl PluginManager {
    fn distribution_network(&self) -> Result<Network> {
        Network::new(
            Meta::parse(
                json!({"id":"host.distribution","name":"插件分发","version":"0.0.1","protocol":1,"allowInsecureHttp":true}),
            )?,
            self.root.join(".distribution"),
            self.strict,
            self.cooldown.clone(),
        )
    }
    async fn load_document(&self, location: &str, cancel: &CancellationToken) -> Result<Document> {
        if cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        if remote(location) {
            let network = Arc::new(self.distribution_network()?);
            let response = network.fetch(Transfer::from_json(json!({"url":location,"responseType":"bytes","credentials":"omit","maxBytes":JS_LIMIT,"retry":0})), cancel.clone()).await?;
            let meta = response.json();
            if meta["ok"] != true {
                return Err(PluginError::new(
                    "NETWORK",
                    format!("下载失败：HTTP {}", meta["status"]),
                ));
            }
            let bytes = response
                .field("data")
                .ok_or_else(|| PluginError::internal("下载缺少内容"))?
                .bytes()?;
            Ok(Document {
                text: utf8(bytes)?,
                final_location: meta["url"].as_str().unwrap_or(location).into(),
            })
        } else if location.starts_with("content://") {
            let reader = self
                .document_reader
                .clone()
                .ok_or_else(|| PluginError::new("UNSUPPORTED", "当前宿主不支持文件提供者地址"))?;
            let owned = location.to_string();
            let read = tokio::task::spawn_blocking(move || reader.read(&owned, JS_LIMIT));
            let bytes = tokio::select! {
                _ = cancel.cancelled() => return Err(PluginError::cancelled()),
                result = read => result.map_err(|_| PluginError::internal("读取所选文件失败"))??,
            };
            if bytes.len() > JS_LIMIT {
                return Err(PluginError::invalid("插件文件超过 2 MiB"));
            }
            Ok(Document {
                text: utf8(bytes)?,
                final_location: location.into(),
            })
        } else {
            use std::io::Read;
            let path = Path::new(location).canonicalize()?;
            let mut bytes = vec![];
            std::fs::File::open(&path)?
                .take((JS_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > JS_LIMIT {
                return Err(PluginError::invalid("插件文件超过 2 MiB"));
            }
            Ok(Document {
                text: utf8(bytes)?,
                final_location: path.to_string_lossy().into_owned(),
            })
        }
    }
    pub(super) async fn inspect_document(code: &str) -> Result<Meta> {
        let copy = code.to_string();
        tokio::task::spawn_blocking(move || runtime::inspect(&copy))
            .await
            .map_err(|_| PluginError::internal("插件检查失败"))?
    }
    async fn candidates(
        &self,
        location: &str,
        document: Document,
        cancel: &CancellationToken,
    ) -> Result<(String, String, Vec<Candidate>)> {
        let installed = self.plugins.read().await.clone();
        if let Some(list) = manifest(&document.text)? {
            let mut items = vec![];
            for entry in list["plugins"].as_array().unwrap() {
                if cancel.is_cancelled() {
                    return Err(PluginError::cancelled());
                }
                let id = entry["id"].as_str().unwrap_or("").to_string();
                let name = entry["name"].as_str().unwrap_or("").to_string();
                let version = entry["version"].as_str().unwrap_or("").to_string();
                let base = document.final_location.clone();
                let reference = entry["url"].as_str().unwrap_or("").to_string();
                let reader = self.document_reader.clone();
                let identity = json!({"id":id,"name":name,"version":version,"protocol":1});
                let resolve = tokio::task::spawn_blocking(move || {
                    // Reuse exactly the plugin's identity and version validation rules.
                    Meta::parse(identity)?;
                    if reference.is_empty() {
                        return Err(PluginError::invalid("列表条目缺少 url"));
                    }
                    resolve_entry(&base, &reference, reader.as_deref())
                });
                let parsed = tokio::select! {
                    _ = cancel.cancelled() => return Err(PluginError::cancelled()),
                    result = resolve => result.map_err(|_| PluginError::internal("解析列表条目失败"))?,
                };
                let (source, error) = match parsed {
                    Ok(v) => (v, None),
                    Err(e) => (String::new(), Some(e)),
                };
                let installed_version = if let Some(p) = installed.get(&id) {
                    Some(p.lock().await.entry.meta.version.clone())
                } else {
                    None
                };
                items.push(Candidate {
                    info: ImportItem {
                        key: uuid::Uuid::new_v4().to_string(),
                        id: id.clone(),
                        name,
                        description: entry["description"].as_str().unwrap_or("").into(),
                        version: version.clone(),
                        source: source.clone(),
                        error,
                        installed_version,
                    },
                    origin: PluginOrigin {
                        kind: "manifest".into(),
                        location: source,
                        manifest: Some(location.into()),
                        manifest_name: list["name"].as_str().map(String::from),
                    },
                    code: None,
                    expected: Some((id, version)),
                    revision: None,
                });
            }
            Ok((
                list["name"].as_str().unwrap_or("插件列表").into(),
                list["description"].as_str().unwrap_or("").into(),
                items,
            ))
        } else {
            let meta = Self::inspect_document(&document.text).await?;
            let installed_version = if let Some(p) = installed.get(&meta.id) {
                Some(p.lock().await.entry.meta.version.clone())
            } else {
                None
            };
            let item = Candidate {
                info: ImportItem {
                    key: uuid::Uuid::new_v4().to_string(),
                    id: meta.id.clone(),
                    name: meta.name.clone(),
                    description: meta.description.clone(),
                    version: meta.version.clone(),
                    source: location.into(),
                    installed_version,
                    error: None,
                },
                origin: PluginOrigin {
                    kind: if remote(location) { "url" } else { "file" }.into(),
                    location: location.into(),
                    manifest: None,
                    manifest_name: None,
                },
                code: Some(document.text),
                expected: Some((meta.id, meta.version)),
                revision: None,
            };
            Ok((meta.name, meta.description, vec![item]))
        }
    }
    pub async fn preview_import(
        &self,
        locations: Vec<String>,
        request_id: &str,
    ) -> Result<ImportPreview> {
        if locations.is_empty() || locations.len() > 500 {
            return Err(PluginError::invalid("一次选择 1 至 500 个文件或地址"));
        }
        let op = self.distribution.begin(request_id)?;
        let mut name = "导入插件".to_string();
        let mut description = String::new();
        let mut items = vec![];
        let mut subscriptions = vec![];
        let single = locations.len() == 1;
        for location in locations {
            let result: Result<_> = async {
                let document = self.load_document(&location, &op.cancel).await?;
                let location = if remote(&location) {
                    location.clone()
                } else {
                    document.final_location.clone()
                };
                let subscribe = remote(&location) && manifest(&document.text)?.is_some();
                let (name, description, items) =
                    self.candidates(&location, document, &op.cancel).await?;
                Ok((name, description, items, subscribe))
            }
            .await;
            match result {
                Ok((n, d, mut candidates, subscribe)) => {
                    if subscribe {
                        subscriptions.push(SubscriptionPreview {
                            location: location.clone(),
                            name: n.clone(),
                            description: d.clone(),
                        });
                    }
                    if single {
                        name = n;
                        description = d;
                    }
                    let mut bytes = items
                        .iter()
                        .map(|c: &Candidate| c.code.as_ref().map_or(0, String::len))
                        .sum::<usize>();
                    for candidate in &mut candidates {
                        let size = candidate.code.as_ref().map_or(0, String::len);
                        if bytes + size > 32 * JS_LIMIT {
                            candidate.code = None;
                        } else {
                            bytes += size;
                        }
                    }
                    items.append(&mut candidates);
                }
                Err(e) if e.code == "CANCELLED" => return Err(e),
                Err(e) => items.push(Candidate {
                    info: ImportItem {
                        key: uuid::Uuid::new_v4().to_string(),
                        id: String::new(),
                        name: location.clone(),
                        description: String::new(),
                        version: String::new(),
                        source: location.clone(),
                        installed_version: None,
                        error: Some(e),
                    },
                    origin: PluginOrigin {
                        kind: "file".into(),
                        location,
                        manifest: None,
                        manifest_name: None,
                    },
                    code: None,
                    expected: None,
                    revision: None,
                }),
            }
            if items.len() > 500 {
                return Err(PluginError::invalid("本次导入超过 500 个条目"));
            }
        }
        if op.cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        Ok(self
            .distribution
            .stage(name, description, items, false, subscriptions))
    }
    pub fn subscription_save(
        &self,
        token: &str,
        location: &str,
    ) -> Result<super::updater::UpdateState> {
        let subscription = self
            .distribution
            .pending
            .lock()
            .unwrap()
            .get(token)
            .filter(|p| p.created.elapsed() < Duration::from_secs(900))
            .and_then(|p| p.subscriptions.iter().find(|s| s.location == location))
            .cloned()
            .ok_or_else(|| PluginError::invalid("只能保存当前预览中的远程插件列表"))?;
        self.updater.subscription_save(subscription)?;
        Ok(self.update_state())
    }
    pub fn discard_import(&self, token: &str) {
        self.distribution.pending.lock().unwrap().remove(token);
    }
    pub async fn apply_import(
        &self,
        token: &str,
        selected: Vec<String>,
        allow_downgrade: bool,
        install_only: bool,
        request_id: &str,
    ) -> Result<ImportReport> {
        let op = self.distribution.begin(request_id)?;
        let pending = {
            let mut staged = self.distribution.pending.lock().unwrap();
            let pending = staged
                .get(token)
                .filter(|p| p.created.elapsed() < Duration::from_secs(900))
                .ok_or_else(|| PluginError::invalid("导入预览已过期，请重新读取"))?;
            if selected.is_empty()
                || selected
                    .iter()
                    .any(|k| !pending.items.iter().any(|c| &c.info.key == k))
            {
                return Err(PluginError::invalid("请选择有效的插件条目"));
            }
            staged.remove(token).unwrap()
        };
        let mut items = vec![];
        for candidate in pending
            .items
            .iter()
            .filter(|c| selected.contains(&c.info.key))
        {
            let result = async {
                if op.cancel.is_cancelled() {
                    return Err(PluginError::cancelled());
                }
                if let Some(e) = &candidate.info.error {
                    return Err(e.clone());
                }
                let code = match &candidate.code {
                    Some(c) => c.clone(),
                    None => {
                        self.load_document(&candidate.origin.location, &op.cancel)
                            .await?
                            .text
                    }
                };
                let meta = Self::inspect_document(&code).await?;
                if candidate
                    .expected
                    .as_ref()
                    .is_some_and(|(id, version)| id != &meta.id || version != &meta.version)
                {
                    return Err(PluginError::invalid("文件的 id 或 version 与列表不一致"));
                }
                if op.cancel.is_cancelled() {
                    return Err(PluginError::cancelled());
                }
                self.install_code(
                    code,
                    Some(
                        candidate
                            .origin
                            .manifest
                            .as_ref()
                            .unwrap_or(&candidate.origin.location)
                            .clone(),
                    ),
                    Some(candidate.origin.clone()),
                    !pending.updates_only && allow_downgrade,
                    if pending.updates_only {
                        None
                    } else {
                        Some(!install_only)
                    },
                    candidate.revision.as_deref(),
                    false,
                    Some(&op.cancel),
                )
                .await
            }
            .await;
            let (plugin, error) = match result {
                Ok(p) => (Some(p), None),
                Err(e) => (None, Some(e)),
            };
            items.push(ImportOutcome {
                key: candidate.info.key.clone(),
                id: candidate.info.id.clone(),
                plugin,
                error,
            });
        }
        self.discard_import(token);
        Ok(ImportReport { items })
    }
    fn update_origin(entry: &IndexEntry) -> Option<PluginOrigin> {
        if let Some(url) = &entry.meta.update_url {
            Some(PluginOrigin {
                kind: "url".into(),
                location: url.clone(),
                manifest: None,
                manifest_name: None,
            })
        } else {
            entry
                .origin
                .clone()
                .filter(|o| remote(o.manifest.as_ref().unwrap_or(&o.location)))
                .or_else(|| {
                    entry
                        .source
                        .as_ref()
                        .filter(|s| remote(s))
                        .map(|url| PluginOrigin {
                            kind: "url".into(),
                            location: url.clone(),
                            manifest: None,
                            manifest_name: None,
                        })
                })
        }
    }
    async fn update_candidate(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<Candidate>> {
        let plugin = self.get(id).await?;
        let (entry, revision) = {
            let p = plugin.lock().await;
            (p.entry.clone(), p.revision.clone())
        };
        let origin = Self::update_origin(&entry);
        let Some(origin) = origin else {
            return Err(PluginError::new("UNSUPPORTED", "没有更新地址"));
        };
        let mut next_origin = origin.clone();
        let code = if origin.kind == "manifest" {
            let location = origin
                .manifest
                .as_ref()
                .ok_or_else(|| PluginError::invalid("缺少来源列表"))?;
            let document = self.load_document(location, cancel).await?;
            if manifest(&document.text)?.is_none() {
                return Err(PluginError::invalid("来源列表已不再是 JSON 插件列表"));
            }
            let (_, _, candidates) = self.candidates(location, document, cancel).await?;
            let mut matching = candidates.into_iter().filter(|c| c.info.id == id);
            let candidate = matching
                .next()
                .ok_or_else(|| PluginError::new("NOT_FOUND", "来源列表已不包含该插件"))?;
            if matching.next().is_some() {
                return Err(PluginError::invalid("来源列表包含重复插件 ID"));
            }
            if let Some(error) = candidate.info.error {
                return Err(error);
            }
            if model::version(&candidate.info.version)? <= model::version(&entry.meta.version)? {
                return Ok(None);
            }
            next_origin = candidate.origin;
            let code = self
                .load_document(&next_origin.location, cancel)
                .await?
                .text;
            let meta = Self::inspect_document(&code).await?;
            if meta.id != id || meta.version != candidate.info.version {
                return Err(PluginError::invalid(
                    "更新文件的 id 或 version 与来源列表不一致",
                ));
            }
            code
        } else {
            self.load_document(&origin.location, cancel).await?.text
        };
        let meta = Self::inspect_document(&code).await?;
        if meta.id != id {
            return Err(PluginError::invalid("更新文件的插件 ID 不一致"));
        }
        if model::version(&meta.version)? <= model::version(&entry.meta.version)? {
            return Ok(None);
        }
        // updateUrl selects bytes; it does not replace the original installation provenance.
        Ok(Some(Candidate {
            info: ImportItem {
                key: uuid::Uuid::new_v4().to_string(),
                id: id.into(),
                name: meta.name,
                description: meta.description,
                version: meta.version.clone(),
                source: next_origin.location.clone(),
                installed_version: Some(entry.meta.version),
                error: None,
            },
            origin: if entry.meta.update_url.is_some() {
                entry.origin.unwrap_or(next_origin)
            } else {
                next_origin
            },
            expected: Some((id.into(), meta.version)),
            code: Some(code),
            revision: Some(revision),
        }))
    }
    pub async fn check_updates(&self, ids: Vec<String>, request_id: &str) -> Result<UpdateCheck> {
        let _work = self
            .updater
            .work
            .try_lock()
            .map_err(|_| PluginError::new("RATE_LIMITED", "已有插件更新检查或安装正在运行"))?;
        let (items, statuses) = self.collect_updates(ids, request_id, false).await?;
        self.capture_updates(&items).await?;
        Ok(UpdateCheck {
            preview: self.distribution.stage(
                "插件更新".into(),
                "选择要安装的更新。替换时会停止该插件的播放。".into(),
                items,
                true,
                vec![],
            ),
            statuses,
        })
    }
    pub(super) async fn collect_updates(
        &self,
        ids: Vec<String>,
        request_id: &str,
        automatic: bool,
    ) -> Result<(Vec<Candidate>, Vec<UpdateStatus>)> {
        if ids.len() > 500 {
            return Err(PluginError::invalid("更新检查超过 500 个插件"));
        }
        let op = self.distribution.begin(request_id)?;
        let mut items = vec![];
        let mut statuses = vec![];
        let mut staged_bytes = 0;
        for id in ids {
            if automatic && !self.updater.policy().check_on_startup {
                break;
            }
            let result = async {
                let plugin = self.get(&id).await?;
                let origin = Self::update_origin(&plugin.lock().await.entry)
                    .ok_or_else(|| PluginError::new("UNSUPPORTED", "没有更新地址"))?;
                let address = origin.manifest.as_ref().unwrap_or(&origin.location);
                if automatic
                    && !self
                        .updater
                        .reserve(&id, address, crate::store::now(), true)?
                {
                    return Ok(None);
                }
                self.update_candidate(&id, &op.cancel).await.map(Some)
            }
            .await;
            if matches!(result, Ok(None)) {
                statuses.push(UpdateStatus {
                    id,
                    status: "skipped".into(),
                    error: None,
                });
                continue;
            }
            let (status, error) = match result {
                Ok(Some(Some(candidate))) => {
                    let size = candidate.code.as_ref().map_or(0, String::len);
                    if staged_bytes + size > 32 * JS_LIMIT {
                        (
                            "failed",
                            Some(PluginError::invalid(
                                "暂存更新已满，请安装本批更新后再次检查",
                            )),
                        )
                    } else {
                        staged_bytes += size;
                        items.push(candidate);
                        ("available", None)
                    }
                }
                Ok(Some(None)) => ("current", None),
                Ok(None) => unreachable!(),
                Err(e) if e.code == "CANCELLED" => return Err(e),
                Err(e) => (
                    if e.code == "UNSUPPORTED" {
                        "noSource"
                    } else {
                        "failed"
                    },
                    Some(e),
                ),
            };
            statuses.push(UpdateStatus {
                id,
                status: status.into(),
                error,
            });
        }
        if op.cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        self.updater.checks_set(statuses.clone())?;
        Ok((items, statuses))
    }
    pub async fn restore_previous(&self, id: &str) -> Result<PluginInfo> {
        // A snapshot is a host-written file, and restoration never rewinds storage or upstream effects.
        let plugin = self.get(id).await?;
        let revision = plugin.lock().await.revision.clone();
        let snapshot: Snapshot = serde_json::from_slice(&std::fs::read(
            self.root.join(id).join("plugin.previous.json"),
        )?)
        .map_err(|_| PluginError::invalid("恢复文件损坏"))?;
        let meta = Self::inspect_document(&snapshot.code).await?;
        if meta.id != id {
            return Err(PluginError::invalid("恢复文件身份不一致"));
        }
        self.install_code(
            snapshot.code,
            snapshot.entry.source,
            snapshot.entry.origin,
            true,
            None,
            Some(&revision),
            true,
            None,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelled_install_waiting_for_mutation_does_not_replace_plugin() {
        let root = tempfile::tempdir().unwrap();
        let manager = PluginManager::with_mode(root.path().into(), Some(false)).unwrap();
        let cancel = CancellationToken::new();
        // Another plugin operation owns the mutation lock while this import is queued.
        let guard = manager.mutation.lock().await;
        let task = manager.install_code(
            include_str!("../tests/fixtures/synthetic.js").into(),
            None,
            None,
            false,
            Some(true),
            None,
            false,
            Some(&cancel),
        );
        tokio::pin!(task);
        // Poll once to enter the wait before cancelling; no timing assumptions or sleeps.
        assert!(futures_util::poll!(task.as_mut()).is_pending());
        cancel.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("cancelled import must finish without waiting for another mutation")
            .err()
            .unwrap();
        assert_eq!(result.code, "CANCELLED");
        assert!(manager.list().await.is_empty());
        assert!(!root.path().join("fixture.source/plugin.js").exists());
        drop(guard);
    }
}
