use hghub_hpp::PluginManager;
use serde_json::json;

#[tokio::test]
async fn runtime_errors_logging_and_snapshots_follow_contract() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let source = r#"
      export const meta={id:'fixture.contract',name:'Runtime contract',version:'1.0.0',protocol:1};
      export default hg=>({
        media:{getDetail(){},resolvePlayback(){}},
        search:{async search({query}){
          if(query==='errors') {
            const a=new hg.PluginError('NETWORK');
            const b=new hg.PluginError('NOT_FOUND');
            const c=new hg.PluginError('NETWORK','x',{retryable:false});
            const d=new hg.PluginError('unknown','not a public message');
            return {items:[{id:'x',title:JSON.stringify([a.retryable,b.retryable,c.retryable,d.code,d.message])}]};
          }
          if(query==='logs') {console.log({hello:'world'},new Uint8Array([1,2]));hg.log.info('界'.repeat(2048));}
          if(query==='snapshot') {try{hg.env.features.push('fake');}catch{};try{hg.env.app.version='fake';}catch{};return {items:[{id:'x',title:JSON.stringify({app:hg.env.app,features:hg.env.features})}]};}
          if(query==='invalid') return {items:[{id:'x',title:'x'}],bad:()=>{}};
          if(query==='private') return {items:[{id:'x',title:JSON.stringify(Object.getOwnPropertyNames(globalThis).filter(k=>k.startsWith('__hpp')))}]};
          if(query==='atob') {const text=Array.from({length:70000},(_,i)=>String.fromCharCode(i&255)).join('');const decoded=atob(btoa(text));return {items:[{id:'x',title:JSON.stringify([decoded.length,decoded===text])}]};}
          if(query==='sparse') {try{await hg.storage.set('sparse',Array(2));}catch(e){return {items:[{id:'x',title:e.code}]};}}
          return {items:[]};
        }}
      });"#;
    assert_eq!(
        manager
            .import_code(source.into(), None, false)
            .await
            .unwrap()
            .status,
        "ready"
    );
    let value = manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"errors"}),
            "errors".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        value["items"][0]["title"],
        r#"[true,false,false,"INTERNAL","插件出错"]"#
    );
    let value = manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"snapshot"}),
            "snapshot".into(),
        )
        .await
        .unwrap();
    let env: serde_json::Value =
        serde_json::from_str(value["items"][0]["title"].as_str().unwrap()).unwrap();
    assert_eq!(env["app"]["version"], "0.0.1");
    assert!(!env["features"].as_array().unwrap().contains(&json!("fake")));
    manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"logs"}),
            "logs".into(),
        )
        .await
        .unwrap();
    let failure = manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"invalid"}),
            "invalid".into(),
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "INTERNAL");
    let value = manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"private"}),
            "private".into(),
        )
        .await
        .unwrap();
    assert_eq!(value["items"][0]["title"], "[]");
    let value = manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"sparse"}),
            "sparse".into(),
        )
        .await
        .unwrap();
    assert_eq!(value["items"][0]["title"], "INVALID_ARGUMENT");
    let value = manager
        .call(
            "fixture.contract",
            "search.search",
            json!({"query":"atob"}),
            "atob".into(),
        )
        .await
        .unwrap();
    assert_eq!(value["items"][0]["title"], "[70000,true]");
    let logs = manager.logs("fixture.contract").await.unwrap();
    assert!(logs.iter().any(|line| line["text"]
        .as_str()
        .is_some_and(|text| text.contains("world") && text.contains("Uint8Array(2)"))));
    assert!(logs
        .iter()
        .all(|line| line["text"].as_str().unwrap_or("").len() <= 2048));
}
