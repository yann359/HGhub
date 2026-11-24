use super::*;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::Uri,
    routing::any,
    Router,
};
use futures_util::StreamExt;
use tokio::sync::Semaphore;

struct Fixture {
    seen: Mutex<Vec<(String, HeaderMap)>>,
    release: Semaphore,
}

async fn serve(
    State(state): State<Arc<Fixture>>,
    uri: Uri,
    headers: HeaderMap,
) -> axum::http::Response<Body> {
    state.seen.lock().unwrap().push((uri.to_string(), headers));
    let response = axum::http::Response::builder();
    if uri.path() == "/redirect" {
        return response
            .status(302)
            .header("location", "/private")
            .body(Body::empty())
            .unwrap();
    }
    if uri.path() == "/cooldown" {
        return response
            .status(503)
            .header("retry-after", "60")
            .body(Body::empty())
            .unwrap();
    }
    if uri.path() == "/immediate" {
        return response.body(Body::from("ready")).unwrap();
    }
    let stream =
        futures_util::stream::once(async { Ok::<_, std::io::Error>(Bytes::from_static(b"first")) })
            .chain(futures_util::stream::once(async move {
                state.release.acquire().await.unwrap().forget();
                Ok(Bytes::from_static(b"last"))
            }));
    response.body(Body::from_stream(stream)).unwrap()
}

async fn fixture() -> (String, Arc<Fixture>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Fixture {
        seen: Default::default(),
        release: Semaphore::new(0),
    });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let router = Router::new()
        .route("/*path", any(serve))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, state, server)
}

