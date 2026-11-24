use hghub_hpp::{PluginError, Result};
use serde::Deserialize;
use serde_json::json;
use tauri::plugin::{mobile::PluginInvokeError, PluginHandle};

pub struct AndroidDocuments(pub PluginHandle<tauri::Wry>);

#[derive(Deserialize, serde::Serialize)]
pub struct DocumentChoice {
    pub name: String,
    pub location: String,
}
#[derive(Deserialize)]
struct DirectoryResult {
    files: Vec<DocumentChoice>,
}
#[derive(Deserialize)]
struct ResolvedDocument {
    location: String,
}
fn error(error: PluginInvokeError) -> PluginError {
    if let PluginInvokeError::InvokeRejected(reply) = &error {
        if matches!(
            reply.code.as_deref(),
            Some("UNSUPPORTED" | "INVALID_ARGUMENT")
        ) {
            return PluginError::new(
                reply.code.as_deref().unwrap(),
                reply
                    .message
                    .clone()
                    .unwrap_or_else(|| "无法读取授权目录".into()),
            );
        }
    }
    PluginError::internal(format!("读取插件目录失败：{error}"))
}
impl AndroidDocuments {
    pub async fn pick(&self) -> Result<Vec<DocumentChoice>> {
        self.0
            .run_mobile_plugin_async::<DirectoryResult>("pickDirectory", ())
            .await
            .map(|result| result.files)
            .map_err(error)
    }
    pub fn resolve(&self, base: &str, reference: &str) -> Result<String> {
        self.0
            .run_mobile_plugin::<ResolvedDocument>(
                "resolveEntry",
                json!({"base":base,"reference":reference}),
            )
            .map(|result| result.location)
            .map_err(error)
    }
}
