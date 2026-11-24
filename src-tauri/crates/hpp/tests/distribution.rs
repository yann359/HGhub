use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Response, Uri},
    routing::get,
    Router,
};
use hghub_hpp::PluginManager;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

const SOURCE: &str = include_str!("fixtures/synthetic.js");

struct Documents(HashMap<String, Vec<u8>>);
impl hghub_hpp::manager::DocumentReader for Documents {
    fn read(&self, location: &str, limit: usize) -> hghub_hpp::Result<Vec<u8>> {
        let data = self
            .0
            .get(location)
            .ok_or_else(|| hghub_hpp::PluginError::new("NOT_FOUND", "Document not found"))?;
        Ok(data.iter().take(limit + 1).copied().collect())
    }
}

#[tokio::test]
async fn opaque_document_import_preserves_origin_and_isolates_invalid_files() {
    let dir = tempfile::tempdir().unwrap();
    let uri = "content://documents/primary%3ADownload%2Fplugin.js";
    let large = "content://documents/oversize.js";
    let invalid = "content://documents/non-utf8.js";
    let list = "content://documents/list.json";
    let manager = PluginManager::open(dir.path().into()).unwrap()
        .with_document_reader(Arc::new(Documents(HashMap::from([
            (uri.into(), SOURCE.as_bytes().to_vec()),
            (large.into(), vec![b' '; 2 * 1024 * 1024 + 1]),
            (invalid.into(), vec![0xff]),
            (list.into(), json!({"plugins":[
                {"id":"fixture.remote","name":"Remote","version":"1.0.0","url":"https://plugins.example/demo.js"},
                {"id":"fixture.relative","name":"Relative","version":"1.0.0","url":"demo.js"}
            ]}).to_string().into_bytes()),
        ]))));
    let preview = manager
        .preview_import(vec![uri.into(), large.into(), invalid.into()], "documents")
        .await
        .unwrap();
    assert!(preview.items[0].error.is_none());
    assert!(preview.items[1]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("2 MiB"));
    assert!(preview.items[2]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("UTF-8"));
    let report = manager
        .apply_import(
            &preview.token,
            keys(&preview),
            false,
            true,
            "install-document",
        )
        .await
        .unwrap();
    let plugin = report.items[0].plugin.as_ref().unwrap();
    assert_eq!(plugin.source.as_deref(), Some(uri));
    assert_eq!(plugin.status, "disabled");
    let preview = manager
        .preview_import(vec![list.into()], "document-list")
        .await
        .unwrap();
    assert_eq!(preview.items[0].source, "https://plugins.example/demo.js");
    assert!(preview.items[0].error.is_none());
    assert_eq!(preview.items[1].error.as_ref().unwrap().code, "UNSUPPORTED");
}
fn code(version: &str) -> String {
    SOURCE.replace("version: \"1.0.0\"", &format!("version: \"{version}\""))
}
fn keys(preview: &hghub_hpp::manager::ImportPreview) -> Vec<String> {
    preview
        .items
        .iter()
        .filter(|i| i.error.is_none())
        .map(|i| i.key.clone())
        .collect()
}

