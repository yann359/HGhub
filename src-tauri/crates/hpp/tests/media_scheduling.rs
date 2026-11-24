use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{Response, Uri},
    routing::get,
    Router,
};
use futures_util::StreamExt;
use hghub_hpp::PluginManager;
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Semaphore;

struct Fixture {
    seen: Mutex<Vec<String>>,
    release: Semaphore,
}

async fn upstream(State(state): State<Arc<Fixture>>, uri: Uri) -> Response<Body> {
    state.seen.lock().unwrap().push(uri.path().to_string());
    let body =
        futures_util::stream::once(async { Ok::<_, std::io::Error>(Bytes::from_static(b"first")) })
            .chain(futures_util::stream::once(async move {
                state.release.acquire().await.unwrap().forget();
                Ok(Bytes::from_static(b"last"))
            }));
    Response::builder()
        .header("content-type", "video/mp4")
        .status(if uri.path() == "/warm" { 206 } else { 200 })
        .header("content-range", "bytes 0-8/9")
        .body(Body::from_stream(body))
        .unwrap()
}

async fn until(test: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !test() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture did not reach expected state");
}

#[tokio::test]
async fn foreground_interrupts_warmup_and_keeps_priority_until_all_bodies_finish() {
    let state = Arc::new(Fixture {
        seen: Default::default(),
        release: Semaphore::new(0),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/:id", get(upstream))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::with_mode(dir.path().into(), Some(false)).unwrap());
    let source = format!(
        r#"
export const meta={{id:'fixture.priority',name:'Priority',version:'1.0.0',protocol:1,allowInsecureHttp:true}};
export default hg=>({{media:{{getDetail({{id}}){{return {{id,title:id,playlists:[{{id:'main',episodes:[]}}]}};}},
resolvePlayback({{mediaId}}){{return {{streams:[{{url:'{base}/'+mediaId,format:'progressive'}}]}};}}}}}});
"#
    );
    assert_eq!(
        manager
            .import_code(source, None, false)
            .await
            .unwrap()
            .status,
        "ready"
    );
    let warm = manager
        .prepare_media(
            "fixture.priority",
            json!({"mediaId":"warm","episodeId":"one","reason":"preload"}),
            0,
            None,
            "warm".into(),
        )
        .await
        .unwrap();
    let play = manager
        .prepare_media(
            "fixture.priority",
            json!({"mediaId":"play","episodeId":"one","reason":"play"}),
            0,
            None,
            "play".into(),
        )
        .await
        .unwrap();
    let warming = {
        let manager = manager.clone();
        let id = warm.session_id.clone();
        tokio::spawn(async move { manager.preload_media(&id, 0.0).await })
    };
    until(|| {
        state
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|path| path == "/warm")
    })
    .await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut playing = Vec::new();
    for _ in 0..7 {
        let client = client.clone();
        let url = play.url.clone();
        playing.push(tokio::spawn(async move {
            client.get(url).send().await.unwrap().bytes().await.unwrap()
        }));
    }
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), warming)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err()
            .code,
        "CANCELLED"
    );
    until(|| {
        state
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|path| *path == "/play")
            .count()
            == 6
    })
    .await;
    assert_eq!(
        manager.preload_media(&warm.session_id, 0.0).await.unwrap()["bytes"],
        0
    );
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        state.seen.lock().unwrap().len(),
        7,
        "six playing bodies; seventh queued, no new warmup"
    );
    state.release.add_permits(1);
    until(|| {
        state
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|path| *path == "/play")
            .count()
            == 7
    })
    .await;
    assert_eq!(
        manager.preload_media(&warm.session_id, 0.0).await.unwrap()["bytes"],
        0
    );
    state.release.add_permits(6);
    for playing in playing {
        assert_eq!(playing.await.unwrap(), b"firstlast".as_slice());
    }
    state.release.add_permits(2);
    assert_eq!(
        manager.preload_media(&warm.session_id, 0.0).await.unwrap()["bytes"],
        18
    );
    let mut last_body = client.get(&play.url).send().await.unwrap();
    assert_eq!(last_body.chunk().await.unwrap().unwrap(), b"first".as_slice());
    let weak = Arc::downgrade(&manager);
    drop(manager);
    assert!(weak.upgrade().is_none(), "streamed body must not keep the entire host alive");
    assert!(tokio::time::timeout(Duration::from_secs(1), last_body.chunk()).await.unwrap().unwrap().is_none());
    server.abort();
}
