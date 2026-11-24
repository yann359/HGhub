use futures_util::future::BoxFuture;
use hghub_hpp::{
    ipc,
    web_login::{LoginCompletion, LoginRequest, WebLoginProvider},
    PluginManager, Result,
};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const PLUGIN: &str = "fixture.web-login";
const SITE: &str = "https://accounts.example.invalid";
type Window = (CancellationToken, oneshot::Sender<LoginCompletion>);
struct FixtureBrowser(mpsc::UnboundedSender<Window>);
impl WebLoginProvider for FixtureBrowser {
    fn open(
        &self,
        plugin: String,
        request: Arc<LoginRequest>,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<LoginCompletion>> {
        assert_eq!(plugin, PLUGIN);
        assert_eq!(request.url, format!("{SITE}/login"));
        let (tx, rx) = oneshot::channel();
        self.0.send((cancel, tx)).unwrap();
        Box::pin(async move { Ok(rx.await.expect("window destroyed without cancellation")) })
    }
}
fn completed(path: &str) -> LoginCompletion {
    LoginCompletion {
        final_url: format!("{SITE}{path}"),
        cookies: vec![
            json!({"name":"session", "value":"synthetic-marker", "domain":"accounts.example.invalid", "path":"/", "hostOnly":true, "secure":true, "httpOnly":true}),
        ],
    }
}
async fn submit(host: Arc<PluginManager>, method_id: &str, request: &str) -> Result<Value> {
    ipc::dispatch(&host, "form.submit".into(), json!({"id":PLUGIN,"target":{"kind":"auth","id":method_id},"values":{},"requestId":request})).await
}
async fn import(host: &Arc<PluginManager>) {
    let info = host
        .import_code(include_str!("fixtures/web_login.js").into(), None, false)
        .await
        .unwrap();
    assert_eq!(info.status, "ready", "{:?}", info.error);
}
#[tokio::test]
async fn form_controls_isolated_login_lifetime_and_budget() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, mut windows) = mpsc::unbounded_channel();
    let host = Arc::new(
        PluginManager::open(dir.path().into())
            .unwrap()
            .with_web_login(Arc::new(FixtureBrowser(tx))),
    );
    import(&host).await;
    let blocked = host
        .call(
            PLUGIN,
            "auth.submit",
            json!({"methodId":"open","values":{}}),
            "background".into(),
        )
        .await
        .unwrap_err();
    assert_eq!(blocked.code, "UNSUPPORTED");
    let methods = host
        .call(PLUGIN, "auth.getMethods", json!({}), "methods".into())
        .await
        .unwrap();
    assert_eq!(methods["methods"][0]["id"], "open");
    let start = tokio::spawn(submit(host.clone(), "open", "interactive"));
    let (close, send) = tokio::time::timeout(Duration::from_secs(3), windows.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        submit(host.clone(), "open", "second")
            .await
            .unwrap_err()
            .code,
        "RATE_LIMITED"
    );
    // Wait longer than this call's five-second budget. Ordinary timers still run.
    tokio::time::sleep(Duration::from_millis(5100)).await;
    assert!(send.send(completed("/account/me")).is_ok());
    let result = start.await.unwrap().unwrap();
    let observed: Value = serde_json::from_str(result["user"]["name"].as_str().unwrap()).unwrap();
    assert_eq!(
        observed,
        json!({"paused":true,"resumed":true,"keys":["finalUrl"],"cookies":0})
    );
    assert!(close.is_cancelled());
    for (req, code) in [
        ("cancel", "CANCELLED"),
        ("timeout", "TIMEOUT"),
        ("mismatch", "AUTH_INVALID"),
    ] {
        let task = tokio::spawn(submit(host.clone(), req, req));
        let (close, send) = tokio::time::timeout(Duration::from_secs(3), windows.recv())
            .await
            .unwrap()
            .unwrap();
        if req == "cancel" {
            host.cancel(req).await;
        }
        if req == "mismatch" {
            assert!(send.send(completed("/accounting")).is_ok());
        }
        assert_eq!(task.await.unwrap().unwrap_err().code, code);
        tokio::time::timeout(Duration::from_secs(1), close.cancelled())
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn unconfigured_app_hides_web_login_method() {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(PluginManager::open(dir.path().into()).unwrap());
    import(&host).await;
    let methods = host
        .call(PLUGIN, "auth.getMethods", json!({}), "methods".into())
        .await
        .unwrap();
    assert_eq!(methods["methods"], json!([]));
}
