use hghub_hpp::PluginManager;
use serde_json::json;

#[tokio::test]
async fn console_validation_and_runtime_errors_use_one_bounded_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let source = r#"
export const meta={id:'fixture.logs',name:'Logs',version:'1.0.0',protocol:1};
export default hg=>({media:{getDetail(){},resolvePlayback(){}},search:{search({query}){
 console.info('plugin message');
  throw new hg.PluginError('UPSTREAM_ERROR','runtime message',{details:{reason:'界\n'.repeat(2000)}});
}},home:{getSections(){console.info('plugin message');return {items:[{id:'bad',layout:'invalid',items:[]}]};}}});
"#;
    assert_eq!(
        manager
            .import_code(source.into(), None, false)
            .await
            .unwrap()
            .status,
        "ready"
    );
    for index in 0..130 {
        let query = if index % 2 == 0 { "invalid" } else { "error" };
        let result = manager
            .call(
                "fixture.logs",
                if query == "invalid" {
                    "home.getSections"
                } else {
                    "search.search"
                },
                json!({"query":query}),
                format!("request-{index}"),
            )
            .await;
        assert_eq!(result.is_ok(), query == "invalid");
    }
    let entries = manager.logs("fixture.logs").await.unwrap();
    assert_eq!(entries.len(), 241);
    assert_eq!(entries.last().unwrap()["dropped"], 20);
    assert!(entries
        .iter()
        .any(|entry| entry["text"] == "plugin message"));
    assert!(entries
        .iter()
        .any(|entry| entry["text"] == "runtime message"));
    assert!(entries
        .iter()
        .any(|entry| entry["level"] == "warn" && entry.get("requestId").is_some()));
    assert!(entries
        .iter()
        .all(|entry| serde_json::to_vec(entry).unwrap().len() <= 2048));
}
