use futures_util::future::BoxFuture;
use hghub_hpp::{
    web_login::{LoginCompletion, LoginRequest, WebLoginProvider},
    PluginError, Result,
};
use serde_json::json;
use std::sync::Arc;
use tauri::plugin::{mobile::PluginInvokeError, PluginHandle};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AndroidLogin(pub PluginHandle<tauri::Wry>);

fn bridge_error(e: PluginInvokeError) -> PluginError {
    if let PluginInvokeError::InvokeRejected(reply) = &e {
        match reply.code.as_deref() {
            Some("CANCELLED") => return PluginError::cancelled(),
            Some("RATE_LIMITED") => {
                return PluginError::new("RATE_LIMITED", "请先关闭当前网页登录窗口")
            }
            Some("NETWORK") => return PluginError::new("NETWORK", "登录页面加载失败"),
            _ => (),
        }
    }
    PluginError::internal(format!("安卓网页登录出错：{e}"))
}
impl AndroidLogin {
    pub async fn supported(&self) -> Result<bool> {
        self.0
            .run_mobile_plugin_async("supported", ())
            .await
            .map_err(bridge_error)
    }
}
impl WebLoginProvider for AndroidLogin {
    fn open(
        &self,
        _: String,
        request: Arc<LoginRequest>,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<LoginCompletion>> {
        let handle = self.0.clone();
        Box::pin(async move {
            let session = uuid::Uuid::new_v4().to_string();
            let close = handle.clone();
            let closed = CancellationToken::new();
            let finished = closed.clone();
            let id = session.clone();
            // The runtime may drop this future on timeout. Keep native dismissal independent.
            tokio::spawn(async move {
                tokio::select! { biased;
                    _ = finished.cancelled() => (),
                    _ = cancel.cancelled() => {
                        if let Err(error) = close.run_mobile_plugin_async::<()>("cancelLogin", json!({"session":id})).await {
                            eprintln!("关闭网页登录窗口失败：{error}");
                        }
                    }
                }
            });
            let result = handle
                .run_mobile_plugin_async(
                    "openLogin",
                    json!({"session":session,"request":request.as_ref()}),
                )
                .await;
            closed.cancel();
            result.map_err(bridge_error)
        })
    }
}