fn network(dir: &std::path::Path) -> Arc<Network> {
    let meta = Meta::parse(json!({"id":"fixture.scheduling","name":"Scheduling","protocol":1,"version":"1.0.0","allowInsecureHttp":true})).unwrap();
    Arc::new(Network::new(meta, dir.into(), Some(false), Default::default()).unwrap())
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

fn fetch(
    network: &Arc<Network>,
    value: Value,
    cancel: &CancellationToken,
) -> tokio::task::JoinHandle<Result<Transfer>> {
    let network = network.clone();
    let cancel = cancel.clone();
    tokio::spawn(async move { network.fetch(Transfer::from_json(value), cancel).await })
}

#[tokio::test]
async fn anonymous_get_shares_actual_request_and_waiters_cancel_independently() {
    let (base, fixture, server) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let network = network(dir.path());
    let a_cancel = CancellationToken::new();
    let b_cancel = CancellationToken::new();
    let a = fetch(
        &network,
        json!({"url":format!("{base}/shared"),"query":{"q":"a b"},"headers":{"X-Test":"same","Host":"ignored"}}),
        &a_cancel,
    );
    let b = fetch(
        &network,
        json!({"url":format!("{base}/shared?q=a+b#fragment"),"headers":{"x-test":"same"},"responseType":"bytes"}),
        &b_cancel,
    );
    until(|| {
        fixture.seen.lock().unwrap().len() == 1
            && network
                .inflight
                .lock()
                .unwrap()
                .values()
                .any(|f| f.result.receiver_count() == 2)
    })
    .await;
    a_cancel.cancel();
    assert_eq!(a.await.unwrap().unwrap_err().code, "CANCELLED");
    fixture.release.add_permits(1);
    assert_eq!(
        b.await
            .unwrap()
            .unwrap()
            .field("data")
            .unwrap()
            .bytes()
            .unwrap(),
        b"firstlast"
    );
    assert_eq!(fixture.seen.lock().unwrap().len(), 1);

    let a_cancel = CancellationToken::new();
    let b_cancel = CancellationToken::new();
    let p = json!({"url":format!("{base}/abandoned")});
    let a = fetch(&network, p.clone(), &a_cancel);
    let b = fetch(&network, p.clone(), &b_cancel);
    until(|| {
        fixture.seen.lock().unwrap().len() == 2
            && network
                .inflight
                .lock()
                .unwrap()
                .values()
                .any(|f| f.result.receiver_count() == 2)
    })
    .await;
    a_cancel.cancel();
    b_cancel.cancel();
    assert_eq!(a.await.unwrap().unwrap_err().code, "CANCELLED");
    assert_eq!(b.await.unwrap().unwrap_err().code, "CANCELLED");
    until(|| network.inflight.lock().unwrap().is_empty() && network.gate.available_permits() == 6)
        .await;
    let fresh = fetch(&network, p, &CancellationToken::new());
    until(|| fixture.seen.lock().unwrap().len() == 3).await;
    fixture.release.add_permits(3);
    assert_eq!(fresh.await.unwrap().unwrap().json()["data"], "firstlast");
    server.abort();
}

#[tokio::test]
async fn shared_waiters_keep_own_timeout_and_credentials_never_merge() {
    let (base, fixture, server) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let network = network(dir.path());
    let cancel = CancellationToken::new();
    let a = fetch(
        &network,
        json!({"url":format!("{base}/timeout"),"timeoutMs":150}),
        &cancel,
    );
    let b = fetch(
        &network,
        json!({"url":format!("{base}/timeout"),"timeoutMs":3000}),
        &cancel,
    );
    assert_eq!(a.await.unwrap().unwrap_err().code, "TIMEOUT");
    assert_eq!(fixture.seen.lock().unwrap().len(), 1);
    fixture.release.add_permits(1);
    assert_eq!(b.await.unwrap().unwrap().json()["data"], "firstlast");
    for (index, headers) in [
        json!({"Authorization":"secret"}),
        json!({"Cookie":"key=value"}),
    ]
    .into_iter()
    .enumerate()
    {
        let p = json!({"url":format!("{base}/credentials/{index}"),"headers":headers});
        let a = fetch(&network, p.clone(), &cancel);
        let b = fetch(&network, p, &cancel);
        until(|| fixture.seen.lock().unwrap().len() == 3 + index * 2).await;
        fixture.release.add_permits(2);
        a.await.unwrap().unwrap();
        b.await.unwrap().unwrap();
    }
    network.cookies("set", json!({"url":format!("{base}/private"),"cookie":{"name":"login","value":"value","path":"/private"}})).unwrap();
    let p = json!({"url":format!("{base}/redirect")});
    let a = fetch(&network, p.clone(), &cancel);
    let b = fetch(&network, p, &cancel);
    until(|| {
        fixture
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/private")
            .count()
            == 2
    })
    .await;
    assert!(fixture
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == "/private")
        .all(|(_, h)| h["cookie"] == "login=value"));
    fixture.release.add_permits(2);
    a.await.unwrap().unwrap();
    b.await.unwrap().unwrap();
    let a = fetch(
        &network,
        json!({"url":format!("{base}/distinct"),"headers":{"Accept":"text/plain"}}),
        &cancel,
    );
    let b = fetch(
        &network,
        json!({"url":format!("{base}/distinct"),"headers":{"Accept":"application/json"}}),
        &cancel,
    );
    until(|| {
        fixture
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| path == "/distinct")
            .count()
            == 2
    })
    .await;
    fixture.release.add_permits(2);
    a.await.unwrap().unwrap();
    b.await.unwrap().unwrap();
    server.abort();
}

