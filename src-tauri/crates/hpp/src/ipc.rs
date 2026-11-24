//! Thin IPC adapter; protocol execution and media transport live in hghub-hpp.
use crate::{PluginError, PluginManager, Result};
use serde_json::{json, Value};
use std::sync::Arc;

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .ok_or_else(|| PluginError::invalid(format!("缺少 {key}")))
}
fn stream_index(p: &Value, key: &str) -> Result<usize> {
    match p.get(key) {
        None => Ok(0),
        Some(v) => v
            .as_u64()
            .filter(|n| *n < 16)
            .map(|n| n as usize)
            .ok_or_else(|| PluginError::invalid(format!("{key} 必须为 0 至 15 的整数"))),
    }
}

pub async fn dispatch(
    manager: &Arc<PluginManager>,
    action: String,
    params: Value,
) -> Result<Value> {
    let p = &params;
    if let Some(action) = action.strip_prefix("library.") {
        return manager.library.operation(action, p);
    }
    let value = match action.as_str() {
        "host.media.configure" => json!({"changed": manager.configure_media(
            serde_json::from_value(p.clone()).map_err(|_| PluginError::invalid("播放能力格式无效"))?
        ).await?}),
        "proxy.get" => crate::proxy::get(),
        "proxy.set" => crate::proxy::set(text(p, "mode")?, p["url"].as_str().unwrap_or(""))
            .map_err(PluginError::invalid)?,
        "proxy.test" => crate::proxy::probe(
            text(p, "mode")?,
            p["url"].as_str().unwrap_or(""),
            text(p, "target")?,
        )
        .await
        .map_err(PluginError::invalid)?,
        "list" => json!(manager.list().await),
        "updates.state" => json!(manager.refresh_update_state().await?),
        "updates.policy.set" => json!(manager.update_policy_set(
            serde_json::from_value(p.clone()).map_err(|_| PluginError::invalid(
                "更新偏好必须包含 checkOnStartup 与 autoInstall 布尔值"
            ))?
        )?),
        "updates.startup" => json!(manager.startup_updates(text(p, "requestId")?).await?),
        "updates.tick" => json!(
            manager
                .apply_pending_updates(None, text(p, "requestId")?, true)
                .await?
        ),
        "updates.install" => json!(
            manager
                .apply_pending_updates(
                    Some(
                        serde_json::from_value(p["ids"].clone())
                            .map_err(|_| PluginError::invalid("缺少 ids 数组"))?
                    ),
                    text(p, "requestId")?,
                    false
                )
                .await?
        ),
        "updates.dismiss" => json!(manager.dismiss_update(text(p, "id")?)?),
        "subscriptions.save" => {
            json!(manager.subscription_save(text(p, "token")?, text(p, "location")?)?)
        }
        "subscriptions.remove" => json!(manager.subscription_remove(text(p, "id")?)?),
        "subscriptions.refresh" => json!(
            manager
                .subscription_refresh(text(p, "id")?, text(p, "requestId")?)
                .await?
        ),
        "resolver.preferences.get" => json!(manager.resolver_preferences.get()),
        "resolver.preferences.set" => {
            let id = match p.get("pluginId") {
                None | Some(Value::Null) => None,
                Some(Value::String(id)) => Some(id.as_str()),
                _ => return Err(PluginError::invalid("pluginId 必须为解析器 ID 或 null")),
            };
            if let Some(id) = id {
                if !manager.list().await.iter().any(|plugin| {
                    plugin.meta.id == id
                        && plugin.meta.kind == "resolver"
                        && plugin.enabled
                        && matches!(plugin.status.as_str(), "ready" | "unloaded")
                }) {
                    return Err(PluginError::invalid("只能记住当前可用的解析器"));
                }
            }
            manager.resolver_preferences.set(text(p, "host")?, id)?;
            Value::Null
        }
        "resolver.preferences.clear" => {
            manager.resolver_preferences.clear()?;
            Value::Null
        }
        "distribution.preview" => json!(
            manager
                .preview_import(
                    serde_json::from_value(p["locations"].clone())
                        .map_err(|_| PluginError::invalid("缺少 locations 数组"))?,
                    text(p, "requestId")?
                )
                .await?
        ),
        "distribution.apply" => json!(
            manager
                .apply_import(
                    text(p, "token")?,
                    serde_json::from_value(p["selected"].clone())
                        .map_err(|_| PluginError::invalid("缺少 selected 数组"))?,
                    p["allowDowngrade"].as_bool().unwrap_or(false),
                    p["installOnly"].as_bool().unwrap_or(false),
                    text(p, "requestId")?
                )
                .await?
        ),
        "distribution.discard" => {
            manager.discard_import(text(p, "token")?);
            Value::Null
        }
        "updates.check" => json!(
            manager
                .check_updates(
                    serde_json::from_value(p["ids"].clone())
                        .map_err(|_| PluginError::invalid("缺少 ids 数组"))?,
                    text(p, "requestId")?
                )
                .await?
        ),
        "restore" => json!(manager.restore_previous(text(p, "id")?).await?),
        "import" => json!(
            manager
                .import_code_mode(
                    text(p, "code")?.into(),
                    p["source"].as_str().map(String::from),
                    p["allowDowngrade"].as_bool().unwrap_or(false),
                    p["installOnly"].as_bool().unwrap_or(false)
                )
                .await?
        ),
        "enable" => json!(
            manager
                .set_enabled(
                    text(p, "id")?,
                    p["enabled"]
                        .as_bool()
                        .ok_or_else(|| PluginError::invalid("缺少 enabled"))?
                )
                .await?
        ),
        "retry" => json!(manager.retry(text(p, "id")?).await?),
        "remove" => {
            manager.remove(text(p, "id")?).await?;
            Value::Null
        }
        "settings.get" => manager.settings_get(text(p, "id")?).await?,
        "settings.set" => json!(
            manager
                .settings_set(text(p, "id")?, text(p, "key")?.into(), p["value"].clone())
                .await?
        ),
        "endpoint.list" => {
            manager
                .endpoint_list(
                    text(p, "id")?,
                    p["refresh"].as_bool().unwrap_or(false),
                    text(p, "requestId")?.into(),
                )
                .await?
        }
        "endpoint.probe" => {
            manager
                .endpoint_probe(text(p, "id")?, text(p, "requestId")?.into())
                .await?
        }
        "endpoint.set" => json!(
            manager
                .endpoint_set(text(p, "id")?, text(p, "endpointId")?)
                .await?
        ),
        "logs" => json!(manager.logs(text(p, "id")?).await?),
        "form.actions" => {
            let method = match text(p, "target")? {
                "auth" => "auth.getMethods",
                "ui" => "ui.getActions",
                _ => return Err(PluginError::invalid("未知表单目标")),
            };
            manager
                .call(
                    text(p, "id")?,
                    method,
                    json!({}),
                    text(p, "requestId")?.into(),
                )
                .await?
        }
        "form.submit" => {
            let target = &p["target"];
            let (method, key) = match text(target, "kind")? {
                "auth" => ("auth.submit", "methodId"),
                "ui" => ("ui.submit", "actionId"),
                _ => return Err(PluginError::invalid("未知表单目标")),
            };
            let mut args = serde_json::Map::new();
            args.insert(key.into(), json!(text(target, "id")?));
            args.insert("values".into(), p["values"].clone());
            manager
                .call_scoped(
                    text(p, "id")?,
                    method,
                    crate::wire::Transfer::from_json(Value::Object(args)),
                    text(p, "requestId")?.into(),
                    method == "auth.submit",
                )
                .await?.json()
        }
        "call" => {
            manager
                .call(
                    text(p, "id")?,
                    text(p, "method")?,
                    p["args"].clone(),
                    text(p, "requestId")?.into(),
                )
                .await?
        }
        "cancel" => {
            manager.cancel(text(p, "requestId")?).await;
            Value::Null
        }
        "media.prepare" | "live.prepare" | "article.prepare" => json!(
            manager
                .prepare_content_selected(
                    text(p, "id")?,
                    match action.as_str() {
                        "live.prepare" => "live.resolve",
                        "article.prepare" => "articles.getDetail",
                        _ => "media.resolvePlayback",
                    },
                    p["args"].clone(),
                    crate::media::PlaybackSelection {
                        stream_index: stream_index(p, "streamIndex")?,
                        resolver_id: p["resolverId"].as_str().map(String::from),
                        resolver_host: p["resolverHost"].as_str().map(String::from),
                        resolver_servers: if p.get("resolverServers").is_some() {
                            serde_json::from_value(p["resolverServers"].clone()).map_err(|_| {
                                PluginError::invalid(
                                    "resolverServers 必须为插件 ID 到线路 ID 的对象",
                                )
                            })?
                        } else {
                            Default::default()
                        },
                        resolver_stream_index: stream_index(p, "resolverStreamIndex")?,
                    },
                    text(p, "requestId")?.into()
                )
                .await?
        ),
        "media.refresh" => json!(manager.refresh_media(text(p, "sessionId")?, p["version"].as_u64().ok_or_else(|| PluginError::invalid("缺少媒体版本"))?, text(p, "requestId")?.into()).await?),
        "media.preload" => {
            manager
                .preload_media(
                    text(p, "sessionId")?,
                    p["positionSec"]
                        .as_f64()
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .unwrap_or(0.0),
                )
                .await?
        }
        "media.preload.cancel" => {
            manager.sessions.cancel_preload(text(p, "sessionId")?);
            Value::Null
        }
        "media.release" => {
            manager.sessions.release(text(p, "sessionId")?);
            Value::Null
        }
        "media.activity" => {
            let _playback = manager.playback_gate.read().await;
            manager.sessions.activity(
                text(p, "sessionId")?,
                p["playing"]
                    .as_bool()
                    .ok_or_else(|| PluginError::invalid("缺少 playing"))?,
                p["version"].as_u64(),
            )?;
            Value::Null
        }
        "assets.prepare" => {
            manager
                .prepare_assets(
                    text(p, "id")?,
                    p["assets"]
                        .as_array()
                        .ok_or_else(|| PluginError::invalid("缺少 assets"))?
                        .clone(),
                )
                .await?
        }
        _ => return Err(PluginError::new("UNSUPPORTED", "未知插件管理操作")),
    };
    Ok(value)
}
