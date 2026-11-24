use super::*;

#[derive(Clone)]
pub(super) struct Input {
    pub plugin_id: String,
    pub method: String,
    pub params: Value,
    pub selection: PlaybackSelection,
}

struct Waiter<'a> {
    sessions: &'a MediaSessions,
    id: String,
}
impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        self.sessions
            .refresh_waiters
            .lock()
            .unwrap()
            .remove(&self.id);
    }
}

impl MediaSessions {
    pub(crate) fn cancel_refresh_waiter(&self, request_id: &str) {
        if let Some(token) = self.refresh_waiters.lock().unwrap().get(request_id) {
            token.cancel();
        }
    }
}

impl PluginManager {
    pub async fn refresh_media(
        self: &Arc<Self>,
        id: &str,
        version: u64,
        request_id: String,
    ) -> Result<PreparedMedia> {
        if request_id.is_empty() || request_id.len() > 128 {
            return Err(PluginError::invalid("requestId 无效"));
        }
        let cancel = CancellationToken::new();
        {
            let mut waiters = self.sessions.refresh_waiters.lock().unwrap();
            if waiters.contains_key(&request_id) {
                return Err(PluginError::invalid("requestId 已在使用"));
            }
            waiters.insert(request_id.clone(), cancel.clone());
        }
        let _waiter = Waiter {
            sessions: &self.sessions,
            id: request_id,
        };
        self.refresh_media_wait(id, version, &cancel).await
    }

    async fn refresh_media_wait(
        self: &Arc<Self>,
        id: &str,
        version: u64,
        cancel: &CancellationToken,
    ) -> Result<PreparedMedia> {
        let mut receiver = {
            let mut tasks = self.sessions.refreshing.lock().unwrap();
            let session = self
                .sessions
                .sessions
                .lock()
                .unwrap()
                .get(id)
                .cloned()
                .ok_or_else(|| PluginError::new("NOT_FOUND", "媒体会话已释放"))?;
            if session.cancel.is_cancelled() {
                return Err(PluginError::cancelled());
            }
            if version < session.version {
                return session
                    .prepared
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or_else(|| PluginError::invalid("图片会话不能刷新"));
            }
            if version != session.version {
                return Err(PluginError::invalid("媒体版本无效"));
            }
            let mut input = session
                .refresh_input
                .clone()
                .ok_or_else(|| PluginError::invalid("图片会话不能刷新"))?;
            if let Some(task) = tasks.get(id) {
                task.subscribe()
            } else {
                let (sender, receiver) = tokio::sync::watch::channel(None);
                tasks.insert(id.into(), sender.clone());
                let manager = self.clone();
                let id = id.to_string();
                tokio::spawn(async move {
                    input.params["reason"] = json!("refresh");
                    let internal_id = uuid::Uuid::new_v4().to_string();
                    let work = async {
                        let _gate = manager.playback_gate.read().await;
                        for (owner, (_, revision)) in &session.providers {
                            if manager.revision(owner).await? != *revision {
                                return Err(PluginError::cancelled());
                            }
                        }
                        let budget = manager
                            .meta(&input.plugin_id)
                            .await?
                            .budget("media.resolvePlayback");
                        tokio::time::timeout(
                            budget,
                            manager.prepare_inner(
                                &input.plugin_id,
                                &input.method,
                                input.params,
                                input.selection,
                                internal_id.clone(),
                                Some((id.clone(), session.clone())),
                            ),
                        )
                        .await
                        .map_err(|_| PluginError::timeout())?
                    };
                    let result = tokio::select! {
                        biased;
                        _ = session.cancel.cancelled() => Err(PluginError::cancelled()),
                        _ = sender.closed() => Err(PluginError::cancelled()),
                        value = work => value,
                    };
                    if result.is_err() {
                        manager.cancel(&internal_id).await;
                    }
                    sender.send_replace(Some(result));
                    manager.sessions.refreshing.lock().unwrap().remove(&id);
                });
                receiver
            }
        };
        loop {
            if cancel.is_cancelled() {
                return Err(PluginError::cancelled());
            }
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            tokio::select! {
                _ = cancel.cancelled() => return Err(PluginError::cancelled()),
                changed = receiver.changed() => changed.map_err(|_| PluginError::internal("刷新任务结束但缺少结果"))?,
            }
        }
    }
}
