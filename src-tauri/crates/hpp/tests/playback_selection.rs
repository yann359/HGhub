use hghub_hpp::{ipc, model, PluginManager};
use serde_json::{json, Value};
use std::sync::Arc;

#[tokio::test]
async fn external_streams_require_confirmation_without_creating_media_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::with_mode(dir.path().into(), Some(false)).unwrap());
    let source = r#"
      export const meta={id:'fixture.external',name:'External',version:'1.0.0',protocol:1};
      export default hg=>({media:{getDetail({id}){return {id,title:id,playlists:[{id:'main',episodes:[]}]};},
        resolvePlayback(p){return {streams:[{url:p.episodeId,kind:'external',external:{app:'Player',mime:'video/mp4'},headers:{Cookie:'secret',Authorization:'secret'}}]};}
      }});
    "#;
    manager
        .import_code(source.into(), None, false)
        .await
        .unwrap();
    for url in [
        "https://media.example/movie",
        "http://media.example/movie",
        "magnet:?xt=urn:btih:fixture",
        "ed2k://|file|fixture|1|abc|/",
        "rtsp://media.example/live",
        "rtmp://media.example/live",
    ] {
        for reason in ["play", "preload", "refresh"] {
            let error = manager
                .prepare_media(
                    "fixture.external",
                    json!({"mediaId":"one","episodeId":url,"reason":reason,"codecs":[]}),
                    0,
                    None,
                    format!("external-{reason}"),
                )
                .await
                .err()
                .unwrap();
            assert_eq!(error.code, "INTERACTION_REQUIRED", "{url}: {error:?}");
            let choices = error.playback_choices.unwrap();
            assert_eq!(choices["external"]["url"], url);
            assert_eq!(choices["external"]["app"], "Player");
            assert!(!choices.to_string().contains("secret"));
            assert!(!manager.sessions.has_playback());
        }
    }
    for url in [
        "javascript:alert(1)",
        "file:///tmp/video",
        "data:video/mp4,test",
        "https://user:password@media.example/video",
        "https://media.example/\nvideo",
    ] {
        assert!(model::url(url, true, true).is_err());
    }
    let error: hghub_hpp::PluginError = serde_json::from_value(json!({"code":"INTERACTION_REQUIRED","message":"fixture","playbackChoices":{"external":{"url":"https://forged.example"}}})).unwrap();
    assert!(error.playback_choices.is_none());
    assert!(model::playback(json!({"streams":[{"kind":"external","url":"https://media.example","external":{"app":{}}}]}), false, false).is_err());
    let valid = model::playback(json!({"streams":[{"kind":"external","url":"https://media.example","external":{"app":"Player","private":"discard"}}]}), false, false).unwrap();
    assert_eq!(valid["streams"][0]["external"], json!({"app":"Player"}));
    manager.import_code(r#"export const meta={id:'fixture.external.resolver',name:'Resolver',kind:'resolver',matches:['media.example'],version:'1.0.0',protocol:1};
        export default hg=>({resolver:{resolve(){return {streams:[{kind:'external',url:'magnet:?xt=urn:btih:resolved'}]};}}});"#.into(), None, false).await.unwrap();
    manager
        .import_code(
            source.replace("kind:'external',external:", "kind:'page',external:"),
            None,
            false,
        )
        .await
        .unwrap();
    let error = manager.prepare_media("fixture.external", json!({"mediaId":"one","episodeId":"https://media.example/page","reason":"play","codecs":[]}), 0, None, "external-resolver".into()).await.err().unwrap();
    let choices = error.playback_choices.unwrap();
    assert_eq!(choices["external"]["url"], "magnet:?xt=urn:btih:resolved");
    assert_eq!(choices["resolver"]["pluginId"], "fixture.external.resolver");
}