#[tokio::test]
async fn api_limits_cover_bodies_and_leave_other_hosts_and_media_available() {
    let (base, fixture, server) = fixture().await;
    let other = base.replace("127.0.0.1", "127.0.0.2");
    let dir = tempfile::tempdir().unwrap();
    let network = network(dir.path());
    let cancel = CancellationToken::new();
    let mut requests = Vec::new();
    for index in 0..8 {
        requests.push(fetch(
            &network,
            json!({"url":format!("{base}/one/{index}")}),
            &cancel,
        ));
    }
    until(|| fixture.seen.lock().unwrap().len() == 4).await;
    for index in 0..4 {
        requests.push(fetch(
            &network,
            json!({"url":format!("{other}/two/{index}")}),
            &cancel,
        ));
    }
    until(|| fixture.seen.lock().unwrap().len() == 6).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let seen = fixture.seen.lock().unwrap().clone();
    assert_eq!(
        seen.len(),
        6,
        "response headers must not release a body permit"
    );
    assert_eq!(
        seen.iter().filter(|(p, _)| p.starts_with("/one/")).count(),
        4
    );
    assert_eq!(
        seen.iter().filter(|(p, _)| p.starts_with("/two/")).count(),
        2
    );
    let response = tokio::time::timeout(
        Duration::from_secs(1),
        network.request(
            &format!("{base}/immediate"),
            Method::GET,
            HeaderMap::new(),
            None,
            true,
            true,
            Queue::Media,
            &cancel,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.text().await.unwrap(), "ready");
    fixture.release.add_permits(12);
    for request in requests {
        request.await.unwrap().unwrap();
    }
    assert_eq!(network.gate.available_permits(), 6);
    server.abort();
}

#[tokio::test]
async fn cooldown_is_shared_across_plugins_and_queues_and_cancellation_removes_queued_work() {
    let (base, fixture, server) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let network = network(dir.path());
    let cancel = CancellationToken::new();
    let mut requests = Vec::new();
    for index in 0..5 {
        requests.push(fetch(
            &network,
            json!({"url":format!("{base}/queued/{index}")}),
            &cancel,
        ));
    }
    until(|| fixture.seen.lock().unwrap().len() == 4).await;
    cancel.cancel();
    for request in requests {
        assert_eq!(request.await.unwrap().unwrap_err().code, "CANCELLED");
    }
    until(|| network.inflight.lock().unwrap().is_empty() && network.gate.available_permits() == 6)
        .await;
    assert_eq!(fixture.seen.lock().unwrap().len(), 4);
    let cancel = CancellationToken::new();
    let response = network
        .fetch(
            Transfer::from_json(json!({"url":format!("{base}/cooldown"),"retry":2})),
            cancel.clone(),
        )
        .await
        .unwrap();
    assert_eq!(response.json()["status"], 503);
    let other_dir = tempfile::tempdir().unwrap();
    let mut meta = network.meta.clone();
    meta.id = "fixture.other".into();
    let other = Arc::new(
        Network::new(
            meta,
            other_dir.path().into(),
            Some(false),
            network.cooldown.clone(),
        )
        .unwrap(),
    );
    for queue in [Queue::Api, Queue::Media] {
        let error = other
            .request(
                &format!("{base}/immediate"),
                Method::GET,
                HeaderMap::new(),
                None,
                false,
                true,
                queue,
                &cancel,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "RATE_LIMITED");
    }
    assert_eq!(fixture.seen.lock().unwrap().len(), 5);
    server.abort();
}

#[tokio::test]
async fn plugin_method_and_body_limit_boundaries_remain_independent() {
    let (base, fixture, server) = fixture().await;
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first = network(first_dir.path());
    let second = network(second_dir.path());
    let cancel = CancellationToken::new();
    let p = json!({"url":format!("{base}/scope")});
    let a = fetch(&first, p.clone(), &cancel);
    let b = fetch(&second, p, &cancel);
    until(|| fixture.seen.lock().unwrap().len() == 2).await;
    fixture.release.add_permits(2);
    a.await.unwrap().unwrap();
    b.await.unwrap().unwrap();

    let p = json!({"url":format!("{base}/post"),"method":"POST","body":"value"});
    let a = fetch(&first, p.clone(), &cancel);
    let b = fetch(&first, p, &cancel);
    until(|| fixture.seen.lock().unwrap().len() == 4).await;
    fixture.release.add_permits(2);
    a.await.unwrap().unwrap();
    b.await.unwrap().unwrap();

    let a = fetch(
        &first,
        json!({"url":format!("{base}/limit"),"maxBytes":5}),
        &cancel,
    );
    let b = fetch(
        &first,
        json!({"url":format!("{base}/limit"),"maxBytes":20}),
        &cancel,
    );
    until(|| fixture.seen.lock().unwrap().len() == 6).await;
    fixture.release.add_permits(2);
    assert_eq!(a.await.unwrap().unwrap_err().code, "UPSTREAM_ERROR");
    assert_eq!(b.await.unwrap().unwrap().json()["data"], "firstlast");
    assert_eq!(first.gate.available_permits(), 6);
    server.abort();
}