#[tokio::test]
async fn granted_directory_resolves_opaque_ids_and_only_reads_selected_plugins() {
    struct TreeDocuments {
        files: Documents,
        reads: Mutex<Vec<String>>,
        references: Mutex<Vec<String>>,
    }
    impl hghub_hpp::manager::DocumentReader for TreeDocuments {
        fn read(&self, location: &str, limit: usize) -> hghub_hpp::Result<Vec<u8>> {
            self.reads.lock().unwrap().push(location.into());
            self.files.read(location, limit)
        }
        fn resolve_relative(&self, base: &str, reference: &str) -> hghub_hpp::Result<String> {
            assert_eq!(base, "content://provider/tree/root/document/opaque-list");
            self.references.lock().unwrap().push(reference.into());
            match reference {
                "sources/one.js" => Ok("content://provider/tree/root/document/opaque-one".into()),
                "wrong.js" => Ok("content://provider/tree/root/document/opaque-wrong".into()),
                "unused.js" => Ok("content://provider/tree/root/document/opaque-unused".into()),
                "../outside.js" => Err(hghub_hpp::PluginError::invalid("列表条目越出了获授权目录")),
                _ => panic!("unexpected reference {reference}"),
            }
        }
    }
    let list = "content://provider/tree/root/document/opaque-list";
    let one = "content://provider/tree/root/document/opaque-one";
    let wrong = "content://provider/tree/root/document/opaque-wrong";
    let reader = Arc::new(TreeDocuments {
        files: Documents(HashMap::from([
            (list.into(), json!({"plugins":[
                {"id":"fixture.source","name":"One","version":"1.0.0","url":"sources/one.js"},
                {"id":"fixture.source","name":"Wrong","version":"1.0.0","url":"wrong.js"},
                {"id":"fixture.unused","name":"Unused","version":"1.0.0","url":"unused.js"},
                {"id":"fixture.outside","name":"Outside","version":"1.0.0","url":"../outside.js"},
                {"id":"fixture.remote","name":"Remote","version":"1.0.0","url":"https://plugins.example/demo.js"}
            ]}).to_string().into_bytes()),
            (one.into(), SOURCE.as_bytes().to_vec()),
            (wrong.into(), SOURCE.replace("fixture.source", "fixture.wrong").into_bytes()),
        ])),
        reads: Mutex::default(),
        references: Mutex::default(),
    });
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into())
        .unwrap()
        .with_document_reader(reader.clone());
    let preview = manager
        .preview_import(vec![list.into()], "directory")
        .await
        .unwrap();
    assert_eq!(*reader.reads.lock().unwrap(), vec![list]);
    assert_eq!(reader.references.lock().unwrap().len(), 4);
    assert_eq!(preview.items[0].source, one);
    assert_eq!(
        preview.items[3].error.as_ref().unwrap().code,
        "INVALID_ARGUMENT"
    );
    assert_eq!(preview.items[4].source, "https://plugins.example/demo.js");
    assert!(preview.items[4].error.is_none());
    let report = manager
        .apply_import(
            &preview.token,
            preview.items[..2]
                .iter()
                .map(|item| item.key.clone())
                .collect(),
            false,
            true,
            "directory-install",
        )
        .await
        .unwrap();
    assert_eq!(
        report.items[0].plugin.as_ref().unwrap().source.as_deref(),
        Some(list)
    );
    assert_eq!(
        report.items[0]
            .plugin
            .as_ref()
            .unwrap()
            .origin
            .as_ref()
            .unwrap()
            .location,
        one
    );
    assert!(report.items[1]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("id 或 version"));
    assert_eq!(*reader.reads.lock().unwrap(), vec![list, one, wrong]);
    assert_eq!(manager.list().await.len(), 1);
}

