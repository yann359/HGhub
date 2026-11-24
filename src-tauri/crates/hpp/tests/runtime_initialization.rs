use hghub_hpp::PluginManager;
use serde_json::json;
use std::time::{Duration, Instant};

fn source(factory: &str) -> String {
    format!(
        r#"
export const meta={{id:'fixture.initialization',name:'Initialization',version:'1.0.0',protocol:1}};
export default async hg=>{{
{factory}
return {{media:{{getDetail({{id}}){{return {{id,title:'ready',playlists:[{{id:'main',episodes:[]}}]}};}},resolvePlayback(){{return {{streams:[{{url:'https://fixture.invalid/movie.mp4',format:'progressive'}}]}};}}}}}};
}};
"#
    )
}

#[tokio::test]
async fn pending_factory_waits_for_five_second_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let started = Instant::now();
    let info = manager
        .import_code(source("await new Promise(()=>{});"), None, false)
        .await
        .unwrap();
    assert_eq!(info.error.unwrap().code, "TIMEOUT");
    assert!(started.elapsed() >= Duration::from_millis(4900));
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn factory_promise_jobs_have_separate_slices_and_keep_io_forbidden() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let factory = r#"
for(let i=0;i<6;i++) {
  await Promise.resolve();
  const until=Date.now()+220;while(Date.now()<until){}
}
hg.settings.get('optional');hg.endpoint.current;hg.env.plugin.id;
hg.log.info(hg.codec.utf8Decode(hg.codec.utf8Encode('initialized')));
hg.crypto.randomBytes(1);
for(const forbidden of [
 ()=>hg.http.fetch({url:'https://fixture.invalid/never'}),
 ()=>hg.storage.get('x'),()=>hg.storage.set('x','y'),
 ()=>hg.cookies.get({url:'https://fixture.invalid'}),
 ()=>hg.html.parse('<p>never</p>'),()=>setTimeout(()=>{},0),
]) {
  let code;try{await forbidden();}catch(e){code=e.code;}
  if(code!=='UNSUPPORTED')throw new Error('factory IO was allowed');
}
await new Promise(resolve=>queueMicrotask(resolve));
"#;
    let info = manager
        .import_code(source(factory), None, false)
        .await
        .unwrap();
    assert_eq!(info.status, "ready", "{:?}", info.error);
    assert_eq!(
        manager
            .call(
                "fixture.initialization",
                "media.getDetail",
                json!({"id":"one"}),
                "ready".into()
            )
            .await
            .unwrap()["title"],
        "ready"
    );
}

#[tokio::test]
async fn synchronous_factory_loop_is_interrupted_before_total_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let started = Instant::now();
    let info = manager
        .import_code(source("while(true){}"), None, false)
        .await
        .unwrap();
    assert_eq!(info.error.unwrap().code, "TIMEOUT");
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn endless_microtasks_do_not_reset_factory_total_budget() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let started = Instant::now();
    let info = manager
        .import_code(source("while(true){await Promise.resolve();}"), None, false)
        .await
        .unwrap();
    assert_eq!(info.error.unwrap().code, "TIMEOUT");
    assert!(started.elapsed() >= Duration::from_millis(4900));
    assert!(started.elapsed() < Duration::from_secs(8));
}
