use super::*;

#[tokio::test]
async fn dropping_initialization_stops_pending_and_running_factory() {
    for factory in [
        "await new Promise(()=>{});",
        "while(true){}",
        "while(true){await Promise.resolve();}",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let meta = Meta::parse(
            json!({"id":"fixture.cancel-init","name":"Cancel init","version":"1.0.0","protocol":1}),
        )
        .unwrap();
        let store = Arc::new(Store::open(dir.path().join("store.json")).unwrap());
        let network = Arc::new(
            Network::new(
                meta.clone(),
                dir.path().into(),
                Some(false),
                Default::default(),
            )
            .unwrap(),
        );
        let logs: Logs = Default::default();
        let source = format!(
            "export const meta={};export default async hg=>{{hg.log.info('started');{factory}}};",
            serde_json::to_string(&meta).unwrap()
        );
        let task = tokio::spawn(Worker::spawn(
            source,
            meta,
            json!({}),
            Value::Null,
            store,
            network.clone(),
            logs.clone(),
            Default::default(),
            None,
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while logs.lock().unwrap().snapshot().is_empty() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        task.abort();
        assert!(task.await.is_err());
        tokio::time::timeout(Duration::from_secs(1), async {
            while Arc::strong_count(&network) > 1 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("cancelled factory thread retained its network");
    }
}