#[tokio::test]
async fn cancelling_directory_preview_stops_before_resolving_next_entry() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    };
    struct BlockingDocuments {
        started: tokio::sync::Notify,
        release: Mutex<mpsc::Receiver<()>>,
        calls: AtomicUsize,
    }
    impl hghub_hpp::manager::DocumentReader for BlockingDocuments {
        fn read(&self, _: &str, _: usize) -> hghub_hpp::Result<Vec<u8>> {
            Ok(json!({"plugins":[
                {"id":"fixture.one","name":"One","version":"1.0.0","url":"one.js"},
                {"id":"fixture.two","name":"Two","version":"1.0.0","url":"two.js"}
            ]})
            .to_string()
            .into_bytes())
        }
        fn resolve_relative(&self, _: &str, _: &str) -> hghub_hpp::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release.lock().unwrap().recv().unwrap();
            Ok("content://provider/one".into())
        }
    }
    let (release, receiver) = mpsc::channel();
    let reader = Arc::new(BlockingDocuments {
        started: Default::default(),
        release: Mutex::new(receiver),
        calls: AtomicUsize::new(0),
    });
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(
        PluginManager::open(dir.path().into())
            .unwrap()
            .with_document_reader(reader.clone()),
    );
    let pending = {
        let manager = manager.clone();
        tokio::spawn(async move {
            manager
                .preview_import(vec!["content://provider/list".into()], "cancel-directory")
                .await
        })
    };
    reader.started.notified().await;
    manager.cancel("cancel-directory").await;
    let outcome = tokio::time::timeout(Duration::from_secs(2), pending).await;
    release.send(()).unwrap();
    assert_eq!(outcome.unwrap().unwrap().err().unwrap().code, "CANCELLED");
    assert_eq!(reader.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn local_manifest_selection_identity_failures_install_only_and_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let files = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_mode(dir.path().into(), Some(false)).unwrap();
    std::fs::write(files.path().join("one.js"), SOURCE).unwrap();
    std::fs::write(
        files.path().join("wrong.js"),
        SOURCE.replace("fixture.source", "fixture.wrong"),
    )
    .unwrap();
    std::fs::write(
        files.path().join("manifest.json"),
        json!({"name":"虚构插件集","plugins":[
            {"id":"fixture.source","name":"正确条目","description":"列表展示简介","version":"1.0.0","url":"one.js"},
            {"id":"fixture.source","name":"身份不一致","version":"1.0.0","url":"wrong.js"},
            {"id":"fixture.missing","name":"不勾选条目","version":"1.0.0","url":"missing.js"},
            {"id":"bad/id","name":"坏条目","version":"01.0.0","url":"bad.js"}
        ]})
        .to_string(),
    )
    .unwrap();
    let location = files
        .path()
        .join("manifest.json")
        .to_string_lossy()
        .into_owned();
    let preview = manager
        .preview_import(vec![location.clone()], "local-preview")
        .await
        .unwrap();
    assert_eq!(preview.items.len(), 4);
    assert_eq!(preview.items[0].description, "列表展示简介");
    assert!(preview.items[3].error.is_some());
    assert!(manager.list().await.is_empty());
    let selected = preview.items[..2].iter().map(|i| i.key.clone()).collect();
    let report = manager
        .apply_import(&preview.token, selected, false, true, "local-import")
        .await
        .unwrap();
    assert_eq!(report.items.len(), 2);
    assert_eq!(report.items[0].plugin.as_ref().unwrap().status, "disabled");
    assert_eq!(
        report.items[0]
            .plugin
            .as_ref()
            .unwrap()
            .origin
            .as_ref()
            .unwrap()
            .manifest_name
            .as_deref(),
        Some("虚构插件集")
    );
    assert!(report.items[1]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("不一致"));
    assert_eq!(manager.list().await.len(), 1);
    drop(manager);
    let manager = PluginManager::with_mode(dir.path().into(), Some(false)).unwrap();
    assert_eq!(
        manager.list().await[0]
            .origin
            .as_ref()
            .unwrap()
            .manifest_name
            .as_deref(),
        Some("虚构插件集")
    );
    let initial = manager.set_enabled("fixture.source", true).await.unwrap();
    manager
        .settings_set("fixture.source", "label".into(), json!("保留用户设置"))
        .await
        .unwrap();
    manager
        .call(
            "fixture.source",
            "search.search",
            json!({"query":"bytes"}),
            "store".into(),
        )
        .await
        .unwrap();
    // Multiple broken overwrites must keep the last known usable version, even across a restart.
    for v in ["1.1.0", "1.2.0"] {
        let bad = format!("export const meta={{id:'fixture.source',name:'失败更新',version:'{v}',protocol:1}};export default ()=>{{throw new Error('broken factory')}};");
        assert_eq!(
            manager
                .import_code(bad, Some("failed.js".into()), false)
                .await
                .unwrap()
                .status,
            "failed"
        );
    }
    drop(manager);
    let manager = PluginManager::with_mode(dir.path().into(), Some(false)).unwrap();
    assert!(manager.list().await[0].can_restore);
    let restored = manager.restore_previous("fixture.source").await.unwrap();
    assert_eq!(
        restored.origin.as_ref().unwrap().manifest_name.as_deref(),
        Some("虚构插件集")
    );
    assert_eq!(restored.meta.version, initial.meta.version);
    assert_eq!(restored.status, "ready");
    assert_eq!(
        manager.settings_get("fixture.source").await.unwrap()["label"],
        "保留用户设置"
    );
    let detail = manager
        .call(
            "fixture.source",
            "media.getDetail",
            json!({"id":"one"}),
            "detail".into(),
        )
        .await
        .unwrap();
    assert_eq!(detail["title"], "保留用户设置");
    let p = manager
        .preview_import(
            vec![files.path().join("one.js").to_string_lossy().into_owned()],
            "single",
        )
        .await
        .unwrap();
    manager
        .apply_import(&p.token, keys(&p), false, false, "same-version")
        .await
        .unwrap();
    assert_ne!(manager.list().await[0].revision, restored.revision);
    assert!(manager
        .apply_import(&p.token, keys(&p), false, false, "replayed")
        .await
        .is_err());
    assert!(manager.list().await[0]
        .origin
        .as_ref()
        .unwrap()
        .location
        .ends_with("one.js"));
}

