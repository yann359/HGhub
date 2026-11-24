//! Tauri adapter; the development harness shares the exact same dispatcher.
use hghub_hpp::{PluginManager, Result};
use serde_json::Value;
use std::sync::Arc;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_fs::FsExt;

/// Document-provider access stays in the host adapter, never in a plugin API.
pub struct PickedDocuments(pub tauri::AppHandle);
impl hghub_hpp::manager::DocumentReader for PickedDocuments {
    #[cfg(target_os = "android")]
    fn resolve_relative(&self, base: &str, reference: &str) -> Result<String> {
        use tauri::Manager;
        self.0
            .state::<crate::android_documents::AndroidDocuments>()
            .resolve(base, reference)
    }
    fn read(&self, location: &str, limit: usize) -> Result<Vec<u8>> {
        use std::io::Read;
        let file: tauri_plugin_fs::FilePath = location
            .parse()
            .map_err(|_| hghub_hpp::PluginError::invalid("文件选择器返回了无效地址"))?;
        let mut bytes = Vec::new();
        let mut options = tauri_plugin_fs::OpenOptions::new();
        options.read(true);
        self.0
            .fs()
            .open(file, options)?
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

#[tauri::command]
pub async fn pick_plugin_directory(app: tauri::AppHandle) -> Result<Value> {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        Ok(serde_json::json!(
            app.state::<crate::android_documents::AndroidDocuments>()
                .pick()
                .await?
        ))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err(hghub_hpp::PluginError::new(
            "UNSUPPORTED",
            "此平台请直接选择本地列表文件",
        ))
    }
}

#[tauri::command]
pub async fn pick_plugin_files(app: tauri::AppHandle) -> Result<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file().add_filter("HPP 插件或列表", &["js", "mjs", "json"]);
        #[cfg(target_os = "ios")]
        let dialog = dialog.set_file_access_mode(tauri_plugin_dialog::FileAccessMode::Copy);
        let files = dialog.blocking_pick_files().unwrap_or_default();
        #[cfg(target_os = "ios")]
        return ios_import_paths(&app, files);
        #[cfg(not(target_os = "ios"))]
        Ok(files.into_iter().map(|file| file.to_string()).collect())
    })
    .await
    .map_err(|_| hghub_hpp::PluginError::internal("无法打开文件选择器"))?
}

#[cfg(target_os = "ios")]
fn ios_import_paths(app: &tauri::AppHandle, files: Vec<tauri_plugin_dialog::FilePath>) -> Result<Vec<String>> {
    use std::{fs, io::Read, path::PathBuf};
    use tauri::Manager;
    let paths = files.into_iter().map(|file| file.into_path().map_err(|_| {
        hghub_hpp::PluginError::invalid("无法读取文件选择器返回的地址")
    })).collect::<Result<Vec<PathBuf>>>()?;
    let is_manifest = |path: &PathBuf| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json"));
    let manifests = paths.iter().filter(|path| is_manifest(path)).count();
    if manifests != 1 || paths.len() == 1 {
        return Ok(paths.into_iter().map(|path| path.to_string_lossy().into_owned()).collect());
    }
    // iOS Files may place selected copies in separate Inbox folders.
    // Only grant the manifest access to files selected in this same operation.
    if paths.len() > 500 {
        return Err(hghub_hpp::PluginError::invalid("一次最多选择 500 个文件"));
    }
    let cache = app.path().app_cache_dir()
        .map_err(|_| hghub_hpp::PluginError::internal("无法定位插件导入缓存"))?;
    fs::create_dir_all(&cache)?;
    let staging = tempfile::tempdir_in(cache)?;
    let mut manifest = None;
    for path in &paths {
        let name = path.file_name().ok_or_else(|| hghub_hpp::PluginError::invalid("无法识别导入文件名"))?;
        let destination = staging.path().join(name);
        let mut bytes = Vec::new();
        fs::File::open(path)?.take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(hghub_hpp::PluginError::invalid("插件文件超过 2 MiB"));
        }
        use std::io::Write;
        let mut file = fs::File::create_new(&destination)?;
        file.write_all(&bytes)?;
        if is_manifest(path) {
            manifest = Some(destination.to_string_lossy().into_owned());
        }
    }
    let host = app.state::<PluginHost>();
    *host.import_staging.lock().map_err(|_| hghub_hpp::PluginError::internal("无法保存导入会话"))? = Some(staging);
    Ok(manifest.into_iter().collect())
}

pub struct PluginHost {
    dir: std::path::PathBuf,
    ready: tokio::sync::OnceCell<Arc<PluginManager>>,
    #[cfg(target_os = "ios")]
    bundled: tokio::sync::OnceCell<()>,
    #[cfg(target_os = "ios")]
    import_staging: std::sync::Mutex<Option<tempfile::TempDir>>,
}
impl PluginHost {
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self {
            dir,
            ready: Default::default(),
            #[cfg(target_os = "ios")]
            bundled: Default::default(),
            #[cfg(target_os = "ios")]
            import_staging: Default::default(),
        }
    }
}

#[tauri::command]
pub async fn hpp_manage(
    app: tauri::AppHandle,
    host: tauri::State<'_, PluginHost>,
    action: String,
    params: Value,
) -> Result<Value> {
    let manager = host
        .ready
        .get_or_try_init(|| async {
            let manager = PluginManager::open(host.dir.clone())?
                .with_document_reader(Arc::new(PickedDocuments(app.clone())));
            #[cfg(target_os = "windows")]
            let manager = match crate::windows_login::WindowsLogin::detect() {
                Some(provider) => manager.with_web_login(Arc::new(provider)),
                None => manager,
            };
            #[cfg(target_os = "android")]
            let manager = {
                use tauri::Manager;
                let provider = app.state::<crate::android_login::AndroidLogin>();
                if provider.supported().await? {
                    manager.with_web_login(Arc::new(provider.inner().clone()))
                } else {
                    manager
                }
            };
            Ok::<_, hghub_hpp::PluginError>(Arc::new(manager))
        })
        .await?;
    // The frontend registers the actual WKWebView capabilities before plugin initialization.
    #[cfg(target_os = "ios")]
    if action != "host.media.configure" {
        host.bundled.get_or_init(|| async {
            for (id, error) in crate::bundled_plugins::install(manager, &host.dir).await {
                let message = format!("[bundled] {id}: {}", error.code);
                if let Some(parent) = host.dir.parent() {
                    let _ = crate::logging::append_debug(parent, &message);
                }
            }
        }).await;
    }
    hghub_hpp::ipc::dispatch(manager, action, params).await
}
