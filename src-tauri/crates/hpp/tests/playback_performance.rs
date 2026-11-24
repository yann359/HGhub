use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Response, Uri},
    routing::any,
    Router,
};
use hghub_hpp::PluginManager;
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

type Seen = Arc<Mutex<Vec<(String, String, SocketAddr)>>>;
const SIZE: usize = 3 * 1024 * 1024;
async fn fixture(
    State(seen): State<Seen>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response<Body> {
    let range = headers
        .get("range")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    seen.lock()
        .unwrap()
        .push((uri.path().into(), range.into(), peer));
    if uri.path() == "/slow.mp4" {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    let changed =
        uri.path() == "/changed.mp4" && (headers.contains_key("if-range") || range == "bytes=0-");
    let range = if uri.path() == "/ignored.mp4" {
        ""
    } else {
        range
    };
    let data = match uri.path() {
        "/root.m3u8" => b"#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=10000\nvideo.m3u8\n".to_vec(),
        "/video.m3u8" => b"#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:10,\none.m4s\n#EXTINF:10,\ntwo.m4s\n#EXT-X-ENDLIST\n".to_vec(),
        "/movie.mp4" | "/changed.mp4" | "/ignored.mp4" | "/slow.mp4" => (0..SIZE).map(|i| if changed { 255 } else { (i % 251) as u8 }).collect(),
        _ => b"fixture".to_vec(),
    };
    let size = data.len();
    let mut out = Response::builder()
        .header(
            "etag",
            if changed {
                "\"fixture-v2\""
            } else {
                "\"fixture-v1\""
            },
        )
        .header("content-type", "application/octet-stream")
        .header("accept-ranges", "bytes");
    let (start, end) = if let Some(raw) = range.strip_prefix("bytes=") {
        let (start, end) = raw.split_once('-').unwrap();
        let (start, end) = if start.is_empty() {
            (size.saturating_sub(end.parse().unwrap()), size - 1)
        } else {
            (
                start.parse::<usize>().unwrap(),
                if end.is_empty() {
                    size - 1
                } else {
                    end.parse::<usize>().unwrap().min(size - 1)
                },
            )
        };
        if start >= size {
            return out.status(416).body(Body::empty()).unwrap();
        }
        out = out
            .status(206)
            .header("content-range", format!("bytes {start}-{end}/{size}"));
        (start, end)
    } else {
        (0, size - 1)
    };
    out.header("content-length", end - start + 1)
        .body(if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(data[start..=end].to_vec())
        })
        .unwrap()
}

#[tokio::test]
async fn connection_pool_and_session_warmup_reuse_real_bytes() {
    let seen: Seen = Default::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/*path", any(fixture))
        .with_state(seen.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::with_mode(dir.path().into(), Some(false)).unwrap());
    let source = format!(
        r#"
export const meta={{id:'fixture.performance',name:'Performance',version:'1.0.0',protocol:1,allowInsecureHttp:true}};
export default hg=>({{
search:{{async search(){{await hg.http.fetch({{url:'{base}/api',credentials:'omit'}});return {{items:[]}};}}}},
media:{{getDetail({{id}}){{return {{id,title:'Fixture',playlists:[{{id:'main',episodes:[{{id:'one',index:1}}]}}]}};}},
resolvePlayback({{mediaId}}){{return {{streams:[{{url:'{base}/'+(mediaId==='hls'?'root.m3u8':mediaId+'.mp4'),format:mediaId==='hls'?'hls':'progressive'}}]}};}}}}
}});
"#
    );
    manager.import_code(source, None, false).await.unwrap();
    for _ in 0..2 {
        manager
            .call(
                "fixture.performance",
                "search.search",
                json!({"query":"fixture"}),
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .unwrap();
    }
    let api = seen.lock().unwrap().clone();
    assert_eq!(api.len(), 2);
    assert_eq!(
        api[0].2, api[1].2,
        "sequential API requests must share a TCP connection"
    );
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for kind in ["movie", "hls"] {
        let media = manager
            .prepare_media(
                "fixture.performance",
                json!({"mediaId":kind,"episodeId":"one","reason":"preload","codecs":["h264"]}),
                0,
                None,
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .unwrap();
        assert!(
            !manager.sessions.has_playback(),
            "preparation must not mark playback active"
        );
        let warmed = manager.preload_media(&media.session_id, 0.0).await.unwrap();
        assert!(warmed["bytes"].as_u64().unwrap() > 0);
        let count = seen.lock().unwrap().len();
        manager.preload_media(&media.session_id, 0.0).await.unwrap();
        assert_eq!(
            seen.lock().unwrap().len(),
            count,
            "warmup retry reuses completed resources"
        );
        if kind == "movie" {
            let response = client
                .get(&media.url)
                .header("range", "bytes=0-1023")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 206);
            assert_eq!(
                response.headers()["content-range"],
                format!("bytes 0-1023/{SIZE}")
            );
            assert_eq!(
                response.bytes().await.unwrap().as_ref(),
                &(0..1024).map(|i| (i % 251) as u8).collect::<Vec<_>>()
            );
            assert_eq!(
                seen.lock().unwrap().len(),
                count,
                "cached bytes must not be downloaded again"
            );
            let response = client
                .get(&media.url)
                .header("range", "bytes=0-")
                .send()
                .await
                .unwrap();
            let data = response.bytes().await.unwrap();
            assert_eq!(data.len(), SIZE);
            assert!(data.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8));
            assert_eq!(
                seen.lock().unwrap().last().unwrap().1,
                format!("bytes={}-{}", 2 * 1024 * 1024, SIZE - 1)
            );
        } else {
            let root = client
                .get(&media.url)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            let variant = root.lines().find(|line| line.starts_with("http:")).unwrap();
            let playlist = client
                .get(variant)
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
            for part in playlist.lines().filter(|line| line.starts_with("http:")) {
                assert_eq!(
                    client
                        .get(part)
                        .send()
                        .await
                        .unwrap()
                        .bytes()
                        .await
                        .unwrap()
                        .as_ref(),
                    b"fixture"
                );
            }
            assert_eq!(
                seen.lock().unwrap().len(),
                count,
                "HLS manifest and starting segments must hit the warm cache"
            );
        }
        manager.sessions.release(&media.session_id);
        assert_eq!(client.get(&media.url).send().await.unwrap().status(), 404);
    }
    for kind in ["changed", "ignored", "slow"] {
        let media = manager
            .prepare_media(
                "fixture.performance",
                json!({"mediaId":kind,"episodeId":"one","reason":"preload","codecs":["h264"]}),
                0,
                None,
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .unwrap();
        if kind == "slow" {
            let mgr = manager.clone();
            let id = media.session_id.clone();
            let warming = tokio::spawn(async move { mgr.preload_media(&id, 0.0).await });
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while !seen.lock().unwrap().iter().any(|r| r.0 == "/slow.mp4") {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            manager.sessions.cancel_preload(&media.session_id);
            assert!(
                tokio::time::timeout(std::time::Duration::from_secs(1), warming)
                    .await
                    .unwrap()
                    .unwrap()
                    .is_err()
            );
        } else {
            let warmed = manager.preload_media(&media.session_id, 0.0).await;
            assert_eq!(warmed.is_ok(), kind == "changed");
            let data = client
                .get(&media.url)
                .header("range", "bytes=0-")
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            assert_eq!(data.len(), SIZE);
            assert!(
                data.iter().enumerate().all(|(i, b)| *b
                    == if kind == "changed" {
                        255
                    } else {
                        (i % 251) as u8
                    }),
                "changed validators must fall back without splicing old bytes"
            );
        }
        manager.sessions.release(&media.session_id);
    }
    server.abort();
}
