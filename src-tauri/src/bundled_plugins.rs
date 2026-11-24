use hghub_hpp::{PluginError, PluginManager, Result};
use serde::Deserialize;
use std::path::Path;

const MANIFEST: &str = include_str!("../../bundled-plugins/manifest.json");

#[derive(Deserialize)]
struct Manifest {
    plugins: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    url: String,
}

// Each receipt belongs to the app sandbox and deliberately outlives uninstall.
pub async fn install(manager: &PluginManager, root: &Path) -> Vec<(String, PluginError)> {
    let manifest: Manifest = serde_json::from_str(MANIFEST).expect("invalid embedded HPP manifest");
    let mut errors = Vec::new();
    for entry in manifest.plugins {
        if let Err(error) = install_one(manager, root, &entry).await {
            errors.push((entry.id, error));
        }
    }
    errors
}

async fn install_one(manager: &PluginManager, root: &Path, entry: &Entry) -> Result<()> {
    let code = match entry.url.as_str() {
        "blender-open-movies.js" => include_str!("../../bundled-plugins/blender-open-movies.js"),
        "nasa-svs.js" => include_str!("../../bundled-plugins/nasa-svs.js"),
        "nasa-svs-resolver.js" => include_str!("../../bundled-plugins/nasa-svs-resolver.js"),
        _ => return Err(PluginError::internal("默认插件未编入应用")),
    };
    let receipt = root.join("bundled-installed").join(&entry.id);
    if receipt.try_exists()? {
        return Ok(());
    }
    // Preserve any version the user has already installed, including disabled plugins.
    if manager.meta(&entry.id).await.is_err() {
        manager.import_code(code.to_owned(), None, false).await?;
    }
    std::fs::create_dir_all(root.join("bundled-installed"))?;
    std::fs::File::create_new(receipt)?;
    Ok(())
}