#[tokio::test]
async fn source_and_resolver_choices_route_to_their_own_stage_without_exposing_resources() {
    let dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(PluginManager::with_mode(dir.path().into(), Some(false)).unwrap());
    let source = r#"
      export const meta={id:'fixture.choices',name:'虚构选择源',version:'1.0.0',protocol:1};
      export default hg=>({media:{getDetail({id}){return {id,title:id,playlists:[{id:'main',episodes:[]}]};},
        async resolvePlayback(p){
          await hg.storage.set('last',p);
          if(p.serverId&&p.serverId!=='source-b')throw new Error('resolver server leaked to source');
          const serverId=p.serverId||'source-a';
          return {servers:[{id:'source-a',label:'源 A',secret:'discard'},{id:'source-b',label:'源 B'}],
            streams:[{url:'https://media.example/direct.mp4',format:'progressive',serverId,label:'直接播放',headers:{Authorization:'secret'}},
              {url:'https://page.example/watch',kind:'page',serverId,label:'播放页',resolverId:'fixture.choice.resolver'}],
            subtitles:[{url:'https://media.example/source.srt',format:'srt',default:true,label:'Source'},
              {url:'https://media.example/ignored.ass',format:'ass',default:true}],
            durationSec:100,expiresAt:9000000000000};
      }}});
    "#;
    let resolver = r#"
      export const meta={id:'fixture.choice.resolver',name:'虚构选择解析器',version:'1.0.0',protocol:1,kind:'resolver',matches:['page.example']};
      export default hg=>({resolver:{async resolve(p){
        await hg.storage.set('last',p);
        if(p.serverId&&p.serverId!=='resolver-b')throw new Error('source server leaked to resolver');
        const serverId=p.serverId||'resolver-a';
        return {servers:[{id:'resolver-a',label:'解析 A'},{id:'resolver-b',label:'解析 B'}],
          streams:[{url:'https://media.example/a.mp4',format:'progressive',label:'720p',height:720,serverId},
            {url:'https://media.example/b.mp4',format:'progressive',label:'1080p',height:1080,serverId,headers:{Cookie:'secret'}}],
          subtitles:[{url:'https://media.example/resolver.srt',format:'srt',default:true,label:'Resolver'},
            {url:'https://media.example/second.vtt',format:'vtt',default:true}],
          durationSec:120,expiresAt:8000000000000};
      }}});
    "#;
    for code in [source, resolver] {
        assert_eq!(
            manager
                .import_code(code.into(), None, false)
                .await
                .unwrap()
                .status,
            "ready"
        );
    }
    let params = json!({"id":"fixture.choices","requestId":"select", "streamIndex":1,
        "resolverId":"unavailable.preference", "resolverStreamIndex":1,
        "resolverServers":{"fixture.choice.resolver":"resolver-b","unrelated.resolver":"wrong"},
        "args":{"mediaId":"one","episodeId":"ep","serverId":"source-b","reason":"refresh","codecs":["h264"],"maxHeight":1080}});
    let prepared = ipc::dispatch(&manager, "media.prepare".into(), params)
        .await
        .unwrap();
    assert_eq!(prepared["providerId"], "fixture.choice.resolver");
    assert_eq!(prepared["choices"]["source"]["streamIndex"], 1);
    assert_eq!(prepared["choices"]["source"]["serverId"], "source-b");
    assert_eq!(prepared["choices"]["resolver"]["streamIndex"], 1);
    assert_eq!(prepared["choices"]["resolver"]["serverId"], "resolver-b");
    assert_eq!(
        prepared["choices"]["source"]["streams"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(prepared["playback"]["durationSec"], 120);
    let tracks = prepared["playback"]["subtitles"].as_array().unwrap();
    assert_eq!(tracks.len(), 3);
    assert!(tracks.iter().all(|t| t["format"] == "vtt"));
    let defaults: Vec<_> = tracks.iter().filter(|t| t["default"] == true).collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0]["label"], "Resolver");
    assert_eq!(prepared["playback"]["expiresAt"], 8000000000000u64);
    let refreshed = manager
        .refresh_media(
            prepared["sessionId"].as_str().unwrap(),
            0,
            "refresh-selected-resolver".into(),
        )
        .await
        .unwrap();
    assert_eq!(refreshed.version, 1);
    assert_eq!(refreshed.choices, prepared["choices"]);
    let visible = prepared.to_string();
    for hidden in [
        "secret",
        "Authorization",
        "Cookie",
        "https://page.example/watch",
        "media.example",
    ] {
        assert!(!visible.contains(hidden), "{hidden} leaked");
    }
    // The second source stream is pinned to this resolver, so a stale saved preference cannot override it.
    for (id, stage_server) in [
        ("fixture.choices", "source-b"),
        ("fixture.choice.resolver", "resolver-b"),
    ] {
        let saved: Value = serde_json::from_slice(
            &std::fs::read(dir.path().join(id).join("storage.json")).unwrap(),
        )
        .unwrap();
        assert!(saved.to_string().contains(stage_server));
        assert!(!saved.to_string().contains(if id == "fixture.choices" {
            "resolver-b"
        } else {
            "source-b"
        }));
    }
    manager
        .sessions
        .release(prepared["sessionId"].as_str().unwrap());
    // Defaults have no serverId argument; in particular they must not contain null as a string option.
    let default = manager
        .prepare_media(
            "fixture.choices",
            json!({"mediaId":"one","episodeId":"ep","reason":"play","codecs":["h264"]}),
            1,
            None,
            "default".into(),
        )
        .await
        .unwrap();
    assert_eq!(default.choices["resolver"]["serverId"], "resolver-a");
    manager.sessions.release(&default.session_id);
    manager
        .set_enabled("fixture.choice.resolver", false)
        .await
        .unwrap();
    let unavailable = manager
        .prepare_media(
            "fixture.choices",
            json!({"mediaId":"one","episodeId":"ep","reason":"play","codecs":["h264"]}),
            1,
            None,
            "unavailable".into(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(unavailable.code, "UNSUPPORTED");
    assert_eq!(
        unavailable.playback_choices.unwrap()["source"]["streams"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let spoofed: hghub_hpp::PluginError = serde_json::from_value(
        json!({"code":"AUTH_REQUIRED","message":"test","playbackChoices":{"source":"spoof"}}),
    )
    .unwrap();
    assert!(spoofed.playback_choices.is_none());
    for invalid in [json!(-1), json!("1"), json!(16)] {
        let error = ipc::dispatch(
            &manager,
            "media.prepare".into(),
            json!({"id":"fixture.choices","requestId":"invalid","streamIndex":invalid}),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.code, "INVALID_ARGUMENT");
    }
}

#[test]
fn playback_option_validation_discards_bad_rows_and_strips_unknown_fields() {
    let result=model::validate_result("media.resolvePlayback",json!({
        "streams":[{"url":"https://media.example/a","format":"progressive","label":{"bad":true}},
          {"url":"https://media.example/b","format":"progressive","height":1080,"codec":"h264","label":"高清"}],
        "servers":[{"id":"a","label":"主线","secret":"discard"},{"id":"a","label":"重复"},{"id":2}],
    }),false).unwrap();
    assert_eq!(result["streams"].as_array().unwrap().len(), 1);
    assert_eq!(result["servers"], json!([{"id":"a","label":"主线"}]));
}
