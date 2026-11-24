//! App-local upstream proxy preferences. WebView loopback routing is separate.
//! Required Notice: Copyright (c) 2026 HGhubX contributors (https://github.com/zxsos/HGhubX)
//! Adapted under PolyForm Noncommercial 1.0.0; see the root LICENSE for terms.
//! Adopted from zhujinrun/HGhubX, with atomic state/persistence and URL validation.

use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    System,
    Custom,
    Off,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Custom => "custom",
            Self::Off => "off",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "system" | "auto" | "" => Ok(Self::System),
            "custom" | "manual" => Ok(Self::Custom),
            "off" | "none" | "direct" => Ok(Self::Off),
            _ => Err("代理模式无效".into()),
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub(crate) struct Config {
    pub mode: Mode,
    pub url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::System,
            url: String::new(),
        }
    }
}

impl Config {
    /// An unsaved test must depend only on its input, not the current app mode.
    pub(crate) fn for_test(mode: &str, url: &str) -> Result<Self, String> {
        let mode = Mode::parse(mode)?;
        Ok(Self {
            mode,
            url: if mode == Mode::Custom {
                normalize_proxy(url)?
            } else {
                String::new()
            },
        })
    }

    fn decode(text: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        let Ok(mode) = Mode::parse(
            value
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("system"),
        ) else {
            return Self::default();
        };
        let url = normalize_proxy(value.get("url").and_then(Value::as_str).unwrap_or(""))
            .unwrap_or_default();
        if mode == Mode::Custom && url.is_empty() {
            return Self::default();
        }
        Self { mode, url }
    }
}

#[derive(Default)]
struct State {
    config: Config,
    path: Option<PathBuf>,
}

impl State {
    fn bind(&mut self, dir: &Path) {
        let path = dir.join("proxy.json");
        self.config = std::fs::read_to_string(&path)
            .map(|text| Config::decode(&text))
            .unwrap_or_default();
        self.path = Some(path);
    }

    fn set(&mut self, mode: &str, url: &str) -> Result<(), String> {
        let mode = Mode::parse(mode)?;
        let url = if mode == Mode::Custom || !url.trim().is_empty() {
            normalize_proxy(url)?
        } else {
            self.config.url.clone()
        };
        let path = self.path.as_ref().ok_or("代理设置尚未初始化")?;
        write_atomic(
            path,
            &json!({"mode": mode.as_str(), "url": url}).to_string(),
        )?;
        self.config = Config { mode, url };
        Ok(())
    }
}

static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) fn generation() -> u64 { GENERATION.load(std::sync::atomic::Ordering::Acquire) }

static STATE: Lazy<RwLock<State>> = Lazy::new(|| RwLock::new(State::default()));

pub(crate) fn snapshot() -> Config {
    STATE
        .read()
        .map(|state| state.config.clone())
        .unwrap_or_default()
}

pub fn mode() -> Mode {
    snapshot().mode
}
pub fn custom_url() -> Option<String> {
    let config = snapshot();
    (config.mode == Mode::Custom).then_some(config.url)
}

pub fn bind(dir: &Path) {
    if let Ok(mut state) = STATE.write() {
        state.bind(dir);
    }
}

pub fn set(mode: &str, url: &str) -> Result<Value, String> {
    let mut state = STATE.write().map_err(|_| "代理设置锁失败")?;
    let previous = state.config.clone();
    state.set(mode, url)?;
    if state.config != previous { GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel); }
    Ok(json!({"mode": state.config.mode.as_str(), "url": state.config.url}))
}

pub fn get() -> Value {
    let config = snapshot();
    json!({"mode":config.mode.as_str(),"url":config.url})
}

/// Construct a route per request so changes apply to existing provider instances.
/// Reqwest handles Windows/macOS fixed proxies, environment variables and NO_PROXY.
pub(crate) fn configure(builder: reqwest::ClientBuilder, config: &Config) -> Result<reqwest::ClientBuilder, String> {
    let explicit = match config.mode {
        Mode::Off => return Ok(builder.no_proxy()),
        Mode::Custom => Some(config.url.clone()),
        Mode::System => std::env::var("HGHUB_PROXY").ok().filter(|s| !s.trim().is_empty())
            .or_else(platform_proxy),
    };
    match explicit {
        Some(url) => Ok(builder.no_proxy().proxy(reqwest::Proxy::all(url).map_err(|_| "代理配置无效")?)),
        None => Ok(builder),
    }
}

