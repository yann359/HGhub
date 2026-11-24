use hghub_hpp::{ipc, PluginManager};
use serde_json::{json, Value};
use std::sync::Arc;

const SOURCE: &str = r#"
export const meta={id:'fixture.capabilities',name:'Capabilities',version:'1.0.0',protocol:1};
export default hg=>{
  const snapshot=JSON.stringify(hg.env.media);
  try { hg.env.media.codecs.push('fake'); } catch {}
  try { hg.env.media.formats=[]; } catch {}
  if(JSON.stringify(hg.env.media)!==snapshot)throw new Error('mutable environment');
  const resolve=async p=>{
    if(JSON.stringify(p.codecs)!==JSON.stringify(hg.env.media.codecs))throw new Error('codec mismatch');
    await hg.storage.set('last',{codecs:p.codecs,reason:p.reason});
    return {streams:[p.mediaId==='page'
      ? {url:'https://page.example/watch',kind:'page',resolverId:'fixture.capabilities.resolver'}
      : {url:'https://media.example/video.mp4',format:'progressive'}]};
  };
  return {
    media:{getDetail(){},resolvePlayback:resolve},
    live:{getGroups(){return []},listChannels(){return {items:[]}},resolve},
    search:{async search({query}){return {items:[{id:'x',title:query==='last'?JSON.stringify(await hg.storage.get('last')):snapshot}]}}}
  };
};
"#;
const RESOLVER: &str = r#"
export const meta={id:'fixture.capabilities.resolver',name:'Resolver',version:'1.0.0',protocol:1,kind:'resolver',matches:['page.example']};
export default hg=>({resolver:{resolve(p){
  if(JSON.stringify(p.codecs)!==JSON.stringify(hg.env.media.codecs))throw new Error('resolver codec mismatch');
  return {streams:[{url:'https://media.example/video.mp4',format:'progressive',label:JSON.stringify(p.codecs)}]};
}}});
"#;

async fn read(manager: &PluginManager, query: &str) -> Value {
    let result = manager
        .call(
            "fixture.capabilities",
            "search.search",
            json!({"query":query}),
            uuid::Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    serde_json::from_str(result["items"][0]["title"].as_str().unwrap()).unwrap()
}

async fn configure(manager: &Arc<PluginManager>, value: Value) -> Value {
    ipc::dispatch(manager, "host.media.configure".into(), value)
        .await
        .unwrap()
}

#[tokio::test]
async fn media_snapshot_registration_reload_and_all_playback_calls_agree() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::open(dir.path().into()).unwrap());
    for source in [SOURCE, RESOLVER] {
        assert_eq!(
            manager
                .import_code(source.into(), None, false)
                .await
                .unwrap()
                .status,
            "ready"
        );
    }
    assert_eq!(
        read(&manager, "env").await,
        json!({"codecs":[],"formats":[]})
    );
    let capabilities = json!({"codecs":["hevc","h264"],"formats":["progressive","hls"]});
    assert_eq!(
        configure(&manager, capabilities.clone()).await["changed"],
        true
    );
    assert_eq!(read(&manager, "env").await, capabilities);
    let revision = manager.list().await[0].revision.clone();
    for reason in ["play", "preload", "refresh"] {
        for action in ["media.prepare", "live.prepare"] {
            let result = ipc::dispatch(&manager, action.into(), json!({"id":"fixture.capabilities","requestId":uuid::Uuid::new_v4().to_string(),
                "args":{"mediaId":"one","episodeId":"ep","channelId":"channel","reason":reason,"codecs":["av1"]}})).await.unwrap();
            assert_eq!(
                read(&manager, "last").await,
                json!({"codecs":["hevc","h264"],"reason":reason})
            );
            // Idempotent registration must preserve a live media session and revision.
            assert_eq!(
                configure(&manager, capabilities.clone()).await["changed"],
                false
            );
            assert!(manager
                .sessions
                .activity(result["sessionId"].as_str().unwrap(), true, None)
                .is_ok());
            assert_eq!(manager.list().await[0].revision, revision);
            manager
                .sessions
                .release(result["sessionId"].as_str().unwrap());
        }
    }
    let prepared = manager
        .prepare_media(
            "fixture.capabilities",
            json!({"mediaId":"page","episodeId":"ep","reason":"play"}),
            0,
            None,
            "resolver".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        prepared.choices["resolver"]["streams"][0]["label"],
        r#"["hevc","h264"]"#
    );
    assert_eq!(
        configure(
            &manager,
            json!({"codecs":["av1"],"formats":["progressive"]})
        )
        .await["changed"],
        true
    );
    assert!(manager
        .sessions
        .activity(&prepared.session_id, true, None)
        .is_err());
    assert_eq!(
        read(&manager, "env").await,
        json!({"codecs":["av1"],"formats":["progressive"]})
    );
    assert_ne!(manager.list().await[0].revision, revision);
    manager.retry("fixture.capabilities").await.unwrap();
    assert_eq!(read(&manager, "env").await["codecs"], json!(["av1"]));
    // Device capabilities are not persisted as stale claims across app restarts.
    drop(manager);
    let reopened = PluginManager::open(dir.path().into()).unwrap();
    assert_eq!(
        read(&reopened, "env").await,
        json!({"codecs":[],"formats":[]})
    );
}

#[tokio::test]
async fn media_capability_ipc_rejects_unknown_or_invalid_claims() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::open(dir.path().into()).unwrap());
    for value in [
        json!(null),
        json!({"codecs":"h264","formats":[]}),
        json!({"codecs":["vp9"],"formats":["hls"]}),
        json!({"codecs":["h264"],"formats":["dash"]}),
        json!({"codecs":["h264","h264"],"formats":["hls"]}),
        json!({"codecs":[],"formats":["hls","hls"]}),
        json!({"codecs":["h264"],"formats":[]}),
    ] {
        assert_eq!(
            ipc::dispatch(&manager, "host.media.configure".into(), value)
                .await
                .unwrap_err()
                .code,
            "INVALID_ARGUMENT"
        );
    }
}
