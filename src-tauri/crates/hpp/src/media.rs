//! Loopback transport. Every resource retains its provider and request environment.
use crate::{
    network::{self, Network},
    store::now,
    wire::Transfer,
    PluginError, PluginManager, Result,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, Response, StatusCode},
    routing::get,
    Router,
};
use base64::Engine;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const MAX_RESOURCE: usize = 8 * 1024 * 1024;
static MEDIA_TRANSFERS: once_cell::sync::Lazy<Arc<tokio::sync::Semaphore>> =
    once_cell::sync::Lazy::new(|| Arc::new(tokio::sync::Semaphore::new(6)));
static IMAGE_TRANSFERS: once_cell::sync::Lazy<Arc<tokio::sync::Semaphore>> =
    once_cell::sync::Lazy::new(|| Arc::new(tokio::sync::Semaphore::new(2)));
mod preload;
mod refresh;
mod subtitles;
#[derive(Default)]
pub struct MediaSessions {
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    base: tokio::sync::OnceCell<String>,
    shutdown: CancellationToken,
    preload_gate: tokio::sync::Mutex<()>,
    foreground: Arc<AtomicUsize>,
    refreshing: Mutex<HashMap<String, tokio::sync::watch::Sender<Option<Result<PreparedMedia>>>>>,
    refresh_waiters: Mutex<HashMap<String, CancellationToken>>,
}
struct Foreground(Arc<AtomicUsize>);
impl Drop for Foreground {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Session {
    version: u64,
    refresh_input: Option<refresh::Input>,
    prepared: Mutex<Option<PreparedMedia>>,
    playback: AtomicBool,
    providers: BTreeMap<String, (Arc<Network>, String)>,
    cancel: CancellationToken,
    resources: Mutex<Vec<Resource>>,
    expires_at: Option<u64>,
    cache: preload::Cache,
}
#[derive(Clone, PartialEq)]
struct Resource {
    provider: String,
    url: String,
    headers: Value,
    kind: String,
    subtitle_format: Option<String>,
    transforms: Value,
    credential_origin: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedMedia {
    pub version: u64,
    pub session_id: String,
    pub url: String,
    pub format: String,
    pub provider_id: String,
    pub playback: Value,
    /// Display-only choices; never expose provider URLs, headers or transform keys.
    pub choices: Value,
}
#[derive(Clone, Default)]
pub struct PlaybackSelection {
    pub stream_index: usize,
    pub resolver_id: Option<String>,
    pub resolver_host: Option<String>,
    pub resolver_servers: BTreeMap<String, String>,
    pub resolver_stream_index: usize,
}
fn stream_choices(playback: &Value) -> Value {
    json!(playback["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, stream)| {
            let mut item = json!({"index": index});
            for key in [
                "label", "height", "bitrate", "codec", "kind", "format", "serverId",
            ] {
                if let Some(value) = stream.get(key) {
                    item[key] = value.clone();
                }
            }
            item
        })
        .collect::<Vec<_>>())
}
impl MediaSessions {
    pub fn has_playback(&self) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .any(|s| !s.cancel.is_cancelled() && s.playback.load(Ordering::Acquire))
    }
    pub fn activity(&self, id: &str, playing: bool, version: Option<u64>) -> Result<()> {
        let all = self.sessions.lock().unwrap();
        let session = all
            .get(id)
            .ok_or_else(|| PluginError::new("NOT_FOUND", "媒体会话已释放"))?;
        if version.is_none_or(|v| v == session.version) {
            session.playback.store(playing, Ordering::Release);
        }
        Ok(())
    }
    pub fn cancel_preload(&self, id: &str) {
        if let Some(s) = self.sessions.lock().unwrap().get(id) {
            s.cache.cancel();
        }
    }
    pub fn release_plugin(&self, id: &str) {
        self.sessions.lock().unwrap().retain(|_, s| {
            if s.providers.contains_key(id) {
                s.cancel.cancel();
                false
            } else {
                true
            }
        });
    }
    pub fn release(&self, id: &str) {
        if let Some(s) = self.sessions.lock().unwrap().remove(id) {
            s.cancel.cancel();
        }
    }
    pub async fn start(manager: &Arc<PluginManager>) -> Result<String> {
        let weak = Arc::downgrade(manager);
        let shutdown = manager.sessions.shutdown.clone();
        manager
            .sessions
            .base
            .get_or_try_init(|| async move {
                let listener =
                    tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
                let base = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
                let router = Router::new()
                    .route(
                        "/hpp/:session/:resource",
                        get(serve_resource).head(serve_resource).options(preflight),
                    )
                    .with_state(weak);
                tokio::spawn(async move {
                    let _ = axum::serve(listener, router)
                        .with_graceful_shutdown(shutdown.cancelled_owned())
                        .await;
                });
                Ok::<_, PluginError>(base)
            })
            .await
            .cloned()
    }
}
impl Drop for MediaSessions {
    fn drop(&mut self) {
        self.shutdown.cancel();
        for s in self.sessions.lock().unwrap().values() {
            s.cancel.cancel();
        }
    }
}
impl Session {
    fn register(&self, resource: Resource) -> Result<usize> {
        let mut all = self.resources.lock().unwrap();
        if let Some(i) = all.iter().position(|r| r == &resource) {
            return Ok(i);
        }
        if all.len() >= 50_000 {
            return Err(PluginError::invalid("媒体会话资源数量超限"));
        }
        all.push(resource);
        Ok(all.len() - 1)
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn resolver_matches_host_patterns() {
        assert!(super::matches(
            "api.example.com",
            "https://api.example.com:8443/a?x=1"
        ));
        assert!(super::matches("*.example.com", "https://a.b.example.com/a"));
        assert!(!super::matches("*.example.com", "https://example.com/a"));
        assert!(!super::matches(
            "example.com",
            "https://example.com.evil.test/"
        ));
        assert!(super::matches("*", "https://any.example/a"));
        assert!(super::matches(
            "API.Example.com",
            "https://api.example.com/a"
        ));
        assert!(!super::matches("*", "not a URL"));
        assert!(super::matches(
            "page.example",
            "https://PAGE.EXAMPLE./watch"
        ));
        assert!(super::matches("*.example", "https://page.example./watch"));
        assert!(super::matches("[::1]", "http://[::1]/watch"));
    }
}

fn origin(raw: &str) -> Result<String> {
    Ok(url::Url::parse(raw)
        .map_err(|_| PluginError::invalid("媒体 URL 无效"))?
        .origin()
        .ascii_serialization())
}
fn effective_headers(meta: &crate::model::Meta, extra: &Value) -> Result<Value> {
    let mut h = network::headers(&json!(meta.default_headers))?;
    h.extend(network::headers(extra)?);
    Ok(json!(h
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
        .collect::<BTreeMap<_, _>>()))
}
fn non_credentials(headers: &Value) -> Value {
    let mut v = headers.clone();
    if let Some(o) = v.as_object_mut() {
        o.retain(|k, _| {
            !k.eq_ignore_ascii_case("cookie") && !k.eq_ignore_ascii_case("authorization")
        });
    }
    v
}
fn resource(provider: &str, stream: &Value, headers: Value, kind: &str) -> Result<Resource> {
    let url = stream["url"]
        .as_str()
        .ok_or_else(|| PluginError::invalid("资源缺少 URL"))?
        .to_string();
    Ok(Resource {
        provider: provider.into(),
        credential_origin: origin(&url)?,
        url,
        headers,
        kind: kind.into(),
        subtitle_format: (kind == "subtitle").then(|| stream["format"].as_str().unwrap().into()),
        transforms: stream["transforms"].clone(),
    })
}
fn matches(pattern: &str, value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let Ok(host) = crate::resolver_preferences::hostname(host) else {
        return false;
    };
    if pattern == "*" {
        true
    } else if let Some(domain) = pattern.strip_prefix("*.") {
        let Ok(domain) = crate::resolver_preferences::hostname(domain) else {
            return false;
        };
        host.len() > domain.len() + 1 && host.ends_with(&format!(".{domain}"))
    } else {
        crate::resolver_preferences::hostname(pattern).is_ok_and(|pattern| host == pattern)
    }
}
impl PluginManager {
    pub async fn prepare_assets(
        self: &Arc<Self>,
        plugin_id: &str,
        assets: Vec<Value>,
    ) -> Result<Value> {
        if assets.len() > 200 {
            return Err(PluginError::invalid("图片批次过大"));
        }
        let base = MediaSessions::start(self).await?;
        let meta = self.meta(plugin_id).await?;
        let providers = BTreeMap::from([(
            plugin_id.into(),
            (
                self.network(plugin_id).await?,
                self.revision(plugin_id).await?,
            ),
        )]);
        let session = Arc::new(Session {
            version: 0,
            refresh_input: None,
            prepared: Mutex::new(None),
            playback: AtomicBool::new(false),
            providers,
            cancel: CancellationToken::new(),
            resources: Mutex::new(vec![]),
            expires_at: None,
            cache: Default::default(),
        });
        let id = uuid::Uuid::new_v4().to_string();
        let mut urls = vec![];
        for asset in assets {
            let asset = crate::model::resource(asset, meta.allow_insecure_http)?;
            let mut asset = if let Some(url) = asset.as_str() {
                json!({"url":url})
            } else {
                asset
            };
            asset["transforms"] = json!({"image":asset["transform"]});
            let index = session.register(resource(
                plugin_id,
                &asset,
                effective_headers(&meta, &asset["headers"])?,
                "image",
            )?)?;
            urls.push(format!("{base}/hpp/{id}/{index}"));
        }
        let mut all = self.sessions.sessions.lock().unwrap();
        if all.len() >= 64 {
            return Err(PluginError::new("RATE_LIMITED", "媒体会话数量超限"));
        }
        all.insert(id.clone(), session);
        Ok(json!({"sessionId":id,"urls":urls}))
    }
    pub async fn prepare_media(
        self: &Arc<Self>,
        plugin_id: &str,
        params: Value,
        stream_index: usize,
        resolver_id: Option<String>,
        request_id: String,
    ) -> Result<PreparedMedia> {
        self.prepare_media_selected(
            plugin_id,
            params,
            PlaybackSelection {
                stream_index,
                resolver_id,
                ..Default::default()
            },
            request_id,
        )
        .await
    }
    pub async fn prepare_media_selected(
        self: &Arc<Self>,
        plugin_id: &str,
        params: Value,
        selection: PlaybackSelection,
        request_id: String,
    ) -> Result<PreparedMedia> {
        self.prepare_content_selected(
            plugin_id,
            "media.resolvePlayback",
            params,
            selection,
            request_id,
        )
        .await
    }
    pub async fn prepare_content_selected(
        self: &Arc<Self>,
        plugin_id: &str,
        method: &str,
        params: Value,
        selection: PlaybackSelection,
        request_id: String,
    ) -> Result<PreparedMedia> {
        let _playback = self.playback_gate.read().await;
        if !matches!(
            method,
            "media.resolvePlayback" | "live.resolve" | "articles.getDetail"
        ) {
            return Err(PluginError::invalid("未知播放能力"));
        }
        let budget = self
            .meta(plugin_id)
            .await?
            .budget(if method == "articles.getDetail" {
                "media.resolvePlayback"
            } else {
                method
            });
        let result = tokio::time::timeout(
            budget,
            self.prepare_inner(
                plugin_id,
                method,
                params,
                selection,
                request_id.clone(),
                None,
            ),
        )
        .await;
        if result.is_err() {
            self.cancel(&request_id).await;
        }
        result.unwrap_or_else(|_| Err(PluginError::timeout()))
    }
    async fn prepare_inner(
        self: &Arc<Self>,
        plugin_id: &str,
        method: &str,
        params: Value,
        selection: PlaybackSelection,
        request_id: String,
        replacement: Option<(String, Arc<Session>)>,
    ) -> Result<PreparedMedia> {
        let mut refresh_input = refresh::Input {
            plugin_id: plugin_id.into(),
            method: method.into(),
            params: params.clone(),
            selection: selection.clone(),
        };
        let base = MediaSessions::start(self).await?;
        let mut providers = BTreeMap::new();
        providers.insert(
            plugin_id.to_string(),
            (
                self.network(plugin_id).await?,
                self.revision(plugin_id).await?,
            ),
        );
        let mut playback = self
            .call(
                plugin_id,
                method,
                if method == "articles.getDetail" {
                    json!({"id":params["articleId"]})
                } else {
                    params.clone()
                },
                request_id.clone(),
            )
            .await?;
        if method == "articles.getDetail" {
            // Article metadata never becomes media-session transport configuration.
            playback = crate::model::playback(
                json!({"streams":playback["streams"]}),
                self.meta(plugin_id).await?.allow_insecure_http,
                false,
            )?;
        }
        let mut selected = playback["streams"]
            .get(selection.stream_index)
            .cloned()
            .ok_or_else(|| PluginError::invalid("播放流序号不存在"))?;
        let mut provider = plugin_id.to_string();
        let mut choices = json!({"source": {
            "pluginId": plugin_id, "servers": playback["servers"].as_array().cloned().unwrap_or_default(),
            "streams": stream_choices(&playback), "streamIndex": selection.stream_index,
            "serverId": selected.get("serverId").or_else(|| params.get("serverId"))
        }});
        let mut subtitle_providers = playback["subtitles"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|s| (provider.clone(), s))
            .collect::<Vec<_>>();
        if selected["kind"] == "page" {
            let url = selected["url"].as_str().unwrap();
            let host = crate::resolver_preferences::hostname(
                url::Url::parse(url).unwrap().host_str().unwrap(),
            )?;
            // An explicit provider declared by the stream takes precedence over a saved UI preference.
            let forced = selected["resolverId"].as_str().is_some();
            let wanted = selected["resolverId"]
                .as_str()
                .map(String::from)
                .or_else(|| {
                    selection
                        .resolver_id
                        .filter(|_| selection.resolver_host.as_ref().is_none_or(|h| h == &host))
                });
            let installed = self
                .list()
                .await
                .into_iter()
                .filter(|p| {
                    p.enabled
                        && p.meta.kind == "resolver"
                        && matches!(p.status.as_str(), "ready" | "unloaded")
                })
                .collect::<Vec<_>>();
            let matching = installed
                .iter()
                .filter(|p| p.meta.matches.iter().any(|m| matches(m, url)))
                .collect::<Vec<_>>();
            let offered = if matching.is_empty() {
                installed.iter().collect::<Vec<_>>()
            } else {
                matching.clone()
            };
            choices["resolverRoute"] = json!({"host":host,"forced":forced,
                "resolvers":offered.iter().map(|p|json!({"id":p.meta.id,"name":p.meta.name})).collect::<Vec<_>>()});
            let saved = self.resolver_preferences.get().get(&host).cloned();
            let resolver = if let Some(id) = wanted {
                installed.iter().find(|p| p.meta.id == id).ok_or_else(|| {
                    let mut e = PluginError::new("UNSUPPORTED", "指定解析器不可用");
                    e.playback_choices = Some(choices.clone());
                    e
                })?
            } else if let Some(saved) = matching.iter().find(|p| saved.as_ref() == Some(&p.meta.id))
            {
                saved
            } else if matching.len() == 1 {
                matching[0]
            } else {
                let mut e = PluginError::new(
                    "INTERACTION_REQUIRED",
                    if matching.is_empty() {
                        "该来源需要安装或手动选择解析器"
                    } else {
                        "请选择匹配的解析器"
                    },
                );
                e.details = Some(
                    json!({"resolverHost":host,"resolvers":offered.iter().map(|p|json!({"id":p.meta.id,"name":p.meta.name})).collect::<Vec<_>>()}),
                );
                e.playback_choices = Some(choices.clone());
                return Err(e);
            };
            provider = resolver.meta.id.clone();
            choices["resolverRoute"]["selectedId"] = json!(provider);
            providers.insert(
                provider.clone(),
                (
                    self.network(&provider).await?,
                    self.revision(&provider).await?,
                ),
            );
            let source_headers =
                effective_headers(&self.meta(plugin_id).await?, &selected["headers"])?;
            let resolver_server_id = selection.resolver_servers.get(&provider).cloned();
            let mut resolver_params = json!({"url":url,"headers":non_credentials(&source_headers),"reason":params["reason"],"codecs":params["codecs"]});
            if let Some(height) = params.get("maxHeight") {
                resolver_params["maxHeight"] = height.clone();
            }
            if let Some(server) = &resolver_server_id.as_ref().filter(|s| !s.is_empty()) {
                resolver_params["serverId"] = json!(server);
            }
            let resolved = self
                .call(&provider, "resolver.resolve", resolver_params, request_id)
                .await
                .map_err(|mut e| {
                    e.playback_choices = Some(choices.clone());
                    e
                })?;
            selected = resolved["streams"]
                .get(selection.resolver_stream_index)
                .cloned()
                .ok_or_else(|| PluginError::internal("解析器没有可播放流"))?;
            choices["resolver"] = json!({"pluginId":provider,"name":resolver.meta.name,
                "servers":resolved["servers"].as_array().cloned().unwrap_or_default(),
                "streams":stream_choices(&resolved),"streamIndex":selection.resolver_stream_index,
                "serverId":selected.get("serverId").cloned().or_else(|| resolver_server_id.filter(|s| !s.is_empty()).map(Value::String))
            });
            subtitle_providers.extend(
                resolved["subtitles"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| (provider.clone(), s)),
            );
            for key in ["durationSec", "resumeSec", "skip"] {
                if let Some(v) = resolved.get(key) {
                    playback[key] = v.clone();
                }
            }
            playback["expiresAt"] = json!([
                playback["expiresAt"].as_u64(),
                resolved["expiresAt"].as_u64()
            ]
            .into_iter()
            .flatten()
            .min());
            playback["streams"] = resolved["streams"].clone();
        }
        if selected["kind"] == "external" {
            choices["external"] = json!({"url":selected["url"],"app":selected["external"]["app"],"mime":selected["external"]["mime"],"expiresAt":playback["expiresAt"]});
            let mut e = PluginError::new("INTERACTION_REQUIRED", "该流需要在外部应用中打开");
            e.playback_choices = Some(choices);
            return Err(e);
        }
        let format = selected["format"].as_str().unwrap_or("").to_string();
        if !matches!(format.as_str(), "hls" | "progressive") {
            let mut e = PluginError::new("UNSUPPORTED", "当前播放器尚不支持该媒体格式");
            e.playback_choices = Some(choices);
            return Err(e);
        }
        if format == "progressive"
            && selected["transforms"]
                .as_object()
                .is_some_and(|v| !v.is_empty())
        {
            return Err(PluginError::new(
                "UNSUPPORTED",
                "不支持对完整普通视频执行字节变换",
            ));
        }
        if let Some(server) = choices["source"]["serverId"].as_str() {
            refresh_input.params["serverId"] = json!(server);
        }
        if let Some(resolver) = choices["resolver"]["pluginId"].as_str() {
            refresh_input.selection.resolver_id = Some(resolver.into());
            refresh_input.selection.resolver_host =
                choices["resolverRoute"]["host"].as_str().map(String::from);
            if let Some(server) = choices["resolver"]["serverId"].as_str() {
                refresh_input
                    .selection
                    .resolver_servers
                    .insert(resolver.into(), server.into());
            }
        }
        let session = Arc::new(Session {
            version: replacement.as_ref().map_or(0, |(_, old)| old.version + 1),
            refresh_input: Some(refresh_input),
            prepared: Mutex::new(None),
            playback: AtomicBool::new(params["reason"] != "preload"),
            providers,
            cancel: CancellationToken::new(),
            resources: Mutex::new(vec![]),
            expires_at: playback["expiresAt"].as_u64(),
            cache: Default::default(),
        });
        let root = session.register(resource(
            &provider,
            &selected,
            effective_headers(&self.meta(&provider).await?, &selected["headers"])?,
            if format == "hls" { "playlist" } else { "media" },
        )?)?;
        let id = replacement
            .as_ref()
            .map_or_else(|| uuid::Uuid::new_v4().to_string(), |(id, _)| id.clone());
        let mut subtitles = vec![];
        let default_track = subtitle_providers
            .iter()
            .position(|(owner, track)| {
                owner == &provider && track["format"] != "ass" && track["default"] == true
            })
            .or_else(|| {
                subtitle_providers
                    .iter()
                    .position(|(_, track)| track["format"] != "ass" && track["default"] == true)
            });
        for (track_index, (owner, mut track)) in subtitle_providers.into_iter().enumerate() {
            if track["format"] == "ass" {
                continue;
            }
            let index = session.register(resource(
                &owner,
                &track,
                effective_headers(&self.meta(&owner).await?, &track["headers"])?,
                "subtitle",
            )?)?;
            track["url"] = json!(format!("{base}/hpp/{id}/{index}?v={}", session.version));
            if track["format"] == "srt" {
                track["format"] = json!("vtt");
            }
            if track.get("default").is_some() {
                track["default"] = json!(default_track == Some(track_index));
            }
            if let Some(o) = track.as_object_mut() {
                o.remove("headers");
            }
            subtitles.push(track);
        }
        playback["subtitles"] = json!(subtitles);
        playback.as_object_mut().unwrap().remove("streams");
        let mut all = self.sessions.sessions.lock().unwrap();
        if replacement.is_none() && all.len() >= 64 {
            return Err(PluginError::new(
                "RATE_LIMITED",
                "媒体会话数量超限，请关闭闲置播放",
            ));
        }
        if let Some((_, old)) = &replacement {
            if old.cancel.is_cancelled()
                || !all
                    .get(&id)
                    .is_some_and(|current| Arc::ptr_eq(current, old))
            {
                return Err(PluginError::cancelled());
            }
            session
                .playback
                .store(old.playback.load(Ordering::Acquire), Ordering::Release);
        }
        let prepared = PreparedMedia {
            version: session.version,
            session_id: id.clone(),
            url: format!("{base}/hpp/{id}/{root}?v={}", session.version),
            format,
            provider_id: provider,
            playback,
            choices,
        };
        *session.prepared.lock().unwrap() = Some(prepared.clone());
        all.insert(id, session);
        if let Some((_, old)) = replacement {
            old.cancel.cancel();
            old.cache.cancel();
        }
        Ok(prepared)
    }
}
async fn preflight() -> Response<Body> {
    cors(Response::builder().status(204).body(Body::empty()).unwrap())
}
fn cors(mut r: Response<Body>) -> Response<Body> {
    for (k, v) in [
        ("access-control-allow-origin", "*"),
        ("access-control-allow-methods", "GET, HEAD, OPTIONS"),
        ("access-control-allow-headers", "Range, If-Range"),
        (
            "access-control-expose-headers",
            "Content-Range, Accept-Ranges, Content-Length",
        ),
    ] {
        r.headers_mut()
            .insert(k, axum::http::HeaderValue::from_static(v));
    }
    r
}
async fn serve_resource(
    State(weak): State<Weak<PluginManager>>,
    Path((id, index)): Path<(String, usize)>,
    Query(query): Query<HashMap<String, String>>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    let result = async {
        let manager = weak.upgrade().ok_or_else(PluginError::cancelled)?;
        let session = manager
            .sessions
            .sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| PluginError::new("NOT_FOUND", "媒体会话已释放"))?;
        if session.cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        let version = query.get("v").map(|v| v.parse::<u64>()).transpose().map_err(|_| PluginError::invalid("媒体版本无效"))?.unwrap_or(0);
        if version != session.version { return Err(PluginError::cancelled()); }
        let pending = manager.sessions.refreshing.lock().unwrap().get(&id).map(|task| task.subscribe());
        if let Some(mut pending) = pending {
            loop {
                if let Some(result) = pending.borrow().clone() {
                    result?;
                    return Err(PluginError::cancelled());
                }
                tokio::select! {
                    _ = session.cancel.cancelled() => return Err(PluginError::cancelled()),
                    changed = pending.changed() => changed.map_err(|_| PluginError::internal("媒体刷新缺少结果"))?,
                }
            }
        }
        for (owner, (_, revision)) in &session.providers {
            if !manager.revision(owner).await.is_ok_and(|v| v == *revision) {
                return Err(PluginError::cancelled());
            }
        }
        if session.expires_at.is_some_and(|t| t <= now()) {
            return Err(PluginError::new(
                "AUTH_EXPIRED",
                "播放地址已过期，请刷新播放",
            ));
        }
        let resource = session
            .resources
            .lock()
            .unwrap()
            .get(index)
            .cloned()
            .ok_or_else(|| PluginError::new("NOT_FOUND", "媒体资源不存在"))?;
        let (gate, foreground) = if resource.kind == "image" {
            (&*IMAGE_TRANSFERS, None)
        } else {
            // Foreground media can interrupt speculative I/O, not just jump a queue.
            let sessions = manager.sessions.sessions.lock().unwrap();
            manager.sessions.foreground.fetch_add(1, Ordering::AcqRel);
            for candidate in sessions.values() {
                candidate.cache.cancel();
            }
            (&*MEDIA_TRANSFERS, Some(Foreground(manager.sessions.foreground.clone())))
        };
        let permit = tokio::select! {
            _ = session.cancel.cancelled() => return Err(PluginError::cancelled()),
            value = gate.clone().acquire_owned() => value.map_err(|_| PluginError::cancelled())?,
        };
        let response = proxy(&manager, &session, &id, resource, method, headers).await?;
        let (parts, body) = response.into_parts();
        // Keep the permit for the streamed body, not only until response headers.
        let stream = body.into_data_stream().map(move |chunk| {
            let _keep = (&permit, &foreground);
            chunk
        });
        Ok(Response::from_parts(parts, Body::from_stream(stream)))
    }
    .await;
    cors(result.unwrap_or_else(|e| {
        Response::builder()
            .status(match e.code.as_str() {
                "NOT_FOUND" => 404,
                "CANCELLED" => 410,
                "AUTH_EXPIRED" => 401,
                "UNSUPPORTED" => 422,
                _ => 502,
            })
            .header("content-type", "text/plain; charset=utf-8")
            .body(Body::from(e.message))
            .unwrap()
    }))
}
async fn proxy(
    manager: &Arc<PluginManager>,
    session: &Arc<Session>,
    id: &str,
    r: Resource,
    method: Method,
    incoming: HeaderMap,
) -> Result<Response<Body>> {
    if let Some(response) = preload::cached(manager, session, id, &r, &method, &incoming).await? {
        return Ok(response);
    }
    proxy_upstream(manager, session, id, r, method, incoming, MAX_RESOURCE).await
}
async fn proxy_upstream(
    manager: &Arc<PluginManager>,
    session: &Arc<Session>,
    id: &str,
    r: Resource,
    method: Method,
    incoming: HeaderMap,
    buffer_limit: usize,
) -> Result<Response<Body>> {
    let network = &session.providers[&r.provider].0;
    let transform = r.transforms.get(&r.kind).filter(|v| !v.is_null());
    let buffered =
        r.kind == "playlist" || r.subtitle_format.as_deref() == Some("srt") || transform.is_some();
    let mut headers = network::headers(&r.headers)?;
    if origin(&r.url)? != r.credential_origin {
        headers.remove("authorization");
        headers.remove("cookie");
    }
    if !buffered {
        for k in ["range", "if-range"] {
            if let Some(v) = incoming.get(k) {
                headers.insert(k, v.clone());
            }
        }
    }
    let upstream = tokio::time::timeout(
        Duration::from_secs(20),
        network.request(
            &r.url,
            if buffered {
                Method::GET
            } else {
                method.clone()
            },
            headers,
            None,
            true,
            true,
            network::Queue::Media,
            &session.cancel,
        ),
    )
    .await
    .map_err(|_| PluginError::timeout())??;
    let status = upstream.status();
    let upstream_headers = upstream.headers().clone();
    let final_url = upstream.url().clone();
    if buffered && !status.is_success() {
        return Ok(Response::builder()
            .status(status)
            .body(Body::empty())
            .unwrap());
    }
    if !buffered {
        let mut response = Response::builder().status(status);
        for k in [
            "content-type",
            "content-length",
            "content-range",
            "accept-ranges",
            "etag",
            "last-modified",
        ] {
            if let Some(v) = upstream_headers.get(k) {
                response = response.header(k, v);
            }
        }
        let cancel = session.cancel.clone();
        let stream = upstream.bytes_stream();
        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from_stream(stream.take_until(cancel.cancelled_owned()))
        };
        return Ok(response.body(body).unwrap());
    }
    let mut upstream = upstream;
    let mut data = vec![];
    loop {
        let chunk = tokio::select! {_=session.cancel.cancelled()=>return Err(PluginError::cancelled()),v=tokio::time::timeout(Duration::from_secs(20),upstream.chunk())=>v.map_err(|_|PluginError::timeout())?.map_err(|_|PluginError::new("NETWORK","媒体读取失败"))?};
        let Some(chunk) = chunk else {
            break;
        };
        if data.len() + chunk.len() > buffer_limit {
            return Err(PluginError::invalid("媒体资源超过缓冲预算"));
        }
        data.extend_from_slice(&chunk);
    }
    let mut content_type = upstream_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    if let Some(t) = transform {
        match t["type"].as_str() {
            Some("aes-cbc") => {
                let b64 = |key: &str| {
                    base64::engine::general_purpose::STANDARD
                        .decode(t[key].as_str().unwrap_or(""))
                        .map_err(|_| PluginError::invalid("AES 资源参数需要 Base64"))
                };
                data = crate::native::aes_transform(
                    &json!({"mode":"cbc","padding":t["padding"].as_str().unwrap_or("pkcs7")}),
                    b64("key")?,
                    b64("iv")?,
                    data,
                    vec![],
                    false,
                )?;
            }
            Some("plugin") => {
                let params = Transfer::Object(BTreeMap::from([
                    ("url".into(), Transfer::String(final_url.to_string())),
                    ("kind".into(), Transfer::String(r.kind.clone())),
                    ("name".into(), Transfer::from_json(t["name"].clone())),
                    ("state".into(), Transfer::from_json(t["state"].clone())),
                    ("data".into(), Transfer::Bytes(data)),
                    ("contentType".into(), Transfer::String(content_type.clone())),
                ]));
                let call = manager.call_transfer(
                    &r.provider,
                    "transformResource",
                    params,
                    uuid::Uuid::new_v4().to_string(),
                );
                let result = tokio::select! {_=session.cancel.cancelled()=>return Err(PluginError::cancelled()),v=call=>v?};
                data = result
                    .field("data")
                    .ok_or_else(|| PluginError::internal("变换缺少 data"))?
                    .bytes()?;
                if let Some(v) = result.json()["contentType"].as_str() {
                    content_type = v.into();
                }
            }
            _ => return Err(PluginError::new("UNSUPPORTED", "未知资源变换")),
        }
    }
    if r.kind == "playlist" {
        let text = std::str::from_utf8(&data)
            .map_err(|_| PluginError::new("UPSTREAM_CHANGED", "HLS 清单不是 UTF-8"))?;
        if !text.trim_start_matches('\u{feff}').starts_with("#EXTM3U") {
            return Err(PluginError::new("UPSTREAM_CHANGED", "响应不是 HLS 清单"));
        }
        if r.transforms.as_object().is_some_and(|t| !t.is_empty())
            && (text.contains("#EXT-X-BYTERANGE") || text.contains("BYTERANGE="))
        {
            return Err(PluginError::new(
                "UNSUPPORTED",
                "当前版本尚不支持带字节变换的 HLS BYTERANGE",
            ));
        }
        let base = manager.sessions.base.get().unwrap();
        data = rewrite_hls(text, |raw, kind| {
            let url = final_url
                .join(raw)
                .map_err(|_| PluginError::invalid("HLS 子资源 URL 无效"))?;
            network.target(url.as_str())?;
            let index = session.register(Resource {
                url: url.to_string(),
                kind: kind.into(),
                ..r.clone()
            })?;
            Ok(format!("{base}/hpp/{id}/{index}?v={}", session.version))
        })?
        .into_bytes();
        content_type = "application/vnd.apple.mpegurl".into();
    }
    if r.subtitle_format.as_deref() == Some("srt") {
        data = subtitles::srt_to_vtt(&data, &content_type)?;
        content_type = "text/vtt; charset=utf-8".into();
    }
    bounded_response(data, &content_type, method, &incoming)
}
fn bounded_response(
    data: Vec<u8>,
    content_type: &str,
    method: Method,
    headers: &HeaderMap,
) -> Result<Response<Body>> {
    let size = data.len();
    let mut response = Response::builder()
        .header("content-type", content_type)
        .header("accept-ranges", "bytes");
    let range = headers.get("range").and_then(|v| v.to_str().ok());
    let (start, end, status) = if let Some(range) = range {
        let parsed = (|| {
            let (start, end) = range.strip_prefix("bytes=")?.split_once('-')?;
            if start.is_empty() {
                let suffix = end.parse::<usize>().ok()?;
                if suffix == 0 {
                    return None;
                }
                Some((size.saturating_sub(suffix), size.checked_sub(1)?))
            } else {
                let start = start.parse::<usize>().ok()?;
                let end = if end.is_empty() {
                    size.checked_sub(1)?
                } else {
                    end.parse::<usize>().ok()?.min(size.checked_sub(1)?)
                };
                if start > end {
                    return None;
                }
                Some((start, end))
            }
        })();
        let Some((s, e)) = parsed else {
            return Ok(response
                .status(416)
                .header("content-range", format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap());
        };
        response = response.header("content-range", format!("bytes {s}-{e}/{size}"));
        (s, e + 1, StatusCode::PARTIAL_CONTENT)
    } else {
        (0, size, StatusCode::OK)
    };
    Ok(response
        .status(status)
        .header("content-length", end - start)
        .body(if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(data[start..end].to_vec())
        })
        .unwrap())
}
fn rewrite_hls(
    text: &str,
    mut rewrite: impl FnMut(&str, &str) -> Result<String>,
) -> Result<String> {
    let uri = regex::Regex::new(r#"URI="([^"]*)""#).unwrap();
    let mut output = String::new();
    let mut next_playlist = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let kind = if trimmed.starts_with("#EXT-X-KEY:")
                || trimmed.starts_with("#EXT-X-SESSION-KEY:")
            {
                "key"
            } else if trimmed.starts_with("#EXT-X-MEDIA:")
                || trimmed.starts_with("#EXT-X-I-FRAME-STREAM-INF:")
                || trimmed.starts_with("#EXT-X-RENDITION-REPORT:")
            {
                "playlist"
            } else {
                "segment"
            };
            let mut last = 0;
            for c in uri.captures_iter(line) {
                let m = c.get(1).unwrap();
                output.push_str(&line[last..m.start()]);
                output.push_str(&rewrite(m.as_str(), kind)?);
                last = m.end();
            }
            output.push_str(&line[last..]);
            if trimmed.starts_with("#EXT-X-STREAM-INF:") {
                next_playlist = true;
            }
        } else if !trimmed.is_empty() {
            output.push_str(&rewrite(
                trimmed,
                if next_playlist { "playlist" } else { "segment" },
            )?);
            next_playlist = false;
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    Ok(output)
}