#[derive(Default)]
struct Fixture {
    files: HashMap<String, String>,
    requests: Vec<(String, HeaderMap)>,
}
type Shared = Arc<Mutex<Fixture>>;
async fn serve(State(state): State<Shared>, uri: Uri, headers: HeaderMap) -> Response<Body> {
    let path = uri.path().to_string();
    state.lock().unwrap().requests.push((path.clone(), headers));
    if path == "/download" {
        return Response::builder()
            .status(302)
            .header("location", "/set/list")
            .body(Body::empty())
            .unwrap();
    }
    if path == "/slow" {
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    let content = state.lock().unwrap().files.get(&path).cloned();
    Response::builder()
        .status(if content.is_some() { 200 } else { 404 })
        .body(Body::from(content.unwrap_or_default()))
        .unwrap()
}
async fn server() -> (String, Shared, tokio::task::JoinHandle<()>) {
    let state: Shared = Default::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/*path", get(serve))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, state, task)
}

#[tokio::test]
async fn redirected_url_manifest_numeric_updates_stale_check_and_provenance() {
    let (base, state, server) = server().await;
    let list = |version: &str| {
        json!({"name":"重定向列表","plugins":[{"id":"fixture.source","name":"虚构源","version":version,"url":"plugin"}]}).to_string()
    };
    state.lock().unwrap().files.extend([
        ("/set/list".into(), list("1.9.0")),
        ("/set/plugin".into(), code("1.9.0")),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::with_mode(dir.path().into(), Some(false)).unwrap();
    let source = format!("{base}/download");
    let preview = manager
        .preview_import(vec![source.clone()], "remote")
        .await
        .unwrap();
    assert_eq!(preview.name, "重定向列表");
    assert!(preview.items[0].source.ends_with("/set/plugin"));
    assert!(!state
        .lock()
        .unwrap()
        .requests
        .iter()
        .any(|(p, _)| p == "/set/plugin"));
    let report = manager
        .apply_import(&preview.token, keys(&preview), false, false, "install")
        .await
        .unwrap();
    let initial = report.items[0].plugin.as_ref().unwrap();
    assert_eq!(initial.meta.version, "1.9.0");
    assert_eq!(
        initial.origin.as_ref().unwrap().manifest.as_ref().unwrap(),
        &source
    );
    state.lock().unwrap().files.extend([
        ("/set/list".into(), list("1.10.0")),
        ("/set/plugin".into(), code("1.10.0")),
    ]);
    let check = manager
        .check_updates(vec!["fixture.source".into()], "update-check")
        .await
        .unwrap();
    assert_eq!(check.statuses[0].status, "available");
    assert_eq!(manager.list().await[0].revision, initial.revision);
    assert_eq!(manager.list().await[0].meta.version, "1.9.0");
    manager
        .settings_set("fixture.source", "label".into(), json!("修订已变化"))
        .await
        .unwrap();
    let stale = manager
        .apply_import(
            &check.preview.token,
            keys(&check.preview),
            true,
            false,
            "stale-apply",
        )
        .await
        .unwrap();
    assert!(stale.items[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("重新检查"));
    let check = manager
        .check_updates(vec!["fixture.source".into()], "fresh-check")
        .await
        .unwrap();
    manager.set_enabled("fixture.source", false).await.unwrap();
    let updated = manager
        .apply_import(
            &check.preview.token,
            keys(&check.preview),
            false,
            false,
            "update",
        )
        .await
        .unwrap();
    let updated = updated.items[0].plugin.as_ref().unwrap();
    assert_eq!(updated.meta.version, "1.10.0");
    assert_eq!(updated.status, "disabled");
    assert_eq!(
        updated.origin.as_ref().unwrap().manifest.as_ref().unwrap(),
        &source
    );
    assert_eq!(
        manager
            .check_updates(vec!["fixture.source".into()], "manual-again")
            .await
            .unwrap()
            .statuses[0]
            .status,
        "current"
    );
    assert!(state
        .lock()
        .unwrap()
        .requests
        .iter()
        .all(|(_, h)| !h.contains_key("authorization") && !h.contains_key("cookie")));
    server.abort();
}

#[tokio::test]
async fn update_url_precedes_manifest_identity_mismatch_and_download_cancellation() {
    let (base, state, server) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::with_mode(dir.path().into(), Some(false)).unwrap());
    let initial = SOURCE.replacen(
        "protocol: 1,",
        &format!("protocol: 1, allowInsecureHttp: true, updateUrl: '{base}/direct',"),
        1,
    );
    manager
        .import_code(initial, Some(format!("{base}/unused-list")), false)
        .await
        .unwrap();
    state.lock().unwrap().files.insert(
        "/direct".into(),
        code("1.1.0").replace("fixture.source", "other.identity"),
    );
    let check = manager
        .check_updates(vec!["fixture.source".into()], "identity")
        .await
        .unwrap();
    assert_eq!(check.statuses[0].status, "failed");
    assert!(check.statuses[0]
        .error
        .as_ref()
        .unwrap()
        .message
        .contains("ID 不一致"));
    assert!(!state
        .lock()
        .unwrap()
        .requests
        .iter()
        .any(|(p, _)| p == "/unused-list"));
    let m = manager.clone();
    let url = format!("{base}/slow");
    let waiting = tokio::spawn(async move { m.preview_import(vec![url], "cancel-download").await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    manager.cancel("cancel-download").await;
    assert_eq!(waiting.await.unwrap().err().unwrap().code, "CANCELLED");
    state
        .lock()
        .unwrap()
        .files
        .insert("/html".into(), "<!doctype html><title>Error</title>".into());
    let bad = manager
        .preview_import(vec![format!("{base}/html")], "html")
        .await
        .unwrap();
    assert!(bad.items[0].error.is_some());
    assert_eq!(manager.list().await.len(), 1);
    server.abort();
}
