use crate::{
    model::Meta,
    network_policy,
    store::{atomic_write, now},
    wire::Transfer,
    PluginError, Result,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Method, Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use url::Url;

mod api;
pub use api::Queue;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedCookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    host_only: bool,
    secure: bool,
    http_only: bool,
    expires_at: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
struct WebLoginSession {
    generation: String,
    cookies: Vec<SavedCookie>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct CookieJars {
    ordinary: Vec<SavedCookie>,
    // Network belongs to a single plugin. URL origins include the scheme and port.
    web_logins: HashMap<String, WebLoginSession>,
}

impl CookieJars {
    fn len(&self) -> usize {
        self.ordinary.len()
            + self
                .web_logins
                .values()
                .map(|s| s.cookies.len())
                .sum::<usize>()
    }
}

pub struct Network {
    pub meta: Meta,
    pub strict_override: Option<bool>,
    jar: Mutex<CookieJars>,
    jar_path: PathBuf,
    gate: Arc<tokio::sync::Semaphore>,
    hosts: Mutex<HashMap<String, Weak<tokio::sync::Semaphore>>>,
    inflight: Mutex<HashMap<String, Arc<api::Flight>>>,
    rate: Mutex<(f64, Instant)>,
    cache: Mutex<HashMap<String, CachedResponse>>,
    clients: tokio::sync::Mutex<HashMap<String, (Instant, reqwest::Client)>>,
    pub cooldown: Arc<Mutex<HashMap<String, Instant>>>,
}
#[derive(Clone)]
struct RedactedSetCookie;

struct CachedResponse {
    value: Transfer,
    expires: Instant,
    bytes: usize,
}
impl Network {
    pub fn new(
        meta: Meta,
        dir: PathBuf,
        strict: Option<bool>,
        cooldown: Arc<Mutex<HashMap<String, Instant>>>,
    ) -> Result<Self> {
        let jar_path = dir.join("cookies.json");
        let jar = if jar_path.exists() {
            #[derive(Deserialize)]
            #[serde(untagged)]
            enum DiskJar {
                Ordinary(Vec<SavedCookie>),
                WithWebLogins(CookieJars),
            }
            match serde_json::from_slice(&std::fs::read(&jar_path)?)
                .map_err(|_| PluginError::internal("Cookie 存储损坏"))?
            {
                DiskJar::Ordinary(ordinary) => CookieJars {
                    ordinary,
                    ..Default::default()
                },
                DiskJar::WithWebLogins(jars) => jars,
            }
        } else {
            CookieJars::default()
        };
        Ok(Self {
            meta,
            strict_override: strict,
            jar: Mutex::new(jar),
            jar_path,
            gate: Arc::new(tokio::sync::Semaphore::new(6)),
            hosts: Default::default(),
            inflight: Default::default(),
            rate: Mutex::new((15.0, Instant::now())),
            cache: Mutex::new(HashMap::new()),
            clients: Default::default(),
            cooldown,
        })
    }
    pub fn strict(&self) -> bool {
        self.strict_override
            .unwrap_or_else(network_policy::strict_security)
    }
    pub fn target(&self, raw: &str) -> Result<Url> {
        crate::model::url(raw, self.meta.allow_insecure_http, false)?;
        network_policy::parse_target_with_mode(raw, self.strict())
            .map_err(|e| PluginError::new("NETWORK", e))
    }
    fn matches(c: &SavedCookie, u: &Url) -> bool {
        let h = u.host_str().unwrap_or("");
        let path = u.path();
        let domain = h == c.domain || !c.host_only && h.ends_with(&format!(".{}", c.domain));
        let p = path == c.path
            || path.starts_with(&c.path)
                && (c.path.ends_with('/') || path.as_bytes().get(c.path.len()) == Some(&b'/'));
        domain
            && p
            && (!c.secure || u.scheme() == "https")
            && c.expires_at.is_none_or(|t| t > now())
    }
    fn save_jar(&self, jars: &CookieJars) -> Result<()> {
        let count = jars.len();
        if count > 3000 {
            return Err(PluginError::invalid("Cookie 数量超限"));
        }
        let encoded = if jars.web_logins.is_empty() {
            serde_json::to_vec(&jars.ordinary)
        } else {
            serde_json::to_vec(jars)
        }
        .map_err(|_| PluginError::internal("无法保存 Cookie"))?;
        atomic_write(&self.jar_path, &encoded)
    }
    fn parse_cookie(u: &Url, v: Value) -> Result<SavedCookie> {
        let host = u
            .host_str()
            .ok_or_else(|| PluginError::invalid("Cookie URL 缺少主机"))?;
        let domain = v["domain"]
            .as_str()
            .unwrap_or(host)
            .trim_start_matches('.')
            .to_ascii_lowercase();
        if domain != host && !host.ends_with(&format!(".{domain}")) {
            return Err(PluginError::invalid("Cookie domain 与 URL 不匹配"));
        }
        let host_only = v["hostOnly"].as_bool().unwrap_or(v.get("domain").is_none());
        if host_only && domain != host {
            return Err(PluginError::invalid("hostOnly Cookie 必须绑定当前主机"));
        }
        let c = SavedCookie {
            name: v["name"]
                .as_str()
                .ok_or_else(|| PluginError::invalid("Cookie 缺少名称"))?
                .into(),
            value: v["value"]
                .as_str()
                .ok_or_else(|| PluginError::invalid("Cookie 缺少值"))?
                .into(),
            domain,
            path: v["path"].as_str().unwrap_or("/").into(),
            host_only,
            secure: v["secure"].as_bool().unwrap_or(false),
            http_only: v["httpOnly"].as_bool().unwrap_or(false),
            expires_at: v["expiresAt"].as_u64(),
        };
        if c.name.is_empty()
            || c.name.len() + c.value.len() > 16384
            || c.name.chars().any(|c| c.is_control() || ";= ".contains(c))
            || c.value.chars().any(|c| c.is_control() || c == ';')
        {
            return Err(PluginError::invalid("Cookie 名称或值无效"));
        }
        Ok(c)
    }
    fn upsert(jar: &mut Vec<SavedCookie>, c: SavedCookie) {
        jar.retain(|x| !(x.name == c.name && x.domain == c.domain && x.path == c.path));
        if c.expires_at.is_none_or(|t| t > now()) {
            jar.push(c);
        }
    }
    // Called only by the native host after a successful, first-party login.
    pub(crate) fn replace_web_login(
        &self,
        initial: &str,
        final_url: &str,
        values: Vec<Value>,
    ) -> Result<()> {
        let initial = self.target(initial)?;
        let final_url = self.target(final_url)?;
        if initial.scheme() != "https" || initial.origin() != final_url.origin() {
            return Err(PluginError::invalid("网页登录必须在起始 HTTPS origin 完成"));
        }
        if values.len() > 3000 {
            return Err(PluginError::invalid("网页登录 Cookie 数量无效"));
        }
        let mut cookies = Vec::new();
        for value in values {
            // The native adapter must supply the original scope, including host-only.
            if !value["hostOnly"].is_boolean()
                || !value["secure"].is_boolean()
                || !value["httpOnly"].is_boolean()
                || !value["domain"].is_string()
                || !value["path"].as_str().is_some_and(|p| p.starts_with('/'))
                || !(value["expiresAt"].is_null() || value["expiresAt"].is_u64())
            {
                return Err(PluginError::invalid("缺少有效的网页登录 Cookie 属性"));
            }
            let c = Self::parse_cookie(&final_url, value)?;
            if !Self::matches(&c, &final_url) {
                return Err(PluginError::invalid("Cookie 与登录完成地址不匹配"));
            }
            Self::upsert(&mut cookies, c);
        }
        let mut guard = self.jar.lock().unwrap();
        let mut next = guard.clone();
        let origin = initial.origin().ascii_serialization();
        if !next.web_logins.contains_key(&origin) && next.web_logins.len() >= 3000 {
            return Err(PluginError::invalid("网页登录站点数量超限"));
        }
        next.web_logins.insert(
            origin,
            WebLoginSession {
                generation: uuid::Uuid::new_v4().to_string(),
                cookies,
            },
        );
        self.save_jar(&next)?;
        *guard = next;
        Ok(())
    }
    pub fn cookies(&self, op: &str, p: Value) -> Result<Value> {
        let u = p["url"].as_str().map(|s| self.target(s)).transpose()?;
        match op {
            "get" => {
                let u = u.ok_or_else(|| PluginError::invalid("缺少 Cookie URL"))?;
                Ok(json!(self
                    .jar
                    .lock()
                    .unwrap()
                    .ordinary
                    .iter()
                    .filter(|c| Self::matches(c, &u))
                    .collect::<Vec<_>>()))
            }
            "set" => {
                let c = Self::parse_cookie(
                    &u.ok_or_else(|| PluginError::invalid("缺少 Cookie URL"))?,
                    p["cookie"].clone(),
                )?;
                let mut guard = self.jar.lock().unwrap();
                let mut next = guard.clone();
                Self::upsert(&mut next.ordinary, c);
                self.save_jar(&next)?;
                *guard = next;
                Ok(Value::Null)
            }
            "clear" => {
                let mut jar = self.jar.lock().unwrap();
                let mut next = jar.clone();
                next.ordinary
                    .retain(|c| u.as_ref().is_some_and(|u| !Self::matches(c, u)));
                if let Some(url) = u {
                    next.web_logins.remove(&url.origin().ascii_serialization());
                } else {
                    next.web_logins.clear();
                }
                self.save_jar(&next)?;
                *jar = next;
                Ok(Value::Null)
            }
            _ => Err(PluginError::invalid("未知 Cookie 操作")),
        }
    }
    fn request_cookies(&self, url: &Url, attach: bool) -> (Option<String>, String) {
        let jars = self.jar.lock().unwrap();
        let session = jars.web_logins.get(&url.origin().ascii_serialization());
        let generation = session.map(|s| s.generation.clone());
        if !attach {
            return (generation, String::new());
        }
        let cookies = session.map_or(&jars.ordinary, |s| &s.cookies);
        let mut matched: Vec<_> = cookies.iter().filter(|c| Self::matches(c, url)).collect();
        matched.sort_by_key(|c| std::cmp::Reverse(c.path.len()));
        (
            generation,
            matched
                .iter()
                .map(|c| format!("{}={}", c.name, c.value))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
    fn receive_cookies(
        &self,
        u: &Url,
        headers: &mut HeaderMap,
        sent_generation: Option<&str>,
        store: bool,
    ) -> Result<bool> {
        if !headers.contains_key("set-cookie") {
            return Ok(false);
        }
        let mut guard = self.jar.lock().unwrap();
        let origin = u.origin().ascii_serialization();
        let current = guard.web_logins.get(&origin).map(|s| s.generation.as_str());
        let hidden = sent_generation.is_some() || current.is_some();
        // A late response after logout or account replacement cannot revive that session.
        if !store || sent_generation != current {
            if hidden {
                headers.remove("set-cookie");
            }
            return Ok(hidden);
        }
        let mut next = guard.clone();
        let total = next.len();
        let jar = next
            .web_logins
            .get_mut(&origin)
            .map_or(&mut next.ordinary, |s| &mut s.cookies);
        let capacity = 3000 - (total - jar.len());
        for h in headers.get_all("set-cookie") {
            if let Ok(s) = h.to_str() {
                if let Ok(c) = cookie::Cookie::parse(s.to_string()) {
                    let default_path = u
                        .path()
                        .rsplit_once('/')
                        .map(|(s, _)| if s.is_empty() { "/" } else { s })
                        .unwrap_or("/");
                    let mut v = json!({"name":c.name(),"value":c.value(),"path":c.path().unwrap_or(default_path),"secure":c.secure().unwrap_or(false),"httpOnly":c.http_only().unwrap_or(false)});
                    if let Some(d) = c.domain() {
                        v["domain"] = json!(d);
                    }
                    if let Some(age) = c.max_age() {
                        v["expiresAt"] = json!(if age.whole_seconds() <= 0 {
                            0
                        } else {
                            now().saturating_add(age.whole_milliseconds() as u64)
                        });
                    } else if let Some(t) = c.expires_datetime() {
                        v["expiresAt"] = json!((t.unix_timestamp().max(0) as u64) * 1000);
                    }
                    // Ignore individual invalid upstream cookies, as browsers do.
                    if let Ok(c) = Self::parse_cookie(u, v) {
                        Self::upsert(jar, c);
                        if jar.len() > capacity {
                            jar.pop();
                        }
                    }
                }
            }
        }
        if hidden {
            headers.remove("set-cookie");
        }
        self.save_jar(&next)?;
        *guard = next;
        Ok(hidden)
    }
    async fn rate_limit(&self, cancel: &CancellationToken) -> Result<()> {
        loop {
            {
                let mut r = self.rate.lock().unwrap();
                let elapsed = r.1.elapsed().as_secs_f64();
                r.0 = (r.0 + elapsed * 5.0).min(15.0);
                r.1 = Instant::now();
                if r.0 >= 1.0 {
                    r.0 -= 1.0;
                    return Ok(());
                }
            }
            tokio::select! {_=cancel.cancelled()=>return Err(PluginError::cancelled()),_=tokio::time::sleep(Duration::from_millis(50))=>{}}
        }
    }
    // A pool is bound to validated DNS answers and the effective network policy.
    // Revalidate periodically; never reuse a pool across proxy/security changes.
    async fn client(&self, target: &Url, cancel: &CancellationToken) -> Result<reqwest::Client> {
        let strict = self.strict();
        let key = format!(
            "{}:{}:{}",
            crate::proxy::generation(),
            strict,
            target.origin().ascii_serialization()
        );
        let mut pools = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            guard = self.clients.lock() => guard,
        };
        pools.retain(|_, (created, _)| created.elapsed() < Duration::from_secs(60));
        if let Some((_, client)) = pools.get(&key) {
            return Ok(client.clone());
        }
        let addrs = tokio::select! {
            _ = cancel.cancelled() => return Err(PluginError::cancelled()),
            value = network_policy::resolve_target(target, strict) => value.map_err(|e| PluginError::new("NETWORK", e))?,
        };
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(45))
            .pool_max_idle_per_host(6)
            .user_agent("HGhub/HPP-dev")
            .resolve_to_addrs(target.host_str().unwrap(), &addrs);
        let client = crate::proxy::configure(builder, &crate::proxy::snapshot())
            .map_err(|e| PluginError::new("NETWORK", e))?
            .build()
            .map_err(|_| PluginError::new("NETWORK", "无法创建 HTTP 客户端"))?;
        if pools.len() >= 32 {
            if let Some(oldest) = pools
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(key, _)| key.clone())
            {
                pools.remove(&oldest);
            }
        }
        pools.insert(key, (Instant::now(), client.clone()));
        Ok(client)
    }
    pub async fn request(
        &self,
        raw: &str,
        method: Method,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
        cookies: bool,
        follow: bool,
        queue: Queue,
        cancel: &CancellationToken,
    ) -> Result<Response> {
        let mut target = self.target(raw)?;
        let initial = target.origin();
        let mut explicit = headers;
        for hop in 0..=5 {
            if target.origin() != initial {
                explicit.remove("authorization");
                explicit.remove("cookie");
            }
            let outgoing = self.outgoing(
                target.clone(),
                method.clone(),
                explicit.clone(),
                body.clone(),
                cookies,
            )?;
            let response = self.send(outgoing, queue, cancel).await?;
            if let Some(next) =
                self.redirect(&target, response.status(), response.headers(), follow, hop)?
            {
                target = next;
            } else {
                return Ok(response);
            }
        }
        unreachable!("redirect rejects a sixth hop")
    }
    pub async fn fetch(
        self: &Arc<Self>,
        p: Transfer,
        cancel: CancellationToken,
    ) -> Result<Transfer> {
        let v = p.json();
        let effective = headers(&v["headers"])?;
        let method = v["method"].as_str().unwrap_or("GET");
        let retries = v["retry"].as_u64().unwrap_or(0).min(2);
        let eligible = method == "GET"
            && v["credentials"] == "omit"
            && !effective.contains_key("authorization")
            && !effective.contains_key("cookie")
            && v["cache"].is_object();
        let ttl = v["cache"]["ttlSec"]
            .as_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 86400.0);
        let stale = v["cache"]["staleIfErrorSec"]
            .as_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 604800.0);
        let mut key = v.clone();
        if let Some(o) = key.as_object_mut() {
            for k in ["cache", "retry", "timeoutMs"] {
                o.remove(k);
            }
            o.insert(
                "headers".into(),
                json!(effective
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.to_str().unwrap_or("")))
                    .collect::<std::collections::BTreeMap<_, _>>()),
            );
        }
        let key = format!("{}:{}", crate::proxy::generation(), key);
        let mut old = None;
        if eligible {
            if let Some(entry) = self.cache.lock().unwrap().get(&key) {
                if entry.expires > Instant::now() {
                    return Ok(cached(entry.value.clone()));
                }
                if Instant::now().duration_since(entry.expires).as_secs_f64() <= stale {
                    old = Some(entry.value.clone());
                }
            }
        }
        let mut attempt = 0;
        loop {
            let result = self.fetch_once(p.clone(), cancel.clone()).await;
            let failed = match &result {
                Err(e) => {
                    matches!(e.code.as_str(), "NETWORK" | "TIMEOUT")
                        || e.details
                            .as_ref()
                            .and_then(|d| d["status"].as_u64())
                            .is_some_and(|s| s >= 500)
                }
                Ok((v, _)) => v.json()["status"].as_u64().is_some_and(|s| s >= 500),
            };
            if failed && matches!(method, "GET" | "HEAD") && attempt < retries {
                let host = v["url"]
                    .as_str()
                    .and_then(|u| Url::parse(u).ok())
                    .and_then(|u| u.host_str().map(String::from))
                    .unwrap_or_default();
                let cooling = self
                    .cooldown
                    .lock()
                    .unwrap()
                    .get(&host)
                    .is_some_and(|t| *t > Instant::now());
                if !cooling {
                    let delay = if attempt == 0 { 400 } else { 1200 };
                    attempt += 1;
                    tokio::select! {_=cancel.cancelled()=>return Err(PluginError::cancelled()),_=tokio::time::sleep(Duration::from_millis(delay))=>{}};
                    continue;
                }
            }
            if failed {
                if let Some(value) = old {
                    return Ok(cached(value));
                }
            }
            if eligible {
                if let Ok((value, false)) = &result {
                    let response = value.json();
                    let control = response["headers"]["cache-control"]
                        .as_str()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if response["ok"] == true
                        && response["setCookies"].as_array().is_some_and(Vec::is_empty)
                        && !response["headers"]["vary"]
                            .as_str()
                            .unwrap_or("")
                            .split(',')
                            .any(|s| s.trim() == "*")
                        && !control.split(',').any(|s| s.trim() == "no-store")
                    {
                        let bytes = serde_json::to_vec(value)
                            .map(|v| v.len())
                            .unwrap_or(usize::MAX);
                        if bytes <= 8 * 1024 * 1024 && ttl > 0.0 {
                            let mut cache = self.cache.lock().unwrap();
                            cache.remove(&key);
                            while !cache.is_empty()
                                && cache.values().map(|e| e.bytes).sum::<usize>() + bytes
                                    > 8 * 1024 * 1024
                            {
                                let oldest = cache
                                    .iter()
                                    .min_by_key(|(_, e)| e.expires)
                                    .map(|(k, _)| k.clone())
                                    .unwrap();
                                cache.remove(&oldest);
                            }
                            cache.insert(
                                key,
                                CachedResponse {
                                    value: value.clone(),
                                    expires: Instant::now() + Duration::from_secs_f64(ttl),
                                    bytes,
                                },
                            );
                        }
                    }
                }
            }
            return result.map(|(value, _)| value);
        }
    }
    async fn fetch_once(
        self: &Arc<Self>,
        p: Transfer,
        cancel: CancellationToken,
    ) -> Result<(Transfer, bool)> {
        let v = p.json();
        let timeout =
            Duration::from_millis(v["timeoutMs"].as_u64().unwrap_or(15000).clamp(1, 30000));
        let work = async {
            let mut u = self.target(
                v["url"]
                    .as_str()
                    .ok_or_else(|| PluginError::invalid("缺少请求 URL"))?,
            )?;
            if let Some(q) = v["query"].as_object() {
                for (k, v) in q {
                    u.query_pairs_mut().append_pair(
                        k,
                        &v.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| v.to_string()),
                    );
                }
            }
            let method = Method::from_bytes(v["method"].as_str().unwrap_or("GET").as_bytes())
                .map_err(|_| PluginError::invalid("请求 method 无效"))?;
            if !matches!(
                method.as_str(),
                "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE"
            ) {
                return Err(PluginError::invalid("不支持该请求 method"));
            }
            let mut headers = headers(&v["headers"])?;
            let mut body = None;
            if let Some(b) = p.field("body") {
                if method == Method::GET || method == Method::HEAD {
                    return Err(PluginError::invalid("GET/HEAD 不允许 body"));
                }
                let j = b.json();
                body = Some(if let Some(j) = j.get("json") {
                    if !headers.contains_key("content-type") {
                        headers
                            .insert("content-type", HeaderValue::from_static("application/json"));
                    }
                    serde_json::to_vec(j).unwrap()
                } else if let Some(form) = j["form"].as_object() {
                    if !headers.contains_key("content-type") {
                        headers.insert(
                            "content-type",
                            HeaderValue::from_static("application/x-www-form-urlencoded"),
                        );
                    }
                    let mut ser = url::form_urlencoded::Serializer::new(String::new());
                    for (k, v) in form {
                        ser.append_pair(
                            k,
                            v.as_str()
                                .ok_or_else(|| PluginError::invalid("form 值必须为字符串"))?,
                        );
                    }
                    ser.finish().into_bytes()
                } else {
                    b.bytes()?
                });
                if body.as_ref().unwrap().len() > 1024 * 1024 {
                    return Err(PluginError::invalid("请求 body 超过 1 MiB"));
                }
            }
            let follow = body.is_none() && v["redirect"] != "manual";
            let max = v["maxBytes"]
                .as_u64()
                .unwrap_or(2 * 1024 * 1024)
                .clamp(1, 8 * 1024 * 1024) as usize;
            let resp = self
                .buffered(
                    u,
                    method,
                    headers,
                    body,
                    v["credentials"] != "omit",
                    follow,
                    max,
                    &cancel,
                )
                .await?;
            let mut meta = json!({"status":resp.status.as_u16(),"ok":resp.status.is_success(),"url":resp.url.as_str(),"headers":{},"setCookies":[],"fromCache":false});
            for (k, v) in &resp.headers {
                if let Ok(s) = v.to_str() {
                    meta["headers"][k.as_str()] = json!(s);
                }
            }
            meta["setCookies"] = json!(resp
                .headers
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .collect::<Vec<_>>());
            let charset = v["charset"]
                .as_str()
                .map(String::from)
                .or_else(|| {
                    meta["headers"]["content-type"].as_str().and_then(|s| {
                        s.split(';').find_map(|x| {
                            let (name, value) = x.trim().split_once('=')?;
                            name.trim()
                                .eq_ignore_ascii_case("charset")
                                .then(|| value.trim().trim_matches('"').to_string())
                        })
                    })
                })
                .unwrap_or_else(|| "utf-8".into());
            let data = if v["responseType"] == "bytes" {
                Transfer::Bytes(resp.bytes.clone())
            } else {
                let enc = encoding_rs::Encoding::for_label(charset.as_bytes())
                    .ok_or_else(|| PluginError::invalid("不支持该字符集"))?;
                let (text, _, _) = enc.decode(&resp.bytes);
                if v["responseType"] == "json" {
                    Transfer::from_json(serde_json::from_str(&text).map_err(|_| {
                        let mut error = PluginError::new("UPSTREAM_CHANGED", "响应不是合法 JSON");
                        error.details = Some(json!({"status":meta["status"]}));
                        error
                    })?)
                } else {
                    Transfer::String(text.into_owned())
                }
            };
            let mut result = Transfer::from_json(meta);
            if let Transfer::Object(ref mut m) = result {
                m.insert("data".into(), data);
            }
            result.validate(8 * 1024 * 1024)?;
            Ok((result, resp.redacted))
        };
        tokio::select! {_=cancel.cancelled()=>Err(PluginError::cancelled()),v=tokio::time::timeout(timeout,work)=>v.unwrap_or_else(|_|Err(PluginError::timeout()))}
    }
}
fn cached(mut value: Transfer) -> Transfer {
    if let Transfer::Object(ref mut o) = value {
        o.insert("fromCache".into(), Transfer::Bool(true));
    }
    value
}
pub fn headers(v: &Value) -> Result<HeaderMap> {
    let mut out = HeaderMap::new();
    let mut size = 0;
    if !v.is_null() && !v.is_object() {
        return Err(PluginError::invalid("headers 必须为对象"));
    }
    if let Some(m) = v.as_object() {
        for (k, v) in m {
            let k = HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| PluginError::invalid("请求头名称无效"))?;
            if matches!(
                k.as_str(),
                "host" | "content-length" | "connection" | "transfer-encoding" | "accept-encoding"
            ) {
                continue;
            }
            let s = v
                .as_str()
                .ok_or_else(|| PluginError::invalid("请求头值必须为字符串"))?;
            size += k.as_str().len() + s.len();
            if size > 16384 {
                return Err(PluginError::invalid("请求头超过 16 KiB"));
            }
            out.insert(
                k,
                HeaderValue::from_str(s).map_err(|_| PluginError::invalid("请求头值无效"))?,
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod scheduling_tests;
#[cfg(test)]
mod web_login_tests;
