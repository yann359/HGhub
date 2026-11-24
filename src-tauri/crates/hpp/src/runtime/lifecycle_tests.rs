use super::*;

#[tokio::test]
async fn healthy_runtime_disposes_once_with_no_io_and_failed_hook_does_not_block_stop() {
    for hook in [
        "for(const op of [()=>hg.http.fetch({url:'https://fixture.invalid/never'}),()=>hg.storage.set('late',true),()=>hg.html.parse('<p>x</p>'),()=>setTimeout(()=>{},0)]){try{op();throw new Error('allowed');}catch(e){if(e.code!=='CANCELLED')throw e;}}hg.log.info('cleaned');",
        "throw new Error('dispose failed');",
        "while(true){}",
        "return new Promise(()=>{});",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let meta = Meta::parse(json!({"id":"fixture.lifecycle","name":"Lifecycle","version":"1.0.0","protocol":1})).unwrap();
        let store = Arc::new(Store::open(dir.path().join("store.json")).unwrap());
        let network = Arc::new(Network::new(meta.clone(), dir.path().into(), Some(false), Default::default()).unwrap());
        let logs: Logs = Default::default();
        let source = format!("export const meta={};export default hg=>({{media:{{getDetail(){{}},resolvePlayback(){{}}}},dispose(){{hg.log.info('dispose');{hook}}}}});", serde_json::to_string(&meta).unwrap());
        let worker = Worker::spawn(source, meta, json!({}), Value::Null, store.clone(), network, logs.clone(), Default::default(), None).await.unwrap();
        let started = Instant::now();
        worker.stop_and_wait().await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        let entries = logs.lock().unwrap().snapshot();
        assert_eq!(entries.iter().filter(|entry| entry["text"] == "dispose").count(), 1);
        if hook.starts_with("for") { assert!(entries.iter().any(|entry| entry["text"] == "cleaned")); }
        if hook.starts_with("throw") || hook.starts_with("while") {
            assert!(entries.iter().any(|entry| entry["text"] == "插件卸载通知失败"));
        }
        assert_eq!(store.operation("keys", &Transfer::from_json(json!({"prefix":""}))).unwrap().json(), json!([]));
    }
}

#[tokio::test]
async fn interrupted_runtime_skips_dispose() {
    let dir = tempfile::tempdir().unwrap();
    let logs: Logs = Default::default();
    let stop = Arc::new(AtomicBool::new(false));
    let engine = Engine::new(logs.clone(), Default::default(), stop).unwrap();
    let meta = engine.inspect("export const meta={id:'fixture.skip',name:'Skip',version:'1.0.0',protocol:1};export default hg=>({media:{getDetail(){while(true){}},resolvePlayback(){}},dispose(){hg.log.info('must not run');}});").unwrap();
    engine
        .install(&meta, &json!({}), &Value::Null, &Default::default(), false)
        .unwrap();
    engine
        .invoke(
            "start",
            (
                "loop",
                "media.getDetail",
                serde_json::to_string(&Transfer::from_json(json!({}))).unwrap(),
                now() + 10000,
            ),
        )
        .unwrap();
    assert!(engine.pump().is_err());
    drop(engine);
    assert!(!logs
        .lock()
        .unwrap()
        .snapshot()
        .iter()
        .any(|entry| entry["text"] == "must not run"));
    drop(dir);
}