#[cfg(target_os = "android")]
fn platform_proxy() -> Option<String> {
    // Android Wi-Fi fixed proxy. VPN/TUN routes are handled by the OS below sockets.
    use jni::objects::{JObject, JString, JValue};
    let context = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let result = (|| -> jni::errors::Result<Option<String>> {
        let get = |env: &mut jni::JNIEnv, key: &str| -> jni::errors::Result<String> {
            let key = JObject::from(env.new_string(key)?);
            let value = env.call_static_method("java/lang/System", "getProperty", "(Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(&key)])?.l()?;
            if value.is_null() { return Ok(String::new()); }
            Ok(env.get_string(&JString::from(value))?.into())
        };
        let host = get(&mut env, "http.proxyHost")?;
        let port = get(&mut env, "http.proxyPort")?;
        if host.is_empty() || port.parse::<u16>().ok().filter(|v| *v > 0).is_none() { return Ok(None); }
        let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
        Ok(Some(format!("http://{host}:{port}")))
    })();
    if env.exception_check().unwrap_or(false) { let _ = env.exception_clear(); }
    result.ok().flatten()
}

#[cfg(target_os = "linux")]
fn platform_proxy() -> Option<String> {
    if ["HTTP_PROXY","HTTPS_PROXY","ALL_PROXY","http_proxy","https_proxy","all_proxy"].iter().any(|k| std::env::var(k).is_ok_and(|v| !v.is_empty())) { return None; }
    fn setting(schema: &str, key: &str) -> Option<String> {
        let out = std::process::Command::new("gsettings").args(["get",schema,key]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().trim_matches('\'').to_string())
    }
    if setting("org.gnome.system.proxy", "mode").as_deref() != Some("manual") { return None; }
    for (schema, scheme) in [("org.gnome.system.proxy.http","http"),("org.gnome.system.proxy.socks","socks5")] {
        let host = setting(schema,"host").unwrap_or_default();
        let port = setting(schema,"port").unwrap_or_default();
        if !host.is_empty() && port.parse::<u16>().ok().filter(|n|*n>0).is_some() {
            let host = if host.contains(':') { format!("[{host}]") } else {host};
            return Some(format!("{scheme}://{host}:{port}"));
        }
    }
    None
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn platform_proxy() -> Option<String> { None }

pub(crate) fn connection_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() { "连接超时，请检查代理与网络" }
    else if error.is_connect() { "无法连接代理或目标服务器，请检查地址、认证与网络" }
    else if error.is_body() || error.is_decode() { "服务器响应读取失败" }
    else { "HTTP 请求失败" }
}

pub async fn probe(mode: &str, url: &str, target: &str) -> Result<Value, String> {
    let config = Config::for_test(mode, url)?;
    let strict = crate::network_policy::strict_security();
    let mut target = crate::network_policy::parse_target_with_mode(target, strict)?;
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(std::time::Duration::from_secs(12), async {
        for hop in 0..=5 {
            let addrs = crate::network_policy::resolve_target(&target, strict).await?;
            let builder = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(8)).resolve_to_addrs(target.host_str().unwrap(), &addrs);
            let client = configure(builder, &config)?.build().map_err(|_| "无法创建 HTTP 客户端")?;
            let response = client.get(target.clone()).send().await.map_err(|e| connection_error(&e))?;
            if response.status().is_redirection() && hop < 5 {
                if let Some(location) = response.headers().get("location").and_then(|s|s.to_str().ok()) {
                    target = crate::network_policy::redirect_target_with_mode(&target, location, strict)?;
                    continue;
                }
            }
            let status = response.status().as_u16();
            return Ok::<_,String>(json!({"ok":response.status().is_success(),"status":status,"error":if response.status().is_success(){String::new()}else{format!("已连接，服务器返回 HTTP {status}")}}));
        }
        Err("重定向次数过多".into())
    }).await;
    let mut value = match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => json!({"ok":false,"error":error}),
        Err(_) => json!({"ok":false,"error":"连接测试超时（12 秒）"}),
    };
    value["ms"] = json!(started.elapsed().as_millis() as u64);
    Ok(value)
}

