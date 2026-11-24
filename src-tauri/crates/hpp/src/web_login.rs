//! Native adapter contract for a user-initiated, isolated first-party login.
use crate::{network::Network, PluginError, Result};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginConditions {
    pub url_prefix: Option<String>,
    #[serde(default)]
    pub cookie_names: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub url: String,
    pub title: Option<String>,
    pub done_when: LoginConditions,
    #[serde(default = "default_timeout")]
    pub timeout_sec: u64,
}
fn default_timeout() -> u64 {
    300
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCompletion {
    pub final_url: String,
    /// First-party session granted to the application. Only the network layer consumes it.
    pub cookies: Vec<Value>,
}

/// Register only after platform capability checks. Implementations close their window on
/// cancellation (including timeout) and never expose the application bridge to web content.
/// The adapter observes a main-frame navigation or a change in cookie state after opening.
pub trait WebLoginProvider: Send + Sync {
    fn open(
        &self,
        plugin_id: String,
        request: std::sync::Arc<LoginRequest>,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<LoginCompletion>>;
}

impl LoginRequest {
    pub(crate) fn parse(v: Value, net: &Network) -> Result<Self> {
        let request: Self =
            serde_json::from_value(v).map_err(|_| PluginError::invalid("网页登录参数无效"))?;
        let start = net.target(&request.url)?;
        if start.scheme() != "https"
            || !(1..=600).contains(&request.timeout_sec)
            || request
                .title
                .as_ref()
                .is_some_and(|s| s.chars().count() > 60)
            || request
                .done_when
                .cookie_names
                .iter()
                .any(|s| s.is_empty() || s.chars().any(|c| c.is_control() || ";= ".contains(c)))
            || (request.done_when.url_prefix.is_none() && request.done_when.cookie_names.is_empty())
        {
            return Err(PluginError::invalid(
                "网页登录需要有效的 HTTPS 地址、完成条件和超时",
            ));
        }
        if let Some(raw) = &request.done_when.url_prefix {
            if net.target(raw)?.origin() != start.origin() {
                return Err(PluginError::invalid("完成地址须与起始页同源"));
            }
        }
        Ok(request)
    }
    pub fn matches_page(&self, page: &Url) -> bool {
        let start = Url::parse(&self.url).expect("validated login URL");
        if page.origin() != start.origin() {
            return false;
        }
        self.done_when.url_prefix.as_ref().is_none_or(|prefix| {
            let prefix = Url::parse(prefix).expect("validated completion URL");
            let path = prefix.path();
            page.path() == path
                || page
                    .path()
                    .strip_prefix(path)
                    .is_some_and(|tail| path.ends_with('/') || tail.starts_with('/'))
        })
    }

    pub fn matches_cookies(&self, cookies: &[Value]) -> bool {
        self.done_when
            .cookie_names
            .iter()
            .all(|name| cookies.iter().any(|c| c["name"].as_str() == Some(name)))
    }

    pub(crate) fn commit(&self, result: LoginCompletion, net: &Network) -> Result<String> {
        let url = net.target(&result.final_url)?;
        if !self.matches_page(&url) || !self.matches_cookies(&result.cookies) {
            return Err(PluginError::new("AUTH_INVALID", "站点尚未满足登录完成条件"));
        }
        net.replace_web_login(&self.url, &result.final_url, result.cookies)?;
        Ok(result.final_url)
    }
}
