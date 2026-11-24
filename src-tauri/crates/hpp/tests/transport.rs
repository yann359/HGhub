use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Response, Uri},
    routing::any,
    Router,
};
use hghub_hpp::PluginManager;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
const SOURCE: &str = include_str!("fixtures/synthetic.js");
type Requests = Arc<Mutex<Vec<(String, HeaderMap)>>>;

async fn fixture(
    State(seen): State<Requests>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response<Body> {
    seen.lock()
        .unwrap()
        .push((uri.to_string(), headers.clone()));
    let mut out = Response::builder();
    let body=match uri.path(){
        "/cookie"=>{out=out.header("set-cookie","login=fixture; Path=/; HttpOnly").header("content-type","application/json");json!({"cookie":headers.get("cookie").and_then(|v|v.to_str().ok()).unwrap_or("")}).to_string()},
        "/slow"=>{tokio::time::sleep(Duration::from_millis(300)).await;"slow".into()},
        "/start"=>return out.status(302).header("location","/hls/master.m3u8?signature=master").body(Body::empty()).unwrap(),
        "/hls/master.m3u8"=>"#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",URI=\"audio.m3u8?audio=1\"\n#EXT-X-STREAM-INF:BANDWIDTH=100000,AUDIO=\"a\"\nvariant.m3u8?variant=2\n".into(),
        "/hls/variant.m3u8"|"/hls/audio.m3u8"=>"#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin?key=3\"\n#EXT-X-MAP:URI=\"init.mp4?init=4\"\n#EXTINF:1,\nsegment.ts?segment=5\n#EXT-X-ENDLIST\n".into(),
        "/movie.mp4"=>{
            let bytes="0123456789";
            if let Some(range)=headers.get("range").and_then(|v|v.to_str().ok()){
                if range=="bytes=2-5"{out=out.status(206).header("content-range","bytes 2-5/10").header("content-length","4");"2345".into()}
                else{return out.status(416).header("content-range","bytes */10").body(Body::empty()).unwrap();}
            }else{out=out.header("content-length",bytes.len());bytes.into()}
        },
        "/image"=>"fixture-image".into(),
        "/legacy"=>return out.header("content-type","text/html; Charset=GBK").body(Body::from(&b"\xb5\xda1\xbc\xaf"[..])).unwrap(),
        "/subtitle"=>{out=out.header("content-type","application/octet-stream");"1\r\n00:00:01,000 --> 00:00:02,500\r\n字幕\r\n".into()},
        _=>"fixture-resource".into()
    };
    out.body(if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(body)
    })
    .unwrap()
}
async fn server() -> (String, Requests, tokio::task::JoinHandle<()>) {
    let seen: Requests = Default::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/*path", any(fixture))
        .with_state(seen.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, seen, task)
}
fn manager(dir: &tempfile::TempDir) -> Arc<PluginManager> {
    Arc::new(PluginManager::with_mode(dir.path().into(), Some(false)).unwrap())
}
fn params() -> Value {
    json!({"mediaId":"one","episodeId":"ep1","playlistId":"main","reason":"play","codecs":["h264"]})
}

#[tokio::test]
async fn http_text_uses_response_charset_case_insensitively() {
    let (base, _, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    let source = format!(
        r#"
      export const meta={{id:'fixture.charset',name:'Charset',version:'1.0.0',protocol:1,allowInsecureHttp:true}};
      export default hg=>({{media:{{getDetail(){{}},resolvePlayback(){{}}}},
        search:{{async search(){{const r=await hg.http.fetch({{url:'{base}/legacy'}});return {{items:[{{id:'x',title:r.data}}]}};}}}}
      }});
    "#
    );
    m.import_code(source, None, false).await.unwrap();
    let value = m
        .call(
            "fixture.charset",
            "search.search",
            json!({"query":"x"}),
            "charset".into(),
        )
        .await
        .unwrap();
    assert_eq!(value["items"][0]["title"], "第1集");
    task.abort();
}

#[tokio::test]
async fn shared_refresh_switches_resources_atomically_and_cancels_independent_waiters() {
    let (base, seen, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    let source = format!(
        r#"
      export const meta={{id:'fixture.refresh',name:'Refresh',version:'1.0.0',protocol:1,allowInsecureHttp:true}};
      export default hg=>({{media:{{getDetail({{id}}){{return {{id,title:id,playlists:[{{id:'main',episodes:[]}}]}};}},
        async resolvePlayback(p,ctx){{
          if(p.reason==='refresh')await hg.http.fetch({{url:'{base}/slow',signal:ctx.signal}});
          if(p.reason==='refresh' && p.mediaId==='fail' && !await hg.storage.get('failed')){{await hg.storage.set('failed',true);throw new hg.PluginError('NETWORK','fixture failure');}}
          if(p.mediaId==='hls')return {{streams:[{{url:'{base}/hls/master.m3u8',format:'hls'}}]}};
          return {{streams:[{{url:'{base}/movie.mp4?generation='+p.reason,format:'progressive'}}]}};
        }}
      }}}});
    "#
    );
    m.import_code(source, None, false).await.unwrap();
    let media = m
        .prepare_media(
            "fixture.refresh",
            params(),
            0,
            None,
            "prepare-refresh".into(),
        )
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let (a, b) = tokio::join!(
        m.refresh_media(&media.session_id, 0, "refresh-a".into()),
        m.refresh_media(&media.session_id, 0, "refresh-b".into())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.session_id, media.session_id);
    assert_eq!(a.version, 1);
    assert_eq!(a.url, b.url);
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == "/slow")
            .count(),
        1
    );
    assert_eq!(client.get(&media.url).send().await.unwrap().status(), 410);
    assert_eq!(
        client
            .get(&a.url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "0123456789"
    );
    let late = m
        .refresh_media(&media.session_id, 0, "late".into())
        .await
        .unwrap();
    assert_eq!(late.version, 1);
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == "/slow")
            .count(),
        1
    );
    let (cancelled, remaining, _) = tokio::join!(
        m.refresh_media(&media.session_id, 1, "cancelled-waiter".into()),
        m.refresh_media(&media.session_id, 1, "remaining-waiter".into()),
        async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            m.cancel("cancelled-waiter").await;
        }
    );
    assert_eq!(cancelled.err().unwrap().code, "CANCELLED");
    let remaining = remaining.unwrap();
    assert_eq!(remaining.version, 2);
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == "/slow")
            .count(),
        2
    );
    let (released, _) = tokio::join!(
        m.refresh_media(&media.session_id, 2, "released".into()),
        async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            m.sessions.release(&media.session_id);
        }
    );
    assert_eq!(released.err().unwrap().code, "CANCELLED");
    assert_eq!(
        client.get(&remaining.url).send().await.unwrap().status(),
        404
    );
    assert!(!m.sessions.has_playback());
    let failed = m
        .prepare_media(
            "fixture.refresh",
            json!({"mediaId":"fail","episodeId":"ep","reason":"play"}),
            0,
            None,
            "prepare-fail".into(),
        )
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        m.refresh_media(&failed.session_id, 0, "fail-a".into()),
        m.refresh_media(&failed.session_id, 0, "fail-b".into())
    );
    assert_eq!(a.err().unwrap().code, "NETWORK");
    assert_eq!(b.err().unwrap().code, "NETWORK");
    assert_eq!(client.get(&failed.url).send().await.unwrap().status(), 200);
    assert_eq!(
        m.refresh_media(&failed.session_id, 0, "retry".into())
            .await
            .unwrap()
            .version,
        1
    );
    let hls = m
        .prepare_media(
            "fixture.refresh",
            json!({"mediaId":"hls","episodeId":"ep","reason":"play"}),
            0,
            None,
            "prepare-hls".into(),
        )
        .await
        .unwrap();
    let old = client
        .get(&hls.url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let child = old.lines().find(|line| line.starts_with("http")).unwrap();
    let updated = m
        .refresh_media(&hls.session_id, 0, "refresh-hls".into())
        .await
        .unwrap();
    assert_eq!(client.get(child).send().await.unwrap().status(), 410);
    let new = client
        .get(&updated.url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let child = new.lines().find(|line| line.starts_with("http")).unwrap();
    assert!(child.ends_with("?v=1"));
    assert_eq!(client.get(child).send().await.unwrap().status(), 200);
    m.sessions
        .activity(&hls.session_id, false, Some(0))
        .unwrap();
    assert!(m.sessions.has_playback());
    let (abandoned, _) = tokio::join!(
        m.refresh_media(&hls.session_id, 1, "abandoned".into()),
        async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            m.cancel("abandoned").await;
        }
    );
    assert_eq!(abandoned.err().unwrap().code, "CANCELLED");
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        client.get(&updated.url).send().await.unwrap().status(),
        200,
        "cancelled refresh must not commit later"
    );
    m.sessions.release(&hls.session_id);
    let (disabled, _) = tokio::join!(
        m.refresh_media(&failed.session_id, 1, "disable-refresh".into()),
        async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            m.set_enabled("fixture.refresh", false).await.unwrap();
        }
    );
    assert_eq!(disabled.err().unwrap().code, "CANCELLED");
    assert!(!m.sessions.has_playback());
    task.abort();
}

#[tokio::test]
async fn srt_tracks_use_declared_format_and_isolate_failures() {
    let (base, seen, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    let playback = json!({"streams":[{"url":format!("{base}/movie.mp4"),"format":"progressive"}],
        "subtitles":[
            {"url":format!("{base}/subtitle"),"format":"srt","language":"zh","label":"中文","default":true},
            {"url":format!("{base}/subtitle"),"format":"vtt","language":"en","label":"Original"},
            {"url":format!("{base}/bad"),"format":"srt","label":"坏字幕"}]});
    let source = SOURCE.replace(
        "{streams:[{url:\"https://media.example/movie.mp4\",format:\"progressive\"}]}",
        &playback.to_string(),
    );
    m.import_code(source, None, false).await.unwrap();
    let media = m
        .prepare_media("fixture.source", params(), 0, None, "subtitle".into())
        .await
        .unwrap();
    let tracks = media.playback["subtitles"].as_array().unwrap();
    assert_eq!(tracks.len(), 3);
    assert_eq!(tracks[0]["format"], "vtt");
    assert_eq!(tracks[0]["language"], "zh");
    assert_eq!(tracks[0]["label"], "中文");
    assert_eq!(tracks[0]["default"], true);
    assert_ne!(tracks[0]["url"], tracks[1]["url"]);
    let client = reqwest::Client::new();
    let url = tracks[0]["url"].as_str().unwrap();
    let response = client.get(url).send().await.unwrap();
    assert_eq!(
        response.headers()["content-type"],
        "text/vtt; charset=utf-8"
    );
    let converted = response.bytes().await.unwrap();
    assert_eq!(
        &converted[..],
        "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\n字幕\n\n".as_bytes()
    );
    let head = client.head(url).send().await.unwrap();
    assert_eq!(
        head.headers()["content-length"],
        converted.len().to_string()
    );
    assert!(head.bytes().await.unwrap().is_empty());
    let range = client
        .get(url)
        .header("range", "bytes=0-5")
        .send()
        .await
        .unwrap();
    assert_eq!(range.status(), 206);
    assert_eq!(range.text().await.unwrap(), "WEBVTT");
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == "/subtitle")
        .all(|(_, headers)| !headers.contains_key("range")));
    assert_eq!(
        client
            .get(tracks[2]["url"].as_str().unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        502
    );
    assert_eq!(
        client
            .get(&media.url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "0123456789"
    );
    m.sessions.release(&media.session_id);
    assert_eq!(client.get(url).send().await.unwrap().status(), 404);
    task.abort();
}
fn http_source(base: &str) -> String {
    SOURCE.replace("if(query===\"spin\")",&format!(r#"
        if(query==="http"||query==="http-once") {{
          const a=await hg.http.fetch({{url:"{base}/cookie",responseType:"json"}});
          query=a.data.cookie||"<empty>";
        }}
        if(query==="http-cancel") {{await hg.http.fetch({{url:"{base}/slow"}});await hg.storage.set("late-write",true);}}
        if(query==="spin")"#))
}

#[tokio::test]
async fn http_cookies_scope_and_implicit_cancellation() {
    let (base, seen, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    m.import_code(http_source(&base), None, false)
        .await
        .unwrap();
    m.import_code(
        http_source(&base).replace("fixture.source", "fixture.other"),
        None,
        false,
    )
    .await
    .unwrap();
    for (id, expected) in [
        ("fixture.source", "<empty>"),
        ("fixture.source", "login=fixture"),
        ("fixture.other", "<empty>"),
    ] {
        let v = m
            .call(
                id,
                "search.search",
                json!({"query":"http"}),
                uuid::Uuid::new_v4().to_string(),
            )
            .await
            .unwrap();
        assert_eq!(v["items"][0]["title"], expected);
    }
    let copy = m.clone();
    let pending = tokio::spawn(async move {
        copy.call(
            "fixture.source",
            "search.search",
            json!({"query":"http-cancel"}),
            "cancel-http".into(),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if seen.lock().unwrap().iter().any(|(u, _)| u == "/slow") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    m.cancel("cancel-http").await;
    assert_eq!(pending.await.unwrap().unwrap_err().code, "CANCELLED");
    tokio::time::sleep(Duration::from_millis(350)).await;
    let result = m
        .call(
            "fixture.source",
            "search.search",
            json!({"query":"late-check"}),
            "check".into(),
        )
        .await
        .unwrap();
    assert_eq!(result["items"][0]["title"], "undefined");
    task.abort();
}
#[tokio::test]
async fn shared_promises_and_completed_calls_do_not_leak_scope() {
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    m.import_code(SOURCE.into(), None, false).await.unwrap();
    let (a, b) = tokio::join!(
        m.call(
            "fixture.source",
            "search.search",
            json!({"query":"shared"}),
            "shared-a".into()
        ),
        m.call(
            "fixture.source",
            "search.search",
            json!({"query":"shared"}),
            "shared-b".into()
        )
    );
    assert_eq!(a.unwrap()["items"][0]["title"], "shared-a");
    assert_eq!(b.unwrap()["items"][0]["title"], "shared-b");
    m.call(
        "fixture.source",
        "search.search",
        json!({"query":"late"}),
        "late".into(),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        m.call(
            "fixture.source",
            "search.search",
            json!({"query":"late-check"}),
            "check".into()
        )
        .await
        .unwrap()["items"][0]["title"],
        "undefined"
    );
}
#[tokio::test]
async fn progressive_range_head_and_release() {
    let (base, _seen, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    m.import_code(
        SOURCE.replace(
            "https://media.example/movie.mp4",
            &format!("{base}/movie.mp4"),
        ),
        None,
        false,
    )
    .await
    .unwrap();
    let media = m
        .prepare_media("fixture.source", params(), 0, None, "prepare".into())
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let response = client
        .get(&media.url)
        .header("range", "bytes=2-5")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
    assert_eq!(response.text().await.unwrap(), "2345");
    let head = client.head(&media.url).send().await.unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], "10");
    assert!(head.bytes().await.unwrap().is_empty());
    assert_eq!(
        client
            .get(&media.url)
            .header("range", "bytes=30-40")
            .send()
            .await
            .unwrap()
            .status(),
        416
    );
    m.sessions.release(&media.session_id);
    assert_eq!(client.get(&media.url).send().await.unwrap().status(), 404);
    task.abort();
}
#[tokio::test]
async fn hls_redirect_master_keys_maps_and_queries() {
    let (base, seen, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    let source = SOURCE
        .replace("https://media.example/movie.mp4", &format!("{base}/start"))
        .replace("format:\"progressive\"", "format:\"hls\"");
    m.import_code(source, None, false).await.unwrap();
    let media = m
        .prepare_media("fixture.source", params(), 0, None, "prepare".into())
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let master = client
        .get(&media.url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(master.contains("#EXT-X-STREAM-INF"));
    assert!(!master.contains("signature="));
    let variant = master.lines().find(|s| s.starts_with("http")).unwrap();
    let playlist = client
        .get(variant)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let uris = regex::Regex::new(r#"URI="([^"]+)""#).unwrap();
    for capture in uris.captures_iter(&playlist) {
        let r = client.get(&capture[1]).send().await.unwrap();
        assert_eq!(r.status(), 200);
    }
    let segment = playlist.lines().find(|s| s.starts_with("http")).unwrap();
    assert_eq!(client.get(segment).send().await.unwrap().status(), 200);
    let requests = seen.lock().unwrap();
    for url in [
        "/hls/variant.m3u8?variant=2",
        "/hls/key.bin?key=3",
        "/hls/init.mp4?init=4",
        "/hls/segment.ts?segment=5",
    ] {
        assert!(
            requests.iter().any(|(u, _)| u == url),
            "missing {url}: {requests:?}"
        );
    }
    task.abort();
}
#[tokio::test]
async fn endpoint_initialization_and_settings_revoke_media() {
    let (base, _, task) = server().await;
    let dir = tempfile::tempdir().unwrap();
    let m = manager(&dir);
    let source=SOURCE.replace("return {\n    media:",&format!(r#"return {{
      endpoints:{{async list(){{if(hg.endpoint.current===null)await hg.storage.set("initial-null",true);return [{{id:"a",url:"{base}"}},{{id:"b",url:"http://localhost:12345"}}];}}}},
      media:"#)).replace("title:hg.settings.get(\"label\")","title:hg.endpoint.url('/movie.mp4')").replace("https://media.example/movie.mp4",&format!("{base}/movie.mp4"));
    let info = m.import_code(source, None, false).await.unwrap();
    assert_eq!(info.status, "ready", "{:?}", info.error);
    assert_eq!(info.endpoint["id"], "a");
    let detail = m
        .call(
            "fixture.source",
            "media.getDetail",
            json!({"id":"one"}),
            "detail".into(),
        )
        .await
        .unwrap();
    assert_eq!(detail["title"], format!("{base}/movie.mp4"));
    let media = m
        .prepare_media("fixture.source", params(), 0, None, "prepare".into())
        .await
        .unwrap();
    m.settings_set("fixture.source", "label".into(), json!("changed"))
        .await
        .unwrap();
    assert_eq!(reqwest::get(media.url).await.unwrap().status(), 404);
    let info = m.endpoint_set("fixture.source", "b").await.unwrap();
    assert_eq!(info.endpoint["id"], "b");
    task.abort();
}