/// Credentials are retained for the user's input field; callers must redact logs.
pub fn normalize_proxy(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("请填写代理地址（例：http://127.0.0.1:7897）".into());
    }
    if raw.chars().any(char::is_control) {
        return Err("代理地址含控制字符".into());
    }
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("http://{raw}")
    };
    let parsed = url::Url::parse(&with_scheme).map_err(|_| "代理地址格式无效".to_string())?;
    if !["http", "https", "socks5", "socks5h", "socks4", "socks4a"].contains(&parsed.scheme()) {
        return Err("代理协议只支持 http / https / socks5 / socks4".into());
    }
    let host = parsed
        .host_str()
        .filter(|host| !host.is_empty())
        .ok_or("代理地址缺少主机名")?;
    if !matches!(parsed.path(), "" | "/") || parsed.query().is_some() || parsed.fragment().is_some()
    {
        return Err("代理地址不要带路径、参数或片段".into());
    }
    if parsed.port() == Some(0) {
        return Err("代理端口应为 1–65535".into());
    }
    if parsed.username().is_empty() && parsed.password().is_some() {
        return Err("代理认证缺少用户名".into());
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let auth = match (parsed.username(), parsed.password()) {
        ("", _) => String::new(),
        (user, Some(pass)) => format!("{user}:{pass}@"),
        (user, None) => format!("{user}@"),
    };
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let normalized = format!("{}://{auth}{host}{port}", parsed.scheme());
    reqwest::Proxy::all(&normalized).map_err(|_| "代理配置无效（已隐藏原始值）")?;
    Ok(normalized)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp = path.with_extension(format!(
        "json.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let write = || -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    };
    if write().is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err("代理设置保存失败，已保留原设置".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_routes_through_proxy_and_direct_removes_preconfigured_proxy() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        async fn endpoint(body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = format!("http://{}", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut chunk = [0; 1024];
                    let count = socket.read(&mut chunk).await.unwrap();
                    if count == 0 { break; }
                    bytes.extend_from_slice(&chunk[..count]);
                    if bytes.windows(4).any(|v| v == b"\r\n\r\n") { break; }
                }
                let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                socket.write_all(response.as_bytes()).await.unwrap();
                String::from_utf8(bytes).unwrap()
            });
            (address, task)
        }
        let (proxy_address, proxy_request) = endpoint("proxy response").await;
        let custom = Config::for_test("custom", &proxy_address).unwrap();
        let client = configure(reqwest::Client::builder(), &custom).unwrap().build().unwrap();
        let result = client.get("http://upstream.invalid/video").timeout(std::time::Duration::from_secs(3)).send().await.unwrap();
        assert_eq!(result.text().await.unwrap(), "proxy response");
        assert!(proxy_request.await.unwrap().starts_with("GET http://upstream.invalid/video HTTP/1.1"));

        let (origin, origin_request) = endpoint("direct response").await;
        let builder = reqwest::Client::builder().proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap());
        let client = configure(builder, &Config::for_test("off", "").unwrap()).unwrap().build().unwrap();
        let result = client.get(format!("{origin}/cover")).timeout(std::time::Duration::from_secs(3)).send().await.unwrap();
        assert_eq!(result.text().await.unwrap(), "direct response");
        assert!(origin_request.await.unwrap().starts_with("GET /cover HTTP/1.1"));
    }

    fn temp_dir() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("hghubx-proxy-{}-{unique}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn proxy_normalization_supports_auth_ipv6_and_rejects_invalid_targets() {
        assert_eq!(
            normalize_proxy("127.0.0.1:7897").unwrap(),
            "http://127.0.0.1:7897"
        );
        assert_eq!(
            normalize_proxy("socks5h://name:p%40ss@[::1]:1080/").unwrap(),
            "socks5h://name:p%40ss@[::1]:1080"
        );
        assert_eq!(
            normalize_proxy("https://proxy.example:443/").unwrap(),
            "https://proxy.example"
        );
        for bad in [
            "",
            "ftp://proxy.example",
            "http://proxy.example/path",
            "http://proxy.example?token=x",
            "http://proxy.example#fragment",
            "http://proxy.example:0",
            "http://:pass@proxy.example",
            "http://proxy.example\nname",
        ] {
            assert!(normalize_proxy(bad).is_err());
        }
    }

    #[test]
    fn configuration_persists_restores_and_keeps_saved_address_while_off() {
        let dir = temp_dir();
        let mut state = State::default();
        state.bind(&dir);
        state.set("custom", "127.0.0.1:7897").unwrap();
        let mut restarted = State::default();
        restarted.bind(&dir);
        assert!(state.config == restarted.config);
        restarted.set("off", "").unwrap();
        assert_eq!(restarted.config.mode, Mode::Off);
        assert_eq!(restarted.config.url, "http://127.0.0.1:7897");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(dir.join("proxy.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_save_does_not_change_live_configuration() {
        let dir = temp_dir();
        let mut state = State::default();
        state.bind(&dir);
        state.set("custom", "127.0.0.1:7897").unwrap();
        let previous = state.config.clone();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(state.set("off", "").is_err());
        assert!(state.config == previous);
    }

    #[test]
    fn invalid_or_missing_file_resets_to_system_and_test_input_is_independent() {
        for text in [
            "broken",
            r#"{"mode":"custom","url":"ftp://proxy.example"}"#,
            r#"{"mode":"unexpected"}"#,
        ] {
            assert!(Config::decode(text) == Config::default());
        }
        assert_eq!(
            Config::for_test("system", "invalid ignored draft")
                .unwrap()
                .mode,
            Mode::System
        );
        assert_eq!(
            Config::for_test("off", "invalid ignored draft")
                .unwrap()
                .mode,
            Mode::Off
        );
        assert!(Config::for_test("invalid", "").is_err());
        assert!(Config::for_test("custom", "").is_err());
        let dir = temp_dir();
        let mut state = State::default();
        state.bind(&dir);
        state.set("custom", "127.0.0.1:7897").unwrap();
        std::fs::remove_file(dir.join("proxy.json")).unwrap();
        state.bind(&dir);
        assert!(state.config == Config::default());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
