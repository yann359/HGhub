use super::*;
use futures_util::{stream, StreamExt};
use reqwest::{header::HeaderMap, Method};
use std::time::Duration;

impl PluginManager {
    pub async fn endpoint_list(
        &self,
        id: &str,
        refresh: bool,
        request_id: String,
    ) -> Result<Value> {
        let revision = self.revision(id).await?;
        let endpoints = self
            .call(id, "endpoints.list", json!({"refresh":refresh}), request_id)
            .await?;
        let selected = {
            let plugin = self.get(id).await?;
            let p = plugin.lock().await;
            endpoints
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|e| e["id"] == p.entry.endpoint["id"])
                        .or_else(|| a.first())
                })
                .ok_or_else(|| PluginError::internal("线路列表为空，保留原线路"))?
                .clone()
        };
        let _mutation = self.mutation.lock().await;
        let plugin = self.get(id).await?;
        {
            let mut p = plugin.lock().await;
            if p.revision != revision || !p.entry.enabled {
                return Err(PluginError::cancelled());
            }
            if p.entry.endpoint != selected {
                self.sessions.release_plugin(id);
                if let Some(w) = p.worker.take() {
                    w.stop_and_wait().await?;
                }
                p.entry.endpoint = selected;
                p.revision = uuid::Uuid::new_v4().to_string();
                if let Err(e) = self.initialize(&mut p, false).await {
                    p.error = Some(e);
                }
            }
        }
        self.save().await?;
        Ok(endpoints)
    }
    pub async fn endpoint_probe(&self, id: &str, request_id: String) -> Result<Value> {
        let op = self.distribution.begin(&request_id)?;
        let listing = self.call(id, "endpoints.list", json!({"refresh":false}), request_id);
        let endpoints = tokio::select! { _=op.cancel.cancelled()=>return Err(PluginError::cancelled()), result=listing=>result? };
        let network = self.network(id).await?;
        let results = stream::iter(endpoints.as_array().unwrap().iter().cloned().enumerate().map(|(index, e)| {
            let network = network.clone(); let cancel = op.cancel.clone();
            async move {
                let started = Instant::now();
                let url = format!("{}/", e["url"].as_str().unwrap().trim_end_matches('/'));
                let result = tokio::time::timeout(Duration::from_secs(8), network.request(&url, Method::GET, HeaderMap::new(), None, false, true, crate::network::Queue::Api, &cancel)).await;
                let mut item = json!({"id":e["id"],"url":e["url"],"latencyMs":started.elapsed().as_millis() as u64});
                match result {
                    Ok(Ok(response)) => { item["status"] = json!(response.status().as_u16()); item["ok"] = json!(response.status().is_success()); },
                    Ok(Err(e)) => { item["ok"] = json!(false); item["error"] = json!(e.message); },
                    Err(_) => { item["ok"] = json!(false); item["error"] = json!("探测超时"); },
                }
                (index,item)
            }
        })).buffer_unordered(4).collect::<Vec<_>>().await;
        if op.cancel.is_cancelled() {
            return Err(PluginError::cancelled());
        }
        let mut results = results;
        results.sort_by_key(|(i, _)| *i);
        Ok(json!(results
            .into_iter()
            .map(|(_, v)| v)
            .collect::<Vec<_>>()))
    }
}
