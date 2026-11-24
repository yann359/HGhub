use hghub_hpp::PluginManager;
use std::time::{Duration, Instant};

fn source(top_level: &str) -> String {
    format!(
        r#"
{top_level}
export const meta={{id:'fixture.module',name:'Module',version:'1.0.0',protocol:1}};
export default function(){{throw new Error('import must not invoke factory');}}
"#
    )
}

#[tokio::test]
async fn pending_top_level_await_uses_the_full_module_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let started = Instant::now();
    let error = manager
        .import_code(source("await new Promise(()=>{});"), None, false)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "TIMEOUT");
    assert!(started.elapsed() >= Duration::from_millis(4900));
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn top_level_microtasks_do_not_reset_the_total_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let started = Instant::now();
    let error = manager
        .import_code(source("while(true){await Promise.resolve();}"), None, false)
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "TIMEOUT");
    assert!(started.elapsed() >= Duration::from_millis(4900));
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn top_level_synchronous_loop_has_a_one_second_slice() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let started = Instant::now();
    assert_eq!(
        manager
            .import_code(source("while(true){}"), None, false)
            .await
            .err()
            .unwrap()
            .code,
        "TIMEOUT"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn module_jobs_have_separate_slices_without_running_the_factory() {
    let dir = tempfile::tempdir().unwrap();
    let manager = PluginManager::open(dir.path().into()).unwrap();
    let code = source(
        r#"
for(let i=0;i<6;i++) {
  await Promise.resolve();
  const until=Date.now()+220;while(Date.now()<until){}
}
"#,
    );
    let file = dir.path().join("module.js");
    std::fs::write(&file, code).unwrap();
    let preview = manager
        .preview_import(vec![file.to_string_lossy().into_owned()], "module-preview")
        .await
        .unwrap();
    assert!(
        preview.items[0].error.is_none(),
        "{:?}",
        preview.items[0].error
    );
    let report = manager
        .apply_import(
            &preview.token,
            vec![preview.items[0].key.clone()],
            false,
            true,
            "module-install",
        )
        .await
        .unwrap();
    assert_eq!(report.items[0].plugin.as_ref().unwrap().status, "disabled");
}
